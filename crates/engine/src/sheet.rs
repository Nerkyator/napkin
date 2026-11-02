//! Evaluates a whole sheet, line by line.

use crate::Context;
use crate::currency;
use crate::eval::{Env, Flags};
use crate::lexer;
use crate::num::{self, Num};
use crate::parser::{self, Hl, Parsed, Parser};
use crate::units::{self, UnitSpec};
use crate::value::{Style, Value};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Empty,
    Header,
    Comment,
    /// No value found: plain text.
    Text,
    Expression,
    Assignment,
}

/// A highlighted range of the line, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub hl: Hl,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub kind: LineKind,
    pub value: Option<Value>,
    /// The result as shown; `None` when the line has none.
    pub text: Option<String>,
    /// Why a calculation failed, for the tooltip.
    pub error: Option<String>,
    pub spans: Vec<Span>,
    pub flags: Flags,
}

impl Line {
    fn bare(kind: LineKind, spans: Vec<Span>) -> Line {
        Line { kind, value: None, text: None, error: None, spans, flags: Flags::default() }
    }
}

#[derive(Clone, Debug)]
pub struct Sheet {
    pub lines: Vec<Line>,
    /// Sum of every result that can be added, shown at the foot of the window.
    pub total: Option<String>,
}

pub fn evaluate(text: &str, ctx: &Context) -> Sheet {
    let mut state = State {
        ctx,
        vars: HashMap::new(),
        prev: None,
        block: Vec::new(),
        totals: Vec::new(),
        ppi: ctx.ppi.clone(),
        em_px: ctx.em_px.clone(),
        labels: HashSet::new(),
    };
    let lines = text.split('\n').map(|line| state.line(line.trim_end_matches('\r'))).collect();
    let total = state.total();
    Sheet { lines, total }
}

struct State<'a> {
    ctx: &'a Context,
    vars: HashMap<String, Value>,
    prev: Option<Value>,
    block: Vec<Value>,
    totals: Vec<Value>,
    ppi: Num,
    em_px: Num,
    labels: HashSet<String>,
}

struct Assignment {
    name: String,
    start: usize,
    end: usize,
    rhs: usize,
}

/// Replaces a byte range with spaces, keeping every position in place.
fn blank(text: &mut String, start: usize, end: usize) {
    let spaces = " ".repeat(end - start);
    text.replace_range(start..end, &spaces);
}

/// Text between double quotes is a comment.
fn quoted_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut open: Option<(usize, char)> = None;
    for (i, c) in text.char_indices() {
        match (open, c) {
            (None, '"' | '“') => open = Some((i, c)),
            (Some((start, '"')), '"') | (Some((start, '“')), '”') => {
                ranges.push((start, i + c.len_utf8()));
                open = None;
            }
            _ => {}
        }
    }
    ranges
}

/// The colon that ends a label: not between two digits, as in `2:30`.
fn label_colon(text: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if c != ':' {
            continue;
        }
        let before = k.checked_sub(1).map(|j| chars[j].1);
        let after = chars.get(k + 1).map(|p| p.1);
        if before.is_some_and(|b| b.is_ascii_digit()) && after.is_some_and(|a| a.is_ascii_digit()) {
            continue;
        }
        return if text[..i].trim().is_empty() { None } else { Some(i) };
    }
    None
}

/// `name = expression`.
fn find_assignment(text: &str) -> Option<Assignment> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    let start = i;
    if i >= bytes.len() || !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return None;
    }
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    let end = i;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'=') || matches!(bytes.get(i + 1), Some(b'=' | b'>')) {
        return None;
    }
    Some(Assignment { name: text[start..end].to_string(), start, end, rhs: i + 1 })
}

/// Names of units, currencies, functions and keywords cannot be assigned,
/// except the two settings `ppi` and `em`.
fn is_reserved(name: &str) -> bool {
    if name == "ppi" || name == "em" {
        return false;
    }
    parser::is_reserved_word(&name.to_lowercase()) || units::is_unit_word(name) || currency::is_currency_word(name)
}

impl State<'_> {
    fn style(&self) -> Style<'_> {
        Style {
            precision: self.ctx.precision,
            format: self.ctx.format,
            clock: self.ctx.clock,
            today: self.ctx.today(),
            rates: self.ctx.rates.as_deref(),
        }
    }

    fn env(&self, rates_date: Option<chrono::NaiveDate>) -> Env<'_> {
        Env {
            rates_date,
            ctx: self.ctx,
            vars: &self.vars,
            prev: self.prev.as_ref(),
            block: &self.block,
            ppi: &self.ppi,
            em_px: &self.em_px,
            labels: &self.labels,
            flags: Cell::new(Flags::default()),
        }
    }

    fn line(&mut self, line: &str) -> Line {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            self.block.clear();
            return Line::bare(LineKind::Empty, Vec::new());
        }
        let whole = vec![Span { start: line.len() - line.trim_start().len(), end: line.len(), hl: Hl::Comment }];
        if trimmed.starts_with('#') {
            self.block.clear();
            return Line::bare(LineKind::Header, whole);
        }
        if trimmed.starts_with("//") {
            return Line::bare(LineKind::Comment, whole);
        }

        let mut spans = Vec::new();
        let mut masked = line.to_string();
        if let Some(at) = masked.find("//") {
            spans.push(Span { start: at, end: line.len(), hl: Hl::Comment });
            blank(&mut masked, at, line.len());
        }
        for (start, end) in quoted_ranges(&masked) {
            spans.push(Span { start, end, hl: Hl::Comment });
            blank(&mut masked, start, end);
        }
        if let Some(colon) = label_colon(&masked) {
            let start = masked.len() - masked.trim_start().len();
            spans.push(Span { start, end: colon + 1, hl: Hl::Label });
            self.labels.insert(masked[..colon].trim().to_string());
            blank(&mut masked, 0, colon + 1);
        }
        let assignment = find_assignment(&masked);
        if let Some(a) = &assignment {
            spans.push(Span { start: a.start, end: a.end, hl: Hl::Variable });
            blank(&mut masked, 0, a.rhs);
        }

        let tokens = lexer::lex(&masked, self.ctx.format);
        let vars = &self.vars;
        let is_var = |word: &str| vars.contains_key(word);
        let parsed = Parser::new(&tokens, &is_var, self.ctx.rates.as_deref()).parse();
        for (token, hl) in tokens.iter().zip(&parsed.hl) {
            if let Some(hl) = hl {
                spans.push(Span { start: token.start, end: token.end, hl: *hl });
            }
        }
        spans.sort_by_key(|s| s.start);

        let kind = if assignment.is_some() { LineKind::Assignment } else { LineKind::Expression };
        let reserved = assignment.as_ref().is_some_and(|a| is_reserved(&a.name));
        let rates_date = self.rates_date(&parsed);
        let env = self.env(rates_date.clone().unwrap_or(None));
        let result = if reserved {
            Some(Err("this name belongs to a unit, currency or keyword".to_string()))
        } else if parsed.failed {
            Some(Err("two values without an operator".to_string()))
        } else if let Err(error) = rates_date {
            Some(Err(error))
        } else {
            parsed.expr.as_ref().map(|expr| env.eval(expr))
        };
        let flags = env.flags.get();

        let value = match result {
            None if assignment.is_none() => return Line::bare(LineKind::Text, spans),
            None => return Line { error: Some("nothing to assign".into()), ..Line::bare(kind, spans) },
            Some(Err(error)) => return Line { error: Some(error), flags, ..Line::bare(kind, spans) },
            Some(Ok(value)) => value,
        };
        if let Some(a) = &assignment {
            if let Err(error) = self.assign(&a.name, &value) {
                return Line { error: Some(error), flags, ..Line::bare(kind, spans) };
            }
        }
        let text = value.show(&self.style());
        self.prev = Some(value.clone());
        if !flags.aggregate {
            self.block.push(value.clone());
            if assignment.is_none() && !matches!(value, Value::Percent(_)) {
                self.totals.push(value.clone());
            }
        }
        Line { kind, value: Some(value), text: Some(text), error: None, spans, flags }
    }

    /// The day after `on`, whose exchange rates the line uses.
    fn rates_date(&self, parsed: &Parsed) -> Result<Option<chrono::NaiveDate>, String> {
        let Some(expr) = &parsed.rates_date else { return Ok(None) };
        match self.env(None).eval(expr)? {
            Value::Date(date) => Ok(Some(date)),
            Value::Instant(t) => Ok(Some(t.zone.local(&t.utc).date())),
            _ => Err("on needs a date".to_string()),
        }
    }

    fn assign(&mut self, name: &str, value: &Value) -> Result<(), String> {
        match name {
            "ppi" => match value {
                Value::Number(n, _) if num::is_negative(n) || *n == num::zero() => Err("ppi must be positive".into()),
                Value::Number(n, _) => {
                    self.ppi = n.clone();
                    Ok(())
                }
                _ => Err("ppi is a number of pixels per inch".into()),
            },
            "em" => {
                let px = match value {
                    Value::Number(n, _) => n.clone(),
                    Value::Quantity(n, unit) => {
                        let pixel = UnitSpec::Pixel.resolve(&self.ppi, &self.em_px).ok_or("invalid ppi")?;
                        units::convert(n, unit, &pixel).ok_or("em is a length")?
                    }
                    _ => return Err("em is a length".into()),
                };
                self.em_px = px;
                Ok(())
            }
            _ => {
                self.vars.insert(name.to_string(), value.clone());
                Ok(())
            }
        }
    }

    fn total(&self) -> Option<String> {
        let mut values = self.totals.iter();
        let first = values.next()?.clone();
        let env = self.env(None);
        let sum = values.try_fold(first, |acc, value| env.add_sub(false, acc, value.clone())).ok()?;
        Some(sum.show(&self.style()))
    }
}

#[cfg(test)]
mod tests {
    use crate::golden::default_context;

    #[test]
    fn a_label_is_not_a_variable() {
        let sheet = super::evaluate("Price: $40 - 15%\nPrice + 34\nprice = $40\nprice + 34", &default_context());
        let error = sheet.lines[1].error.as_deref().unwrap_or_default();
        assert!(error.contains("is a label"), "{error}");
        assert_eq!(sheet.lines[3].text.as_deref(), Some("$74.00"));
    }
}
