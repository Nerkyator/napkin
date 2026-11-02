//! Builds an expression from the tokens of one line.
//!
//! Precedence, from the loosest: conversion, `|`, `xor`, `&`, shifts,
//! `+ -`, `* / mod`, percent operators, unary minus, `^`, postfix units.

use crate::currency::{self, Rates};
use crate::lexer::{Base, Tk, Token};
use crate::num::{self, Num};
use crate::units::{self, Dim, UnitSpec};
use crate::zones::{self, Zone};
use chrono::NaiveDate;

/// Highlight classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hl {
    Number,
    Unit,
    Keyword,
    Function,
    Variable,
    Comment,
    Label,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
    Shl,
    Shr,
    And,
    Xor,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PctOp {
    Of,
    On,
    Off,
    OfWhat,
    OnWhat,
    OffWhat,
    AsOf,
    AsOn,
    AsOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    Sqrt,
    Cbrt,
    Root,
    Abs,
    Log,
    Ln,
    Fact,
    Round,
    Ceil,
    Floor,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    FromUnix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kw {
    Today,
    Tomorrow,
    Yesterday,
    Now,
    Time,
    Prev,
    Sum,
    Avg,
    Pi,
    E,
    Ppi,
}

#[derive(Clone, Debug)]
pub enum Target {
    Unit(UnitSpec),
    Currency(String),
    Base(Base),
    Zone(Zone),
}

#[derive(Clone, Debug)]
pub enum Expr {
    Number(Num, Base),
    Var(String),
    Undefined(String),
    Kw(Kw),
    Date(NaiveDate),
    TimeOfDay { h: u32, m: u32, s: u32, zone: Option<Zone> },
    ZoneNow(Zone),
    /// `Dec 25`, `25 December 2026`: without a year, the current one.
    MonthDay { month: u32, day: u32, year: Option<i32> },
    /// A date with a time: `2026-10-14 13:30`, `Dec 25 3pm`.
    DateAt { date: Box<Expr>, h: u32, m: u32, s: u32, zone: Option<Zone> },
    /// `days until Dec 25`, `weeks since Jan 1`: from or to now, in a unit.
    Until { unit: UnitSpec, target: Box<Expr>, since: bool },
    /// `Dec 25 to today`, `between Jan 1 and Mar 1`: the time between two moments.
    Span(Box<Expr>, Box<Expr>),
    Unit(Box<Expr>, UnitSpec),
    Money(Box<Expr>, String),
    Percent(Box<Expr>),
    Neg(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Pct(PctOp, Box<Expr>, Box<Expr>),
    Convert(Box<Expr>, Target),
    Call(Func, Vec<Expr>),
    Factorial(Box<Expr>),
}

const BP_CONVERT: u8 = 10;
const BP_OR: u8 = 20;
const BP_XOR: u8 = 22;
const BP_AND: u8 = 24;
const BP_SHIFT: u8 = 30;
const BP_ADD: u8 = 40;
const BP_MUL: u8 = 50;
const BP_PCT: u8 = 55;
const BP_UNARY: u8 = 60;
const BP_POW: u8 = 70;

pub fn function(lower: &str) -> Option<Func> {
    Some(match lower {
        "sqrt" => Func::Sqrt,
        "cbrt" => Func::Cbrt,
        "root" => Func::Root,
        "abs" => Func::Abs,
        "log" => Func::Log,
        "ln" => Func::Ln,
        "fact" => Func::Fact,
        "round" => Func::Round,
        "ceil" => Func::Ceil,
        "floor" => Func::Floor,
        "sin" => Func::Sin,
        "cos" => Func::Cos,
        "tan" => Func::Tan,
        "arcsin" | "asin" => Func::Asin,
        "arccos" | "acos" => Func::Acos,
        "arctan" | "atan" => Func::Atan,
        "sinh" => Func::Sinh,
        "cosh" => Func::Cosh,
        "tanh" => Func::Tanh,
        "fromunix" => Func::FromUnix,
        _ => return None,
    })
}

/// English month names and their abbreviations.
pub fn month_number(word: &str) -> Option<u32> {
    Some(match word.to_lowercase().as_str() {
        "jan" | "january" => 1,
        "feb" | "february" => 2,
        "mar" | "march" => 3,
        "apr" | "april" => 4,
        "may" => 5,
        "jun" | "june" => 6,
        "jul" | "july" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" => 10,
        "nov" | "november" => 11,
        "dec" | "december" => 12,
        _ => return None,
    })
}

fn is_meridiem(word: &str) -> bool {
    matches!(word.to_lowercase().as_str(), "am" | "pm")
}

pub fn keyword(word: &str) -> Option<Kw> {
    if word == "e" {
        return Some(Kw::E);
    }
    if word == "π" {
        return Some(Kw::Pi);
    }
    Some(match word.to_lowercase().as_str() {
        "today" => Kw::Today,
        "tomorrow" => Kw::Tomorrow,
        "yesterday" => Kw::Yesterday,
        "now" => Kw::Now,
        "time" => Kw::Time,
        "prev" => Kw::Prev,
        "sum" | "total" => Kw::Sum,
        "avg" | "average" => Kw::Avg,
        "pi" => Kw::Pi,
        "ppi" => Kw::Ppi,
        _ => return None,
    })
}

fn base_word(lower: &str) -> Option<Base> {
    Some(match lower {
        "hex" | "hexadecimal" => Base::Hex,
        "binary" | "bin" => Base::Bin,
        "octal" | "oct" => Base::Oct,
        "sci" | "scientific" => Base::Sci,
        "decimal" | "dec" => Base::Dec,
        _ => return None,
    })
}

fn scale_word(lower: &str) -> Option<i64> {
    Some(match lower {
        "thousand" => 3,
        "million" => 6,
        "billion" => 9,
        "trillion" => 12,
        _ => return None,
    })
}

/// Words with a meaning of their own, which may not name a variable.
pub fn is_reserved_word(lower: &str) -> bool {
    const WORDS: &[&str] = &[
        "in", "into", "as", "to", "of", "on", "off", "what", "is", "percent", "plus", "and", "with",
        "minus", "subtract", "without", "times", "mul", "multiplied", "divide", "divided", "by",
        "mod", "xor", "am", "pm", "until", "till", "since", "between",
    ];
    WORDS.contains(&lower)
        || function(lower).is_some()
        || keyword(lower).is_some()
        || base_word(lower).is_some()
        || scale_word(lower).is_some()
}

pub struct Parsed {
    /// `None` when the line holds no value.
    pub expr: Option<Expr>,
    /// `on Jan 15`: the day whose exchange rates the line uses.
    pub rates_date: Option<Expr>,
    pub hl: Vec<Option<Hl>>,
    /// The line has values but cannot be read, e.g. `5 30`.
    pub failed: bool,
}

pub struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    hl: Vec<Option<Hl>>,
    is_var: &'a dyn Fn(&str) -> bool,
    rates: Option<&'a Rates>,
    rates_date: Option<Expr>,
    failed: bool,
}

impl<'a> Parser<'a> {
    pub fn new(toks: &'a [Token], is_var: &'a dyn Fn(&str) -> bool, rates: Option<&'a Rates>) -> Self {
        Parser { toks, pos: 0, hl: vec![None; toks.len()], is_var, rates, rates_date: None, failed: false }
    }

    pub fn parse(mut self) -> Parsed {
        let mut expr = self.parse_expr(0, false);
        while !self.failed && self.pos < self.toks.len() {
            // A stray `)` or `;` at the top level is ignored.
            self.pos += 1;
            expr = match expr {
                Some(lhs) => self.parse_infix(lhs, 0),
                None => self.parse_expr(0, false),
            };
        }
        Parsed { expr, rates_date: self.rates_date, hl: self.hl, failed: self.failed }
    }

    fn fail(&mut self) {
        self.failed = true;
    }

    fn tok(&self, k: usize) -> Option<&'a Token> {
        self.toks.get(k)
    }

    fn word_at(&self, k: usize) -> Option<&'a str> {
        self.tok(k)?.word()
    }

    fn word_is(&self, k: usize, expected: &str) -> bool {
        self.word_at(k).is_some_and(|w| w.eq_ignore_ascii_case(expected))
    }

    fn sym_at(&self, k: usize, c: char) -> bool {
        self.tok(k).is_some_and(|t| t.is_sym(c))
    }

    fn mark(&mut self, from: usize, count: usize, hl: Hl) {
        for k in from..(from + count).min(self.hl.len()) {
            self.hl[k] = Some(hl);
        }
    }

    fn is_number_at(&self, k: usize) -> bool {
        matches!(self.tok(k).map(|t| &t.tk), Some(Tk::Num(..)))
    }

    /// Consecutive words joined by single spaces, lower case.
    fn words_joined(&self, k: usize, count: usize) -> Option<String> {
        let mut out = String::new();
        for j in k..k + count {
            let tok = self.tok(j)?;
            let word = tok.word()?;
            if j > k {
                if !tok.space_before {
                    return None;
                }
                out.push(' ');
            }
            out.push_str(word);
        }
        Some(out.to_lowercase())
    }

    /// `+5:30` or `-3` right after `UTC`/`GMT`, in minutes.
    fn offset_after(&self, k: usize) -> Option<(i32, usize)> {
        let sign_tok = self.tok(k)?;
        if sign_tok.space_before {
            return None;
        }
        let sign = if sign_tok.is_sym('+') {
            1
        } else if sign_tok.is_sym('-') {
            -1
        } else {
            return None;
        };
        let value = self.tok(k + 1)?;
        if value.space_before {
            return None;
        }
        match &value.tk {
            Tk::Num(n, Base::Dec) => {
                let hours = num::to_i64(n)?;
                (hours <= 14).then_some((sign * hours as i32 * 60, 2))
            }
            Tk::Time(h, m, 0) => Some((sign * (*h as i32 * 60 + *m as i32), 2)),
            _ => None,
        }
    }

    /// A time zone written at `k`: abbreviation, offset, IANA name or city.
    fn zone_at(&self, k: usize) -> Option<(Zone, usize)> {
        let word = self.word_at(k)?;
        if is_reserved_word(&word.to_lowercase()) {
            return None;
        }
        if zones::is_offset_base(word) {
            if let Some((minutes, used)) = self.offset_after(k + 1) {
                return Some((zones::offset(minutes)?, 1 + used));
            }
        }
        if let Some(zone) = zones::abbreviation(word) {
            return Some((zone, 1));
        }
        if self.tok(k + 1).is_some_and(|t| t.is_sym('/') && !t.space_before) {
            let mut name = word.to_string();
            let mut j = k + 1;
            while self.tok(j).is_some_and(|t| t.is_sym('/') && !t.space_before)
                && self.tok(j + 1).is_some_and(|t| !t.space_before && t.word().is_some())
            {
                name.push('/');
                name.push_str(self.word_at(j + 1)?);
                j += 2;
            }
            if let Some(zone) = zones::iana(&name) {
                return Some((zone, j - k));
            }
        }
        for count in (1..=4).rev() {
            if let Some(name) = self.words_joined(k, count) {
                if let Some(zone) = zones::city(&name) {
                    return Some((zone, count));
                }
            }
        }
        None
    }

    fn unit_at(&self, k: usize) -> Option<(UnitSpec, usize)> {
        let tok = self.tok(k)?;
        if tok.is_sym('°') {
            if let Some(next) = self.tok(k + 1) {
                if let (false, Some(letter @ ("C" | "F"))) = (next.space_before, next.word()) {
                    return units::lookup(&format!("°{letter}")).map(|u| (u, 2));
                }
            }
            return units::lookup("°").map(|u| (u, 1));
        }
        if let Some(compound) = self.compound_at(k) {
            return Some(compound);
        }
        let first = tok.word()?;
        let mut words = vec![first];
        if let Some(second) = self.tok(k + 1).filter(|t| t.space_before).and_then(Token::word) {
            words.push(second);
        }
        units::lookup_phrase(&words)
    }

    /// `km/h`, `MB/s`, `L/100km`: a rate written without spaces.
    fn compound_at(&self, k: usize) -> Option<(UnitSpec, usize)> {
        let numerator = self.word_at(k)?;
        let slash = self.tok(k + 1)?;
        let next = self.tok(k + 2)?;
        if !slash.is_sym('/') || slash.space_before || next.space_before {
            return None;
        }
        let (scale, denominator, used) = match &next.tk {
            Tk::Word(word) => (None, word.as_str(), 3),
            Tk::Num(n, Base::Dec) => (Some(n), self.tok(k + 3).filter(|t| !t.space_before)?.word()?, 4),
            _ => return None,
        };
        units::lookup_compound(numerator, scale, denominator).map(|unit| (UnitSpec::Fixed(unit), used))
    }

    /// Whether a date starts at `k`: `2026-01-15`, `Jan 15`, `15 Jan`, `yesterday`.
    fn date_follows(&self, k: usize) -> bool {
        match self.tok(k).map(|t| &t.tk) {
            Some(Tk::Date(_)) => true,
            Some(Tk::Word(word)) => {
                (month_number(word).is_some() && self.day_at(k + 1).is_some())
                    || matches!(keyword(word), Some(Kw::Today | Kw::Yesterday | Kw::Tomorrow))
            }
            Some(Tk::Num(..)) => {
                self.day_at(k).is_some()
                    && (1..=3).any(|n| self.word_at(k + n).is_some_and(|w| month_number(w).is_some()))
            }
            _ => false,
        }
    }

    fn currency_at(&self, k: usize) -> Option<(String, usize)> {
        let tok = self.tok(k)?;
        if let Tk::Cur(code) = tok.tk {
            return Some((code.to_string(), 1));
        }
        let first = tok.word()?;
        if let Some(second) = self.word_at(k + 1) {
            if let Some(code) = currency::phrase(first, second) {
                return Some((code.to_string(), 2));
            }
        }
        currency::word_code(first, self.rates).map(|code| (code, 1))
    }

    /// What follows a conversion word, if it is a valid target.
    fn target_at(&self, k: usize) -> Option<(Target, usize)> {
        if let Some(word) = self.word_at(k) {
            let lower = word.to_lowercase();
            // `Dec 25` is a date, not `dec` for decimal.
            if lower == "in" || (month_number(word).is_some() && self.day_at(k + 1).is_some()) {
                return None;
            }
            if let Some(base) = base_word(&lower) {
                return Some((Target::Base(base), 1));
            }
        }
        if let Some((code, used)) = self.currency_at(k) {
            return Some((Target::Currency(code), used));
        }
        if let Some((unit, used)) = self.unit_at(k) {
            return Some((Target::Unit(unit), used));
        }
        if let Some((zone, used)) = self.zone_at(k) {
            return Some((Target::Zone(zone), used));
        }
        None
    }

    fn is_conversion_word(&self, k: usize) -> bool {
        self.word_at(k)
            .is_some_and(|w| matches!(w.to_lowercase().as_str(), "in" | "into" | "as" | "to"))
    }

    fn conversion_follows(&self, k: usize) -> bool {
        self.is_conversion_word(k) && self.target_at(k + 1).is_some()
    }

    /// `as a % of`, `as % on`…: the operator and the tokens it uses.
    fn as_percent_at(&self, k: usize) -> Option<(PctOp, usize)> {
        if !self.word_is(k, "as") {
            return None;
        }
        let mut j = k + 1;
        if self.word_is(j, "a") {
            j += 1;
        }
        if !(self.sym_at(j, '%') || self.word_is(j, "percent")) {
            return None;
        }
        j += 1;
        let op = match self.word_at(j)?.to_lowercase().as_str() {
            "of" => PctOp::AsOf,
            "on" => PctOp::AsOn,
            "off" => PctOp::AsOff,
            _ => return None,
        };
        Some((op, j + 1 - k))
    }

    /// Binary operator at `k`: operator, binding power, right associative, tokens.
    fn binary_op_at(&self, k: usize, lhs: Option<&Expr>) -> Option<(BinOp, u8, bool, usize)> {
        let tok = self.tok(k)?;
        let op = match &tok.tk {
            Tk::Sym('+') => (BinOp::Add, BP_ADD, false, 1),
            Tk::Sym('-') => (BinOp::Sub, BP_ADD, false, 1),
            Tk::Sym('*') => (BinOp::Mul, BP_MUL, false, 1),
            Tk::Sym('/') => (BinOp::Div, BP_MUL, false, 1),
            Tk::Sym('^') => (BinOp::Pow, BP_POW, true, 1),
            Tk::Sym('&') => (BinOp::And, BP_AND, false, 1),
            Tk::Sym('|') => (BinOp::Or, BP_OR, false, 1),
            Tk::Shl => (BinOp::Shl, BP_SHIFT, false, 1),
            Tk::Shr => (BinOp::Shr, BP_SHIFT, false, 1),
            Tk::Word(word) => match word.to_lowercase().as_str() {
                "plus" | "and" | "with" => (BinOp::Add, BP_ADD, false, 1),
                "minus" | "subtract" | "without" => (BinOp::Sub, BP_ADD, false, 1),
                "times" | "mul" => (BinOp::Mul, BP_MUL, false, 1),
                "multiplied" if self.word_is(k + 1, "by") => (BinOp::Mul, BP_MUL, false, 2),
                "divide" | "divided" if self.word_is(k + 1, "by") => (BinOp::Div, BP_MUL, false, 2),
                "divide" => (BinOp::Div, BP_MUL, false, 1),
                "mod" => (BinOp::Mod, BP_MUL, false, 1),
                "xor" => (BinOp::Xor, BP_XOR, false, 1),
                // `x` multiplies only between two numbers.
                "x" if lhs.is_some_and(is_numeric_literal) && self.is_number_at(k + 1) => {
                    (BinOp::Mul, BP_MUL, false, 1)
                }
                _ => return None,
            },
            _ => return None,
        };
        Some(op)
    }

    fn parse_expr(&mut self, min_bp: u8, after_op: bool) -> Option<Expr> {
        let lhs = self.parse_prefix(after_op)?;
        self.parse_infix(lhs, min_bp)
    }

    fn parse_infix(&mut self, mut lhs: Expr, min_bp: u8) -> Option<Expr> {
        while !self.failed {
            let k = self.pos;
            let Some(tok) = self.tok(k) else { break };
            if tok.is_sym(')') || tok.is_sym(';') {
                break;
            }
            if let Some((op, used)) = self.as_percent_at(k) {
                if BP_CONVERT < min_bp {
                    break;
                }
                self.mark(k, used, Hl::Keyword);
                self.pos += used;
                let rhs = self.parse_expr(BP_CONVERT + 1, true)?;
                lhs = Expr::Pct(op, Box::new(lhs), Box::new(rhs));
                continue;
            }
            if self.is_conversion_word(k) {
                if let Some((target, used)) = self.target_at(k + 1) {
                    if BP_CONVERT < min_bp {
                        break;
                    }
                    self.mark(k, 1, Hl::Keyword);
                    self.mark(k + 1, used, Hl::Unit);
                    self.pos += 1 + used;
                    lhs = Expr::Convert(Box::new(lhs), target);
                    continue;
                }
            }
            // `100 USD in EUR on Jan 15`: the rates of that day, for the whole line.
            if self.word_is(k, "on") && !matches!(lhs, Expr::Percent(_)) && self.date_follows(k + 1) {
                self.mark(k, 1, Hl::Keyword);
                self.pos += 1;
                let date = self.parse_expr(BP_ADD + 1, true)?;
                if self.rates_date.is_none() {
                    self.rates_date = Some(date);
                }
                continue;
            }
            // `Dec 25 to today`, `9am to 5pm`: the time between two moments.
            if let Some(word) = tok.word() {
                let relation = matches!(word.to_lowercase().as_str(), "to" | "until" | "till");
                if relation && is_date_like(&lhs) && self.starts_date(k + 1) {
                    if BP_ADD < min_bp {
                        break;
                    }
                    self.mark(k, 1, Hl::Keyword);
                    self.pos += 1;
                    let rhs = self.parse_expr(BP_ADD + 1, true)?;
                    lhs = Expr::Span(Box::new(lhs), Box::new(rhs));
                    continue;
                }
            }
            if let Some(word) = tok.word() {
                let lower = word.to_lowercase();
                if matches!(lower.as_str(), "of" | "on" | "off") {
                    if BP_PCT < min_bp {
                        break;
                    }
                    let what = self.word_is(k + 1, "what") && self.word_is(k + 2, "is");
                    let used = if what { 3 } else { 1 };
                    self.mark(k, used, Hl::Keyword);
                    self.pos += used;
                    let op = match (lower.as_str(), what) {
                        ("of", false) => PctOp::Of,
                        ("on", false) => PctOp::On,
                        ("off", false) => PctOp::Off,
                        ("of", true) => PctOp::OfWhat,
                        ("on", true) => PctOp::OnWhat,
                        _ => PctOp::OffWhat,
                    };
                    let rhs = self.parse_expr(BP_PCT + 1, true)?;
                    lhs = Expr::Pct(op, Box::new(lhs), Box::new(rhs));
                    continue;
                }
            }
            if let Some((op, bp, right, used)) = self.binary_op_at(k, Some(&lhs)) {
                if bp < min_bp {
                    break;
                }
                self.mark(k, used, Hl::Keyword);
                self.pos += used;
                let rhs = self.parse_expr(if right { bp } else { bp + 1 }, true)?;
                lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
                continue;
            }
            // Implicit multiplication: `4 (7)`, `(1 + 2)(3 + 4)`.
            if tok.is_sym('(') {
                if BP_MUL < min_bp {
                    break;
                }
                let rhs = self.parse_prefix(true)?;
                lhs = Expr::Bin(BinOp::Mul, Box::new(lhs), Box::new(rhs));
                continue;
            }
            // A number followed by a constant: `2 pi`.
            if let (Expr::Number(..), Some(word)) = (&lhs, tok.word()) {
                if matches!(keyword(word), Some(Kw::Pi | Kw::E)) {
                    if BP_MUL < min_bp {
                        break;
                    }
                    let rhs = self.parse_prefix(true)?;
                    lhs = Expr::Bin(BinOp::Mul, Box::new(lhs), Box::new(rhs));
                    continue;
                }
            }
            if matches!(tok.tk, Tk::Num(..) | Tk::Cur(_) | Tk::Date(_) | Tk::Time(..)) {
                // Values written side by side add up when they share a unit:
                // `2 meter 35 cm`, `1 h 30 min`. Otherwise the line has no result.
                if let (Expr::Unit(..), Tk::Num(..)) = (&lhs, &tok.tk) {
                    if BP_ADD < min_bp {
                        break;
                    }
                    let rhs = self.parse_prefix(true)?;
                    if matches!(rhs, Expr::Unit(..)) {
                        lhs = Expr::Bin(BinOp::Add, Box::new(lhs), Box::new(rhs));
                        continue;
                    }
                }
                self.fail();
                return None;
            }
            // Unknown words after a value are ignored: `5 apples + 3 oranges`.
            self.pos += 1;
        }
        Some(lhs)
    }

    fn parse_prefix(&mut self, after_op: bool) -> Option<Expr> {
        loop {
            if self.failed {
                return None;
            }
            let k = self.pos;
            let tok = self.tok(k)?;
            match &tok.tk {
                Tk::Num(..) => {
                    if let Some(date) = self.day_then_month() {
                        return self.date_time_postfix(date);
                    }
                    let value = self.scaled_number()?;
                    return self.postfix(value, true);
                }
                Tk::Cur(code) => {
                    let code = code.to_string();
                    return self.money_prefix(code, 1);
                }
                Tk::Date(date) => {
                    self.mark(k, 1, Hl::Number);
                    self.pos += 1;
                    return self.date_time_postfix(Expr::Date(*date));
                }
                Tk::Time(h, m, s) => {
                    let (h, m, s) = (*h, *m, *s);
                    self.mark(k, 1, Hl::Number);
                    self.pos += 1;
                    return self.time_postfix(h, m, s);
                }
                Tk::Sym('(') => {
                    self.mark(k, 1, Hl::Keyword);
                    self.pos += 1;
                    let inner = self.parse_expr(0, true);
                    let Some(inner) = inner else {
                        self.fail();
                        return None;
                    };
                    if self.sym_at(self.pos, ')') {
                        self.mark(self.pos, 1, Hl::Keyword);
                        self.pos += 1;
                    }
                    return self.postfix(inner, false);
                }
                Tk::Sym(sign @ ('-' | '+')) if after_op || self.starts_value(k + 1) => {
                    let negative = *sign == '-';
                    self.mark(k, 1, Hl::Keyword);
                    self.pos += 1;
                    let value = self.parse_expr(BP_UNARY, true)?;
                    return Some(if negative { Expr::Neg(Box::new(value)) } else { value });
                }
                Tk::Word(word) => {
                    if let Some(expr) = self.word_prefix(word, after_op) {
                        return Some(expr);
                    }
                    if self.failed {
                        return None;
                    }
                }
                _ => {
                    if after_op {
                        self.fail();
                        return None;
                    }
                    self.pos += 1;
                }
            }
        }
    }

    /// Whether the token at `k` can start a value.
    fn starts_value(&self, k: usize) -> bool {
        match self.tok(k).map(|t| &t.tk) {
            Some(Tk::Num(..) | Tk::Cur(_) | Tk::Date(_) | Tk::Time(..) | Tk::Sym('(')) => true,
            Some(Tk::Word(word)) => {
                (self.is_var)(word) || keyword(word).is_some() || function(&word.to_lowercase()).is_some()
            }
            _ => false,
        }
    }

    /// A word in operand position. `None` with `failed` unset: the word was
    /// skipped as text and the caller keeps looking for a value.
    fn word_prefix(&mut self, word: &str, after_op: bool) -> Option<Expr> {
        let k = self.pos;
        let lower = word.to_lowercase();
        if (self.is_var)(word) {
            self.mark(k, 1, Hl::Variable);
            self.pos += 1;
            return Some(Expr::Var(word.to_string()));
        }
        if let Some(month) = month_number(word) {
            if let Some(date) = self.month_then_day(month) {
                return self.date_time_postfix(date);
            }
        }
        // `Tokyo time`, `CET time`; cities load only when `time` is near.
        if (1..=6).any(|n| self.word_is(k + n, "time")) {
            if let Some((zone, used)) = self.zone_at(k) {
                if self.word_is(k + used, "time") {
                    self.mark(k, used, Hl::Unit);
                    self.mark(k + used, 1, Hl::Keyword);
                    self.pos += used + 1;
                    return Some(Expr::ZoneNow(zone));
                }
            }
        }
        if let Some(kw) = keyword(word) {
            self.mark(k, 1, Hl::Keyword);
            self.pos += 1;
            return Some(Expr::Kw(kw));
        }
        if let Some(func) = function(&lower) {
            self.mark(k, 1, Hl::Function);
            self.pos += 1;
            return self.parse_call(func);
        }
        if lower == "between" && self.starts_date(k + 1) {
            self.mark(k, 1, Hl::Keyword);
            self.pos += 1;
            return self.between();
        }
        // `days until Dec 25`, `weeks since Jan 1`, `days between Jan 1 and Mar 1`.
        if let Some((unit, used)) = self.unit_at(k).filter(|(unit, _)| unit.dim() == Dim::TIME) {
            let relation = self.word_at(k + used).map(str::to_lowercase);
            let relation = relation.as_deref();
            if matches!(relation, Some("to" | "until" | "till" | "since" | "between")) && self.starts_date(k + used + 1) {
                self.mark(k, used, Hl::Unit);
                self.mark(k + used, 1, Hl::Keyword);
                self.pos += used + 1;
                if relation == Some("between") {
                    let span = self.between()?;
                    return Some(Expr::Convert(Box::new(span), Target::Unit(unit)));
                }
                let target = self.parse_expr(BP_ADD + 1, true)?;
                return Some(Expr::Until { unit, target: Box::new(target), since: relation == Some("since") });
            }
        }
        if let Some((code, used)) = self.currency_at(k) {
            if self.is_number_at(k + used) {
                return self.money_prefix(code, used);
            }
            if self.conversion_follows(k + used) {
                self.mark(k, used, Hl::Unit);
                self.pos += used;
                return Some(Expr::Money(Box::new(Expr::Number(num::one(), Base::Dec)), code));
            }
        }
        if lower != "in" {
            if let Some((unit, used)) = self.unit_at(k) {
                if self.conversion_follows(k + used) || matches!(unit, UnitSpec::Em) {
                    self.mark(k, used, Hl::Unit);
                    self.pos += used;
                    return Some(Expr::Unit(Box::new(Expr::Number(num::one(), Base::Dec)), unit));
                }
            }
        }
        // An unknown word where an operand belongs is an undefined variable.
        if after_op || self.binary_op_at(k + 1, None).is_some() {
            self.mark(k, 1, Hl::Variable);
            self.pos += 1;
            return Some(Expr::Undefined(word.to_string()));
        }
        self.pos += 1;
        None
    }

    fn parse_call(&mut self, func: Func) -> Option<Expr> {
        if self.sym_at(self.pos, '(') {
            self.mark(self.pos, 1, Hl::Keyword);
            self.pos += 1;
            let mut args = vec![self.parse_expr(0, true)?];
            while self.sym_at(self.pos, ';') {
                self.mark(self.pos, 1, Hl::Keyword);
                self.pos += 1;
                args.push(self.parse_expr(0, true)?);
            }
            if self.sym_at(self.pos, ')') {
                self.mark(self.pos, 1, Hl::Keyword);
                self.pos += 1;
            }
            return Some(Expr::Call(func, args));
        }
        let mut args = vec![self.parse_expr(BP_UNARY, true)?];
        // `root 3 (27)`, `log 3 (81)`
        if matches!(func, Func::Root | Func::Log) && self.sym_at(self.pos, '(') {
            self.mark(self.pos, 1, Hl::Keyword);
            self.pos += 1;
            args.push(self.parse_expr(0, true)?);
            if self.sym_at(self.pos, ')') {
                self.mark(self.pos, 1, Hl::Keyword);
                self.pos += 1;
            }
        }
        Some(Expr::Call(func, args))
    }

    /// A number with its scale: `2k`, `2M`, `5 million`.
    fn scaled_number(&mut self) -> Option<Expr> {
        let k = self.pos;
        let Some(Tk::Num(value, base)) = self.tok(k).map(|t| t.tk.clone()) else {
            return None;
        };
        self.mark(k, 1, Hl::Number);
        self.pos += 1;
        let mut value = value;
        let next = self.tok(self.pos);
        let attached = next.filter(|t| !t.space_before).and_then(Token::word);
        let power = match attached {
            Some("k") => Some(3),
            Some("M") => Some(6),
            _ => next.and_then(Token::word).and_then(|w| scale_word(&w.to_lowercase())),
        };
        if let Some(power) = power {
            value = num::mul(&value, &num::pow(&num::int(10), &num::int(power))?);
            self.mark(self.pos, 1, Hl::Number);
            self.pos += 1;
        }
        Some(Expr::Number(value, base))
    }

    /// `$10`, `USD 100`, `$25 CAD` (the code after the number wins).
    fn money_prefix(&mut self, code: String, used: usize) -> Option<Expr> {
        let k = self.pos;
        self.mark(k, used, Hl::Unit);
        self.pos += used;
        if !self.is_number_at(self.pos) {
            if self.conversion_follows(self.pos) {
                return Some(Expr::Money(Box::new(Expr::Number(num::one(), Base::Dec)), code));
            }
            return None;
        }
        let amount = self.scaled_number()?;
        let mut code = code;
        if let Some((after, after_used)) = self.currency_at(self.pos) {
            if self.word_at(self.pos).is_some() {
                self.mark(self.pos, after_used, Hl::Unit);
                self.pos += after_used;
                code = after;
            }
        }
        let mut expr = Expr::Money(Box::new(amount), code);
        if self.sym_at(self.pos, '%') {
            self.mark(self.pos, 1, Hl::Keyword);
            self.pos += 1;
            expr = Expr::Percent(Box::new(expr));
        }
        Some(expr)
    }

    /// Units, currencies, percent and time markers after a value.
    fn postfix(&mut self, expr: Expr, literal: bool) -> Option<Expr> {
        let k = self.pos;
        let Some(tok) = self.tok(k) else { return Some(expr) };
        match &tok.tk {
            Tk::Sym('%') => {
                self.mark(k, 1, Hl::Keyword);
                self.pos += 1;
                Some(Expr::Percent(Box::new(expr)))
            }
            Tk::Sym('!') if literal && !tok.space_before => {
                self.mark(k, 1, Hl::Keyword);
                self.pos += 1;
                Some(Expr::Factorial(Box::new(expr)))
            }
            Tk::Cur(code) => {
                let code = code.to_string();
                self.mark(k, 1, Hl::Unit);
                self.pos += 1;
                Some(Expr::Money(Box::new(expr), code))
            }
            Tk::Sym('°') => match self.unit_at(k) {
                Some((unit, used)) => {
                    self.mark(k, used, Hl::Unit);
                    self.pos += used;
                    Some(Expr::Unit(Box::new(expr), unit))
                }
                None => Some(expr),
            },
            Tk::Word(word) => {
                let lower = word.to_lowercase();
                if lower == "percent" {
                    self.mark(k, 1, Hl::Keyword);
                    self.pos += 1;
                    return Some(Expr::Percent(Box::new(expr)));
                }
                if literal && (lower == "am" || lower == "pm") {
                    if let Expr::Number(value, Base::Dec) = &expr {
                        if let Some(hour) = num::to_i64(value).filter(|h| (1..=12).contains(h)) {
                            self.mark(k, 1, Hl::Number);
                            self.pos += 1;
                            let hour = (hour % 12) as u32 + if lower == "pm" { 12 } else { 0 };
                            return self.time_zone_suffix(hour, 0, 0);
                        }
                    }
                }
                if let Some((code, used)) = self.currency_at(k) {
                    self.mark(k, used, Hl::Unit);
                    self.pos += used;
                    return Some(Expr::Money(Box::new(expr), code));
                }
                // `in` is a conversion when a target follows; otherwise inches.
                if lower == "in" && self.target_at(k + 1).is_some() {
                    return Some(expr);
                }
                if let Some((unit, used)) = self.unit_at(k) {
                    self.mark(k, used, Hl::Unit);
                    self.pos += used;
                    return Some(Expr::Unit(Box::new(expr), unit));
                }
                Some(expr)
            }
            _ => Some(expr),
        }
    }

    /// `2:30`, `2:30 pm`, `15:00 HKT`.
    fn time_postfix(&mut self, h: u32, m: u32, s: u32) -> Option<Expr> {
        let mut hour = h;
        if let Some(marker) = self.word_at(self.pos).map(str::to_lowercase) {
            if marker == "am" || marker == "pm" {
                if !(1..=12).contains(&h) {
                    self.fail();
                    return None;
                }
                hour = h % 12 + if marker == "pm" { 12 } else { 0 };
                self.mark(self.pos, 1, Hl::Number);
                self.pos += 1;
            }
        }
        self.time_zone_suffix(hour, m, s)
    }

    /// Whether a date or a time can start at `k`.
    fn starts_date(&self, k: usize) -> bool {
        self.starts_value(k) || self.word_at(k).is_some_and(|w| month_number(w).is_some())
    }

    /// `X and Y`, after `between`.
    fn between(&mut self) -> Option<Expr> {
        let from = self.parse_expr(BP_ADD + 1, true)?;
        if !self.word_is(self.pos, "and") {
            self.fail();
            return None;
        }
        self.mark(self.pos, 1, Hl::Keyword);
        self.pos += 1;
        let to = self.parse_expr(BP_ADD + 1, true)?;
        Some(Expr::Span(Box::new(from), Box::new(to)))
    }

    /// Day of the month at `k`: a whole number from 1 to 31.
    fn day_at(&self, k: usize) -> Option<u32> {
        match &self.tok(k)?.tk {
            Tk::Num(n, Base::Dec) => num::to_i64(n).filter(|d| (1..=31).contains(d)).map(|d| d as u32),
            _ => None,
        }
    }

    /// `st`, `nd`, `rd` or `th` right after a day: `25th`.
    fn ordinal_at(&self, k: usize) -> bool {
        self.tok(k).is_some_and(|t| {
            !t.space_before && t.word().is_some_and(|w| matches!(w.to_lowercase().as_str(), "st" | "nd" | "rd" | "th"))
        })
    }

    /// A four-digit year that may end a date, after an optional comma.
    fn year_after(&self, k: usize) -> (Option<i32>, usize) {
        let year_at = |j: usize| {
            let tok = self.tok(j)?;
            match &tok.tk {
                Tk::Num(n, Base::Dec) if tok.space_before => {
                    num::to_i64(n).filter(|y| (1000..=9999).contains(y)).map(|y| y as i32)
                }
                _ => None,
            }
        };
        if let Some(year) = year_at(k) {
            return (Some(year), 1);
        }
        if self.sym_at(k, ',') {
            if let Some(year) = year_at(k + 1) {
                return (Some(year), 2);
            }
        }
        (None, 0)
    }

    /// `Dec 25`, `December 25th, 2026`.
    fn month_then_day(&mut self, month: u32) -> Option<Expr> {
        let k = self.pos;
        let day = self.day_at(k + 1)?;
        let mut used = 2;
        if self.ordinal_at(k + used) {
            used += 1;
        }
        let (year, year_used) = self.year_after(k + used);
        used += year_used;
        self.mark(k, used, Hl::Number);
        self.pos += used;
        Some(Expr::MonthDay { month, day, year })
    }

    /// `25 Dec`, `25th of December 2026`.
    fn day_then_month(&mut self) -> Option<Expr> {
        let k = self.pos;
        let day = self.day_at(k)?;
        let mut j = k + 1;
        if self.ordinal_at(j) {
            j += 1;
        }
        if self.word_is(j, "of") && self.word_at(j + 1).is_some_and(|w| month_number(w).is_some()) {
            j += 1;
        }
        let month = month_number(self.word_at(j)?)?;
        let (year, year_used) = self.year_after(j + 1);
        let end = j + 1 + year_used;
        self.mark(k, end - k, Hl::Number);
        self.pos = end;
        Some(Expr::MonthDay { month, day, year })
    }

    /// A time right after a date: `2026-10-14 13:30`, `Dec 25 3pm`, `Dec 3 at 3pm`.
    fn date_time_postfix(&mut self, date: Expr) -> Option<Expr> {
        let starts_time = |k: usize| match self.tok(k).map(|t| &t.tk) {
            Some(Tk::Time(..)) => true,
            Some(Tk::Num(..)) => self.word_at(k + 1).is_some_and(is_meridiem),
            _ => false,
        };
        if self.word_is(self.pos, "at") && starts_time(self.pos + 1) {
            self.mark(self.pos, 1, Hl::Keyword);
            self.pos += 1;
        }
        let k = self.pos;
        let time = match self.tok(k).map(|t| &t.tk) {
            Some(Tk::Time(h, m, s)) => {
                let (h, m, s) = (*h, *m, *s);
                self.mark(k, 1, Hl::Number);
                self.pos += 1;
                self.time_postfix(h, m, s)?
            }
            Some(Tk::Num(n, Base::Dec)) if self.word_at(k + 1).is_some_and(is_meridiem) => {
                let Some(hour) = num::to_i64(n).filter(|h| (1..=12).contains(h)) else {
                    return Some(date);
                };
                self.mark(k, 1, Hl::Number);
                self.pos += 1;
                self.time_postfix(hour as u32, 0, 0)?
            }
            _ => return Some(date),
        };
        match time {
            Expr::TimeOfDay { h, m, s, zone } => Some(Expr::DateAt { date: Box::new(date), h, m, s, zone }),
            other => Some(other),
        }
    }

    fn time_zone_suffix(&mut self, h: u32, m: u32, s: u32) -> Option<Expr> {
        let mut zone = None;
        if let Some((found, used)) = self.zone_at(self.pos) {
            self.mark(self.pos, used, Hl::Unit);
            self.pos += used;
            zone = Some(found);
        }
        Some(Expr::TimeOfDay { h, m, s, zone })
    }
}

/// An expression that reads as a date or a time, before `to`.
fn is_date_like(expr: &Expr) -> bool {
    match expr {
        Expr::Date(_)
        | Expr::MonthDay { .. }
        | Expr::DateAt { .. }
        | Expr::TimeOfDay { .. }
        | Expr::ZoneNow(_)
        | Expr::Var(_)
        | Expr::Kw(Kw::Today | Kw::Tomorrow | Kw::Yesterday | Kw::Now | Kw::Time) => true,
        Expr::Bin(BinOp::Add | BinOp::Sub, lhs, _) | Expr::Convert(lhs, Target::Zone(_)) => is_date_like(lhs),
        _ => false,
    }
}

fn is_numeric_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Number(..) => true,
        Expr::Unit(inner, _) | Expr::Money(inner, _) | Expr::Percent(inner) => is_numeric_literal(inner),
        Expr::Bin(_, _, rhs) => is_numeric_literal(rhs),
        _ => false,
    }
}
