//! Splits one line into tokens, keeping byte positions for highlighting.

use crate::num::{self, Num, NumberFormat};
use bigdecimal::num_bigint::BigInt;
use chrono::NaiveDate;

/// Base a number was written in, kept for the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Dec,
    Hex,
    Bin,
    Oct,
    Sci,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tk {
    Num(Num, Base),
    Word(String),
    Sym(char),
    Shl,
    Shr,
    /// Currency symbol, already turned into its ISO code.
    Cur(&'static str),
    Date(NaiveDate),
    Time(u32, u32, u32),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tk: Tk,
    pub start: usize,
    pub end: usize,
    pub space_before: bool,
}

impl Token {
    pub fn word(&self) -> Option<&str> {
        match &self.tk {
            Tk::Word(w) => Some(w),
            _ => None,
        }
    }

    pub fn is_sym(&self, c: char) -> bool {
        self.tk == Tk::Sym(c)
    }
}

struct Cursor<'a> {
    src: &'a str,
    chars: Vec<(usize, char)>,
}

impl Cursor<'_> {
    fn at(&self, k: usize) -> char {
        self.chars.get(k).map_or('\0', |c| c.1)
    }

    fn pos(&self, k: usize) -> usize {
        self.chars.get(k).map_or(self.src.len(), |c| c.0)
    }

    fn digit(&self, k: usize) -> bool {
        self.at(k).is_ascii_digit()
    }

    /// Exactly three digits at `k`, not followed by a fourth.
    fn group_of_three(&self, k: usize) -> bool {
        self.digit(k) && self.digit(k + 1) && self.digit(k + 2) && !self.digit(k + 3)
    }

    fn digits(&self, from: usize, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for k in from..from + count {
            value = value * 10 + self.at(k).to_digit(10)?;
        }
        Some(value)
    }

    /// `YYYY-MM-DD`, without spaces.
    fn date(&self, i: usize) -> Option<(NaiveDate, usize)> {
        if self.at(i + 4) != '-' || self.at(i + 7) != '-' || self.digit(i + 10) {
            return None;
        }
        let year = self.digits(i, 4)?;
        let month = self.digits(i + 5, 2)?;
        let day = self.digits(i + 8, 2)?;
        Some((NaiveDate::from_ymd_opt(year as i32, month, day)?, i + 10))
    }

    /// `H:MM` or `HH:MM`, optionally `:SS`.
    fn time(&self, i: usize) -> Option<(u32, u32, u32, usize)> {
        let hour_digits = if self.digit(i + 1) { 2 } else { 1 };
        let colon = i + hour_digits;
        if self.at(colon) != ':' || !self.digit(colon + 1) || !self.digit(colon + 2) {
            return None;
        }
        let hour = self.digits(i, hour_digits)?;
        let minute = self.digits(colon + 1, 2)?;
        let mut end = colon + 3;
        let mut second = 0;
        if self.at(end) == ':' && self.digit(end + 1) && self.digit(end + 2) {
            second = self.digits(end + 1, 2)?;
            end += 3;
        }
        if self.digit(end) || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        Some((hour, minute, second, end))
    }

    fn number(&self, i: usize, format: NumberFormat) -> Option<(Num, Base, usize)> {
        if self.at(i) == '0' {
            let (radix, base) = match self.at(i + 1) {
                'x' | 'X' => (16, Base::Hex),
                'b' | 'B' => (2, Base::Bin),
                'o' | 'O' => (8, Base::Oct),
                _ => (0, Base::Dec),
            };
            if radix != 0 {
                let mut j = i + 2;
                let mut digits = String::new();
                while self.at(j).is_digit(radix) {
                    digits.push(self.at(j));
                    j += 1;
                }
                if !digits.is_empty() && !self.at(j).is_alphanumeric() {
                    let value = BigInt::parse_bytes(digits.as_bytes(), radix)?;
                    return Some((num::from_bigint(value), base, j));
                }
            }
        }
        let group = format.group_separator();
        let mut j = i;
        let mut int_part = String::new();
        while self.digit(j) {
            int_part.push(self.at(j));
            j += 1;
        }
        if !int_part.is_empty() && int_part.len() <= 3 {
            while (self.at(j) == group || self.at(j) == ' ') && self.group_of_three(j + 1) {
                for k in 1..=3 {
                    int_part.push(self.at(j + k));
                }
                j += 4;
            }
        }
        let mut frac_part = String::new();
        let separator = self.at(j);
        // With the Italian format a dot that is not a thousands separator is
        // still read as a decimal point.
        let is_point = separator == format.decimal_separator()
            || (format == NumberFormat::Italian && separator == '.');
        if is_point && self.digit(j + 1) {
            j += 1;
            while self.digit(j) {
                frac_part.push(self.at(j));
                j += 1;
            }
        }
        if int_part.is_empty() && frac_part.is_empty() {
            return None;
        }
        let mut text = if int_part.is_empty() { "0".to_string() } else { int_part };
        if !frac_part.is_empty() {
            text.push('.');
            text.push_str(&frac_part);
        }
        if matches!(self.at(j), 'e' | 'E') {
            let mut k = j + 1;
            let mut exponent = String::new();
            if matches!(self.at(k), '+' | '-') {
                exponent.push(self.at(k));
                k += 1;
            }
            if self.digit(k) {
                while self.digit(k) {
                    exponent.push(self.at(k));
                    k += 1;
                }
                text.push('e');
                text.push_str(&exponent);
                j = k;
            }
        }
        Some((num::parse(&text)?, Base::Dec, j))
    }
}

fn currency_symbol(c: char) -> Option<&'static str> {
    Some(match c {
        '$' => "USD",
        '€' => "EUR",
        '£' => "GBP",
        '¥' => "JPY",
        '₹' => "INR",
        '₩' => "KRW",
        '₺' => "TRY",
        '₴' => "UAH",
        '₪' => "ILS",
        '฿' => "THB",
        '₽' => "RUB",
        _ => return None,
    })
}

pub fn lex(src: &str, format: NumberFormat) -> Vec<Token> {
    let cur = Cursor { src, chars: src.char_indices().collect() };
    let count = cur.chars.len();
    let mut out: Vec<Token> = Vec::new();
    let mut space = true;
    let mut i = 0;
    while i < count {
        let c = cur.at(i);
        if c.is_whitespace() {
            space = true;
            i += 1;
            continue;
        }
        let start = cur.pos(i);
        let push = |tk: Tk, end_index: usize, out: &mut Vec<Token>| {
            out.push(Token { tk, start, end: cur.pos(end_index), space_before: space });
        };
        let leading_point = (c == format.decimal_separator() || (c == '.' && format == NumberFormat::Italian))
            && cur.digit(i + 1);
        if c.is_ascii_digit() || leading_point {
            if let Some((date, end)) = cur.date(i) {
                push(Tk::Date(date), end, &mut out);
                i = end;
            } else if let Some((h, m, s, end)) = cur.time(i) {
                push(Tk::Time(h, m, s), end, &mut out);
                i = end;
            } else if let Some((value, base, end)) = cur.number(i, format) {
                push(Tk::Num(value, base), end, &mut out);
                i = end;
                // `3x4`: an x between digits is a multiplication.
                if matches!(cur.at(i), 'x' | 'X') && cur.digit(i + 1) {
                    out.push(Token { tk: Tk::Word("x".into()), start: cur.pos(i), end: cur.pos(i + 1), space_before: false });
                    i += 1;
                }
            } else {
                push(Tk::Sym(c), i + 1, &mut out);
                i += 1;
            }
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while cur.at(j).is_alphanumeric() || cur.at(j) == '_' {
                j += 1;
            }
            let word: String = cur.chars[i..j].iter().map(|(_, ch)| ch).collect();
            let prefixed_dollar = match word.as_str() {
                "R" => Some("BRL"),
                "C" => Some("CAD"),
                "A" => Some("AUD"),
                _ => None,
            };
            if let (Some(code), '$') = (prefixed_dollar, cur.at(j)) {
                push(Tk::Cur(code), j + 1, &mut out);
                i = j + 1;
            } else {
                push(Tk::Word(word), j, &mut out);
                i = j;
            }
        } else if let Some(code) = currency_symbol(c) {
            push(Tk::Cur(code), i + 1, &mut out);
            i += 1;
        } else if c == '<' && cur.at(i + 1) == '<' {
            push(Tk::Shl, i + 2, &mut out);
            i += 2;
        } else if c == '>' && cur.at(i + 1) == '>' {
            push(Tk::Shr, i + 2, &mut out);
            i += 2;
        } else {
            let sym = match c {
                '−' | '–' => '-',
                '×' | '·' => '*',
                '÷' => '/',
                other => other,
            };
            push(Tk::Sym(sym), i + 1, &mut out);
            i += 1;
        }
        space = false;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str, format: NumberFormat) -> Vec<Tk> {
        lex(src, format).into_iter().map(|t| t.tk).collect()
    }

    #[test]
    fn numbers_and_groups() {
        let en = NumberFormat::English;
        assert_eq!(kinds("1,000,000 + 1", en)[0], Tk::Num(num::lit("1000000"), Base::Dec));
        assert_eq!(kinds("1 000 000", en), vec![Tk::Num(num::lit("1000000"), Base::Dec)]);
        assert_eq!(kinds("5 30", en).len(), 2);
        assert_eq!(kinds(".5", en), vec![Tk::Num(num::lit("0.5"), Base::Dec)]);
        assert_eq!(kinds("2E-3", en), vec![Tk::Num(num::lit("0.002"), Base::Dec)]);
        assert_eq!(kinds("0xFF", en), vec![Tk::Num(num::lit("255"), Base::Hex)]);
        assert_eq!(kinds("3x4", en).len(), 3);
        assert_eq!(kinds("2em", en)[1], Tk::Word("em".into()));
    }

    #[test]
    fn italian_numbers() {
        let it = NumberFormat::Italian;
        assert_eq!(kinds("1.234,5", it), vec![Tk::Num(num::lit("1234.5"), Base::Dec)]);
        assert_eq!(kinds("1.250", it), vec![Tk::Num(num::lit("1250"), Base::Dec)]);
        assert_eq!(kinds("0.5", it), vec![Tk::Num(num::lit("0.5"), Base::Dec)]);
    }

    #[test]
    fn dates_and_times() {
        let en = NumberFormat::English;
        assert!(matches!(kinds("2026-12-25 - today", en)[0], Tk::Date(_)));
        assert_eq!(kinds("2:30 pm", en)[0], Tk::Time(2, 30, 0));
    }
}
