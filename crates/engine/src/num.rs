//! Decimal numbers and their text form.
//!
//! Sums, differences, products and quotients are exact decimals kept to
//! `WORK_DIGITS` significant digits; transcendental functions and powers with
//! a non-integer exponent go through `f64` and come back as decimals.

use bigdecimal::num_bigint::{BigInt, Sign};
use bigdecimal::{BigDecimal, One, RoundingMode, Signed, ToPrimitive, Zero};
use std::str::FromStr;

pub type Num = BigDecimal;

/// Significant digits kept after every operation (at least 28).
const WORK_DIGITS: u64 = 40;

/// Largest integer exponent computed exactly by repeated multiplication.
const MAX_EXACT_EXPONENT: u64 = 100_000;

pub const PI: &str = "3.141592653589793238462643383279502884197";
pub const E: &str = "2.718281828459045235360287471352662497757";

/// Number separators chosen in the preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NumberFormat {
    /// `1,234.56`
    #[default]
    English,
    /// `1.234,56`
    Italian,
}

impl NumberFormat {
    pub fn decimal_separator(self) -> char {
        match self {
            NumberFormat::English => '.',
            NumberFormat::Italian => ',',
        }
    }

    pub fn group_separator(self) -> char {
        match self {
            NumberFormat::English => ',',
            NumberFormat::Italian => '.',
        }
    }
}

pub fn norm(x: Num) -> Num {
    let x = if x.digits() > WORK_DIGITS { x.with_prec(WORK_DIGITS) } else { x };
    x.normalized()
}

pub fn parse(text: &str) -> Option<Num> {
    Num::from_str(text).ok().map(norm)
}

pub fn lit(text: &str) -> Num {
    parse(text).expect("valid numeric literal")
}

pub fn int(value: i64) -> Num {
    Num::from(value)
}

pub fn zero() -> Num {
    Num::zero()
}

pub fn one() -> Num {
    Num::one()
}

pub fn pi() -> Num {
    lit(PI)
}

pub fn from_bigint(value: BigInt) -> Num {
    norm(Num::from(value))
}

/// Shortest decimal that reads back as `value`; `None` for NaN and infinities.
pub fn from_f64(value: f64) -> Option<Num> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some(zero());
    }
    parse(&format!("{value:e}"))
}

pub fn to_f64(x: &Num) -> f64 {
    x.to_f64().unwrap_or(f64::NAN)
}

pub fn add(a: &Num, b: &Num) -> Num {
    norm(a + b)
}

pub fn sub(a: &Num, b: &Num) -> Num {
    norm(a - b)
}

pub fn mul(a: &Num, b: &Num) -> Num {
    norm(a * b)
}

pub fn div(a: &Num, b: &Num) -> Option<Num> {
    if b.is_zero() { None } else { Some(norm(a / b)) }
}

pub fn neg(a: &Num) -> Num {
    norm(-a)
}

pub fn is_integer(x: &Num) -> bool {
    x.is_integer()
}

pub fn to_bigint(x: &Num) -> Option<BigInt> {
    if !x.is_integer() {
        return None;
    }
    let (digits, scale) = x.normalized().as_bigint_and_exponent();
    if scale > 0 {
        return None;
    }
    let shift = u32::try_from(-scale).ok()?;
    Some(digits * BigInt::from(10u32).pow(shift))
}

pub fn to_i64(x: &Num) -> Option<i64> {
    to_bigint(x)?.to_i64()
}

/// `base ^ exp`: exact for integer exponents, through f64 otherwise.
pub fn pow(base: &Num, exp: &Num) -> Option<Num> {
    if let Some(e) = to_i64(exp) {
        if e.unsigned_abs() <= MAX_EXACT_EXPONENT {
            let mut result = one();
            let mut factor = base.clone();
            let mut n = e.unsigned_abs();
            while n > 0 {
                if n & 1 == 1 {
                    result = mul(&result, &factor);
                }
                n >>= 1;
                if n > 0 {
                    factor = mul(&factor, &factor);
                }
            }
            return if e < 0 { div(&one(), &result) } else { Some(result) };
        }
    }
    from_f64(to_f64(base).powf(to_f64(exp)))
}

/// Rounds half away from zero to `places` decimals.
pub fn round_places(x: &Num, places: i64) -> Num {
    x.with_scale_round(places, RoundingMode::HalfUp).normalized()
}

pub fn ceil(x: &Num) -> Num {
    x.with_scale_round(0, RoundingMode::Ceiling).normalized()
}

pub fn floor(x: &Num) -> Num {
    x.with_scale_round(0, RoundingMode::Floor).normalized()
}

/// Power of ten of the leading digit: 1234 → 3, 0.00123 → -3.
pub fn magnitude(x: &Num) -> i64 {
    let (_, scale) = x.as_bigint_and_exponent();
    x.digits() as i64 - 1 - scale
}

/// Exact digits of `x`: sign, integer part and fraction part.
fn split_digits(x: &Num, trim_fraction: bool) -> (bool, String, String) {
    let (digits, scale) = x.as_bigint_and_exponent();
    let negative = digits.sign() == Sign::Minus && !digits.is_zero();
    let text = digits.magnitude().to_string();
    let (int_part, mut frac_part) = if scale <= 0 {
        (format!("{text}{}", "0".repeat((-scale) as usize)), String::new())
    } else {
        let scale = scale as usize;
        if scale >= text.len() {
            ("0".to_string(), format!("{}{text}", "0".repeat(scale - text.len())))
        } else {
            let cut = text.len() - scale;
            (text[..cut].to_string(), text[cut..].to_string())
        }
    };
    if trim_fraction {
        frac_part = frac_part.trim_end_matches('0').to_string();
    }
    (negative, int_part, frac_part)
}

fn group(int_part: &str, separator: char) -> String {
    let count = int_part.len();
    let mut out = String::with_capacity(count + count / 3);
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (count - i) % 3 == 0 {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

fn join(negative: bool, int_part: &str, frac_part: &str, format: NumberFormat) -> String {
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&group(int_part, format.group_separator()));
    if !frac_part.is_empty() {
        out.push(format.decimal_separator());
        out.push_str(frac_part);
    }
    out
}

/// Plain decimal text, trailing zeros removed, thousands grouped.
pub fn plain(x: &Num, format: NumberFormat) -> String {
    let (negative, int_part, frac_part) = split_digits(x, true);
    let negative = negative && (int_part != "0" || !frac_part.is_empty());
    join(negative, &int_part, &frac_part, format)
}

/// Text with exactly `places` decimals (money).
pub fn fixed(x: &Num, places: u32, format: NumberFormat) -> String {
    let rounded = x.with_scale_round(places as i64, RoundingMode::HalfUp);
    let (negative, int_part, mut frac_part) = split_digits(&rounded, false);
    while frac_part.len() < places as usize {
        frac_part.push('0');
    }
    frac_part.truncate(places as usize);
    let negative = negative && (int_part.chars().any(|c| c != '0') || frac_part.chars().any(|c| c != '0'));
    join(negative, &int_part, &frac_part, format)
}

/// Scientific notation as accepted in input: `1.2e24`, `1e-7`.
pub fn scientific(x: &Num, precision: u32, format: NumberFormat) -> String {
    if x.is_zero() {
        return "0".to_string();
    }
    let mut exponent = magnitude(x);
    let (digits, scale) = x.as_bigint_and_exponent();
    let mut mantissa = round_places(&Num::new(digits, scale + exponent), precision as i64);
    if mantissa.abs() >= int(10) {
        mantissa = norm(mantissa / int(10));
        exponent += 1;
    }
    format!("{}e{exponent}", plain(&mantissa, format))
}

/// A number as shown in the results column.
pub fn format_number(x: &Num, precision: u32, format: NumberFormat) -> String {
    if x.is_zero() {
        return "0".to_string();
    }
    let mag = magnitude(x);
    if mag < -6 {
        return scientific(x, precision, format);
    }
    if x.is_integer() {
        return if mag >= 20 { scientific(x, precision, format) } else { plain(x, format) };
    }
    if mag >= 15 {
        return scientific(x, precision, format);
    }
    let rounded = round_places(x, precision as i64);
    if rounded.is_zero() {
        // Two significant digits instead of 0.00.
        return plain(&round_places(x, -mag + 1), format);
    }
    plain(&rounded, format)
}

pub fn format_radix(x: &Num, radix: u32) -> Option<String> {
    let value = to_bigint(x)?;
    let prefix = match radix {
        16 => "0x",
        8 => "0o",
        2 => "0b",
        _ => "",
    };
    let sign = if value.sign() == Sign::Minus { "-" } else { "" };
    Some(format!("{sign}{prefix}{}", value.magnitude().to_str_radix(radix)))
}

pub fn is_negative(x: &Num) -> bool {
    x.is_negative()
}

pub fn abs(x: &Num) -> Num {
    x.abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_is_half_away_from_zero() {
        assert_eq!(plain(&round_places(&lit("2.5"), 0), NumberFormat::English), "3");
        assert_eq!(plain(&round_places(&lit("-2.5"), 0), NumberFormat::English), "-3");
        assert_eq!(plain(&round_places(&lit("2.675"), 2), NumberFormat::English), "2.68");
    }

    #[test]
    fn display_rules() {
        let en = NumberFormat::English;
        assert_eq!(format_number(&lit("1234567.891"), 2, en), "1,234,567.89");
        assert_eq!(format_number(&lit("0.000123"), 2, en), "0.00012");
        assert_eq!(format_number(&lit("1e-7"), 2, en), "1e-7");
        assert_eq!(format_number(&lit("1.2044e24"), 2, en), "1.2e24");
        assert_eq!(format_number(&lit("18446744073709551616"), 2, en), "18,446,744,073,709,551,616");
        assert_eq!(format_number(&lit("-3"), 2, en), "-3");
        assert_eq!(fixed(&lit("-5"), 2, en), "-5.00");
        assert_eq!(format_number(&lit("1235.5"), 2, NumberFormat::Italian), "1.235,5");
    }

    #[test]
    fn powers() {
        assert_eq!(pow(&int(2), &int(64)).unwrap(), lit("18446744073709551616"));
        assert_eq!(pow(&int(2), &int(-2)).unwrap(), lit("0.25"));
    }
}
