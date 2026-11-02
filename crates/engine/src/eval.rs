//! Evaluates expressions.

use crate::Context;
use crate::currency::Rates;
use crate::lexer::Base;
use crate::num::{self, Num};
use crate::parser::{BinOp, Expr, Func, Kw, PctOp, Target};
use crate::units::{self, Calendar, Dim, Unit, UnitSpec};
use crate::value::{Instant, Value};
use crate::zones::Zone;
use bigdecimal::num_bigint::BigInt;
use bigdecimal::ToPrimitive;
use chrono::{DateTime, Datelike, Months, NaiveDate, TimeDelta, Utc};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

/// What a line relied on; the interface uses it to know when to recompute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// Depends on the current time (`now`, `today`, time zones).
    pub uses_time: bool,
    /// Converted between currencies.
    pub uses_rates: bool,
    /// Uses `sum` or `avg`, so it does not count in later sums.
    pub aggregate: bool,
    /// The rates used are older than 48 hours.
    pub stale_rates: bool,
    /// `on Jan 15`: the rates of that day are not here yet; the app downloads them.
    pub rates_needed: Option<NaiveDate>,
}

pub struct Env<'a> {
    pub ctx: &'a Context,
    pub vars: &'a HashMap<String, Value>,
    pub prev: Option<&'a Value>,
    pub block: &'a [Value],
    pub ppi: &'a Num,
    pub em_px: &'a Num,
    /// Labels written above, to explain a label used as a variable.
    pub labels: &'a HashSet<String>,
    /// `on Jan 15`: convert currencies with the rates of that day.
    pub rates_date: Option<NaiveDate>,
    pub flags: Cell<Flags>,
}

type Res = Result<Value, String>;

fn fail<T>(message: impl Into<String>) -> Result<T, String> {
    Err(message.into())
}

/// A unit from the catalog by its symbol.
fn named_unit(symbol: &str) -> Unit {
    match units::lookup(symbol) {
        Some(UnitSpec::Fixed(unit)) => unit,
        _ => panic!("unit {symbol} missing from the catalog"),
    }
}

fn incompatible(a: &Unit, b: &Unit) -> String {
    format!("{a} and {b} measure different things", a = a.symbol, b = b.symbol)
}

fn percent_ratio(p: &Num) -> Num {
    num::div(p, &num::int(100)).expect("non-zero divisor")
}

/// Multiplies the number inside a value, keeping its unit or currency.
fn scale(value: &Value, factor: &Num) -> Res {
    let n = value.magnitude().ok_or("dates cannot be scaled")?;
    value.with_magnitude(num::mul(n, factor)).ok_or_else(|| "dates cannot be scaled".to_string())
}

fn plain(value: &Value) -> Result<Num, String> {
    match value {
        Value::Number(n, _) => Ok(n.clone()),
        _ => fail("a plain number is expected"),
    }
}

fn real(value: f64) -> Res {
    // Values a hair away from zero are float noise: sin(pi) is 0.
    let value = if value.abs() < 1e-15 { 0.0 } else { value };
    num::from_f64(value)
        .map(|n| Value::Number(n, Base::Dec))
        .ok_or_else(|| "outside the domain of the function".to_string())
}

fn add_months(date: NaiveDate, months: i64) -> Result<NaiveDate, String> {
    let count = Months::new(u32::try_from(months.unsigned_abs()).map_err(|_| "date out of range")?);
    let shifted = if months >= 0 { date.checked_add_months(count) } else { date.checked_sub_months(count) };
    shifted.ok_or_else(|| "date out of range".to_string())
}

fn add_seconds(utc: DateTime<Utc>, seconds: &Num) -> Result<DateTime<Utc>, String> {
    let millis = num::round_places(&num::mul(seconds, &num::int(1000)), 0);
    let millis = num::to_i64(&millis).ok_or("date out of range")?;
    utc.checked_add_signed(TimeDelta::milliseconds(millis)).ok_or_else(|| "date out of range".to_string())
}

fn factorial(n: &Num) -> Res {
    let n = num::to_i64(n).filter(|n| (0..=10_000).contains(n)).ok_or("factorial needs a whole number")?;
    let mut product = BigInt::from(1u32);
    for k in 2..=n {
        product *= k;
    }
    Ok(Value::Number(num::from_bigint(product), Base::Dec))
}

fn nth_root(x: &Num, n: &Num) -> Res {
    if *n == num::int(2) {
        return x.sqrt().map(|r| Value::Number(num::norm(r), Base::Dec)).ok_or_else(|| "no real square root".into());
    }
    if *n == num::int(3) {
        return Ok(Value::Number(num::norm(x.cbrt()), Base::Dec));
    }
    let (x, n_f) = (num::to_f64(x), num::to_f64(n));
    if n_f == 0.0 {
        return fail("zeroth root");
    }
    let odd = num::to_i64(n).is_some_and(|k| k % 2 != 0);
    let root = if x < 0.0 && odd { -(-x).powf(1.0 / n_f) } else { x.powf(1.0 / n_f) };
    real(root)
}

impl Env<'_> {
    fn flag(&self, update: impl FnOnce(&mut Flags)) {
        let mut flags = self.flags.get();
        update(&mut flags);
        self.flags.set(flags);
    }

    fn undefined(&self, name: &str) -> String {
        if self.labels.contains(name) {
            format!("{name} is a label, not a variable: to reuse it, write {name} = …")
        } else {
            format!("{name} is not defined")
        }
    }

    fn rates(&self) -> Option<&Rates> {
        self.ctx.rates.as_deref()
    }

    pub fn eval(&self, expr: &Expr) -> Res {
        match expr {
            Expr::Number(n, base) => Ok(Value::Number(n.clone(), *base)),
            Expr::Var(name) => self.vars.get(name).cloned().ok_or_else(|| self.undefined(name)),
            Expr::Undefined(name) => Err(self.undefined(name)),
            Expr::Kw(kw) => self.keyword(*kw),
            Expr::Date(date) => Ok(Value::Date(*date)),
            Expr::TimeOfDay { h, m, s, zone } => self.time_of_day(*h, *m, *s, *zone),
            Expr::MonthDay { month, day, year } => {
                let year = match year {
                    Some(year) => *year,
                    None => {
                        self.flag(|f| f.uses_time = true);
                        self.ctx.today().year()
                    }
                };
                NaiveDate::from_ymd_opt(year, *month, *day).map(Value::Date).ok_or_else(|| "no such date".into())
            }
            Expr::Span(from, to) => {
                let from = self.eval(from)?;
                let to = self.eval(to)?;
                match self.add_sub(true, to, from)? {
                    Value::Quantity(n, unit) if unit.dim == Dim::TIME => Ok(Value::Quantity(num::abs(&n), unit)),
                    _ => fail("the ends must be dates or times"),
                }
            }
            Expr::Until { unit, target, since } => {
                self.flag(|f| f.uses_time = true);
                let target = self.eval(target)?;
                let reference = match target {
                    Value::Date(_) => Value::Date(self.ctx.today()),
                    Value::Instant(_) => Value::Instant(Instant {
                        utc: self.ctx.now,
                        zone: self.ctx.zone,
                        time_only: false,
                        minutes: true,
                    }),
                    _ => return fail("a date or a time is expected"),
                };
                let difference =
                    if *since { self.add_sub(true, reference, target)? } else { self.add_sub(true, target, reference)? };
                self.convert(difference, &Target::Unit(unit.clone()))
            }
            Expr::DateAt { date, h, m, s, zone } => {
                let Value::Date(day) = self.eval(date)? else {
                    return fail("a time goes after a date");
                };
                let zone = zone.unwrap_or(self.ctx.zone);
                let local = day.and_hms_opt(*h, *m, *s).ok_or("invalid time")?;
                let utc = zone.to_utc(&local).ok_or("this time does not exist in that zone")?;
                Ok(Value::Instant(Instant { utc, zone, time_only: false, minutes: false }))
            }
            Expr::ZoneNow(zone) => {
                self.flag(|f| f.uses_time = true);
                Ok(Value::Instant(Instant { utc: self.ctx.now, zone: *zone, time_only: true, minutes: true }))
            }
            Expr::Unit(inner, spec) => {
                let unit = spec.resolve(self.ppi, self.em_px).ok_or("invalid unit")?;
                match self.eval(inner)? {
                    Value::Number(n, _) => Ok(Value::Quantity(n, unit)),
                    _ => fail("a unit goes after a number"),
                }
            }
            Expr::Money(inner, code) => match self.eval(inner)? {
                Value::Number(n, _) => Ok(Value::Money(n, code.clone())),
                _ => fail("a currency goes with a number"),
            },
            Expr::Percent(inner) => match self.eval(inner)? {
                Value::Number(n, _) => Ok(Value::Percent(n)),
                _ => fail("% goes after a number"),
            },
            Expr::Neg(inner) => {
                let value = self.eval(inner)?;
                let n = value.magnitude().ok_or("dates cannot be negative")?;
                value.with_magnitude(num::neg(n)).ok_or_else(|| "dates cannot be negative".into())
            }
            Expr::Bin(op, a, b) => {
                let a = self.eval(a)?;
                let b = self.eval(b)?;
                self.binary(*op, a, b)
            }
            Expr::Pct(op, a, b) => {
                let a = self.eval(a)?;
                let b = self.eval(b)?;
                self.percent(*op, a, b)
            }
            Expr::Convert(inner, target) => {
                let value = self.eval(inner)?;
                self.convert(value, target)
            }
            Expr::Call(func, args) => {
                let values = args.iter().map(|a| self.eval(a)).collect::<Result<Vec<_>, _>>()?;
                self.call(*func, &values)
            }
            Expr::Factorial(inner) => factorial(&plain(&self.eval(inner)?)?),
        }
    }

    fn keyword(&self, kw: Kw) -> Res {
        let today = || {
            self.flag(|f| f.uses_time = true);
            self.ctx.today()
        };
        let now = |time_only| {
            self.flag(|f| f.uses_time = true);
            Value::Instant(Instant { utc: self.ctx.now, zone: self.ctx.zone, time_only, minutes: true })
        };
        Ok(match kw {
            Kw::Today => Value::Date(today()),
            Kw::Tomorrow => Value::Date(today().succ_opt().ok_or("date out of range")?),
            Kw::Yesterday => Value::Date(today().pred_opt().ok_or("date out of range")?),
            Kw::Now => now(false),
            Kw::Time => now(true),
            Kw::Prev => self.prev.cloned().ok_or("no result above")?,
            Kw::Sum => self.sum()?,
            Kw::Avg => self.average()?,
            Kw::Pi => Value::Number(num::pi(), Base::Dec),
            Kw::E => Value::Number(num::lit(num::E), Base::Dec),
            Kw::Ppi => Value::Number(self.ppi.clone(), Base::Dec),
        })
    }

    fn time_of_day(&self, h: u32, m: u32, s: u32, zone: Option<Zone>) -> Res {
        self.flag(|f| f.uses_time = true);
        let zone = zone.unwrap_or(self.ctx.zone);
        let date = zone.local(&self.ctx.now).date();
        let local = date.and_hms_opt(h, m, s).ok_or("invalid time")?;
        let utc = zone.to_utc(&local).ok_or("this time does not exist in that zone")?;
        Ok(Value::Instant(Instant { utc, zone, time_only: true, minutes: false }))
    }

    /// `sum`: the results above in the current block, added like `+`.
    pub fn sum(&self) -> Res {
        self.flag(|f| f.aggregate = true);
        let mut values = self.block.iter();
        let first = values.next().ok_or("nothing to add up above")?.clone();
        values.try_fold(first, |acc, value| self.add_sub(false, acc, value.clone()))
    }

    fn average(&self) -> Res {
        let total = self.sum()?;
        self.div(total, Value::Number(num::int(self.block.len() as i64), Base::Dec))
    }

    fn binary(&self, op: BinOp, a: Value, b: Value) -> Res {
        match op {
            BinOp::Add => self.add_sub(false, a, b),
            BinOp::Sub => self.add_sub(true, a, b),
            BinOp::Mul => self.mul(a, b),
            BinOp::Div => self.div(a, b),
            BinOp::Pow => self.pow(a, b),
            BinOp::Mod => self.modulo(a, b),
            BinOp::Shl | BinOp::Shr | BinOp::And | BinOp::Xor | BinOp::Or => self.bitwise(op, a, b),
        }
    }

    /// `+` and `-`: mixed units and currencies end in the last one.
    pub fn add_sub(&self, sub: bool, a: Value, b: Value) -> Res {
        let combine = |x: &Num, y: &Num| if sub { num::sub(x, y) } else { num::add(x, y) };
        if let Value::Percent(p) = &b {
            // Adding or removing p% multiplies the value on the left.
            let ratio = percent_ratio(p);
            let factor = if sub { num::sub(&num::one(), &ratio) } else { num::add(&num::one(), &ratio) };
            return scale(&a, &factor);
        }
        match (a, b) {
            (Value::Number(x, base), Value::Number(y, _)) => Ok(Value::Number(combine(&x, &y), base)),
            (Value::Quantity(x, u), Value::Quantity(y, v)) => {
                let x = units::convert(&x, &u, &v).ok_or_else(|| incompatible(&u, &v))?;
                Ok(Value::Quantity(combine(&x, &y), v))
            }
            (Value::Quantity(x, u), Value::Number(y, _)) => Ok(Value::Quantity(combine(&x, &y), u)),
            (Value::Number(x, _), Value::Quantity(y, v)) => Ok(Value::Quantity(combine(&x, &y), v)),
            (Value::Money(x, c), Value::Money(y, d)) => {
                let x = self.exchange(&x, &c, &d)?;
                Ok(Value::Money(combine(&x, &y), d))
            }
            (Value::Money(x, c), Value::Number(y, _)) => Ok(Value::Money(combine(&x, &y), c)),
            (Value::Number(x, _), Value::Money(y, d)) => Ok(Value::Money(combine(&x, &y), d)),
            (Value::Percent(x), Value::Number(y, _)) => Ok(Value::Percent(combine(&x, &y))),
            (Value::Date(d), Value::Quantity(n, u)) if u.dim == Dim::TIME => self.shift_date(d, &n, &u, sub),
            (Value::Quantity(n, u), Value::Date(d)) if u.dim == Dim::TIME && !sub => self.shift_date(d, &n, &u, false),
            (Value::Instant(t), Value::Quantity(n, u)) if u.dim == Dim::TIME => self.shift_instant(t, &n, &u, sub),
            (Value::Quantity(n, u), Value::Instant(t)) if u.dim == Dim::TIME && !sub => {
                self.shift_instant(t, &n, &u, false)
            }
            (Value::Date(a), Value::Date(b)) if sub => {
                Ok(Value::Quantity(num::int((a - b).num_days()), named_unit("day")))
            }
            (a @ (Value::Instant(_) | Value::Date(_)), b @ (Value::Instant(_) | Value::Date(_))) if sub => {
                let seconds = self.timestamp(&a)? - self.timestamp(&b)?;
                let hours = num::div(&num::int(seconds), &num::int(3600)).expect("non-zero divisor");
                Ok(Value::Quantity(hours, named_unit("h")))
            }
            _ => fail("these values cannot be added together"),
        }
    }

    fn timestamp(&self, value: &Value) -> Result<i64, String> {
        match value {
            Value::Instant(t) => Ok(t.utc.timestamp()),
            Value::Date(d) => {
                let midnight = d.and_hms_opt(0, 0, 0).ok_or("invalid date")?;
                Ok(self.ctx.zone.to_utc(&midnight).ok_or("invalid date")?.timestamp())
            }
            _ => fail("not a date"),
        }
    }

    fn shift_date(&self, date: NaiveDate, n: &Num, unit: &Unit, sub: bool) -> Res {
        let n = if sub { num::neg(n) } else { n.clone() };
        // Months and years follow the calendar: Jan 31 + 1 month is Feb 28.
        if let (Some(calendar), Some(count)) = (unit.calendar, num::to_i64(&n)) {
            let months = if calendar == Calendar::Year { count * 12 } else { count };
            return Ok(Value::Date(add_months(date, months)?));
        }
        let seconds = unit.to_base(&n);
        let whole_days = num::div(&seconds, &num::int(86_400)).filter(num::is_integer).and_then(|d| num::to_i64(&d));
        if let Some(days) = whole_days {
            let shifted = date.checked_add_signed(TimeDelta::days(days)).ok_or("date out of range")?;
            return Ok(Value::Date(shifted));
        }
        // A fraction of a day turns the date into a moment.
        let midnight = date.and_hms_opt(0, 0, 0).ok_or("invalid date")?;
        let start = self.ctx.zone.to_utc(&midnight).ok_or("invalid date")?;
        let utc = add_seconds(start, &seconds)?;
        Ok(Value::Instant(Instant { utc, zone: self.ctx.zone, time_only: false, minutes: false }))
    }

    fn shift_instant(&self, mut t: Instant, n: &Num, unit: &Unit, sub: bool) -> Res {
        let n = if sub { num::neg(n) } else { n.clone() };
        if let (Some(calendar), Some(count)) = (unit.calendar, num::to_i64(&n)) {
            let months = if calendar == Calendar::Year { count * 12 } else { count };
            let local = t.zone.local(&t.utc);
            let date = add_months(local.date(), months)?;
            t.utc = t.zone.to_utc(&date.and_time(local.time())).ok_or("invalid time")?;
            return Ok(Value::Instant(t));
        }
        t.utc = add_seconds(t.utc, &unit.to_base(&n))?;
        Ok(Value::Instant(t))
    }

    fn mul(&self, a: Value, b: Value) -> Res {
        match (a, b) {
            (Value::Percent(p), Value::Percent(q)) => Ok(Value::Percent(num::mul(&p, &percent_ratio(&q)))),
            (value, Value::Percent(p)) | (Value::Percent(p), value) => scale(&value, &percent_ratio(&p)),
            (Value::Number(x, base), Value::Number(y, _)) => Ok(Value::Number(num::mul(&x, &y), base)),
            (Value::Quantity(x, u), Value::Number(y, _)) | (Value::Number(y, _), Value::Quantity(x, u)) => {
                Ok(Value::Quantity(num::mul(&x, &y), u))
            }
            (Value::Money(x, c), Value::Number(y, _)) | (Value::Number(y, _), Value::Money(x, c)) => {
                Ok(Value::Money(num::mul(&x, &y), c))
            }
            // Length by length gives an area: 3 m * 4 m = 12 m².
            (Value::Quantity(x, u), Value::Quantity(y, v)) if u.dim == Dim::LENGTH && v.dim == Dim::LENGTH => {
                let x = units::convert(&x, &u, &v).ok_or_else(|| incompatible(&u, &v))?;
                let area = v.power(2).ok_or("no unit for this product")?;
                Ok(Value::Quantity(num::mul(&x, &y), area))
            }
            // A price times an amount: 3.5 kWh * €0.25 = €0.88.
            (Value::Money(x, c), Value::Quantity(y, _)) | (Value::Quantity(y, _), Value::Money(x, c)) => {
                Ok(Value::Money(num::mul(&x, &y), c))
            }
            (Value::Quantity(x, u), Value::Quantity(y, v)) => self.product(x, u, y, v),
            _ => fail("these values cannot be multiplied"),
        }
    }

    fn div(&self, a: Value, b: Value) -> Res {
        let quotient = |x: &Num, y: &Num| num::div(x, y).ok_or_else(|| "division by zero".to_string());
        match (a, b) {
            (value, Value::Percent(p)) => {
                let inverse = quotient(&num::one(), &percent_ratio(&p))?;
                scale(&value, &inverse)
            }
            (Value::Number(x, base), Value::Number(y, _)) => Ok(Value::Number(quotient(&x, &y)?, base)),
            (Value::Quantity(x, u), Value::Number(y, _)) => Ok(Value::Quantity(quotient(&x, &y)?, u)),
            (Value::Quantity(x, u), Value::Quantity(y, v)) => match units::convert(&x, &u, &v) {
                Some(x) if u.dim == v.dim => Ok(Value::Number(quotient(&x, &y)?, Base::Dec)),
                // Different measures make a rate: 100 km / 2 h = 50 km/h.
                _ => {
                    let rate = units::compound(&u, &v).ok_or_else(|| incompatible(&u, &v))?;
                    Ok(Value::Quantity(quotient(&x, &y)?, rate))
                }
            },
            (Value::Money(x, c), Value::Number(y, _)) => Ok(Value::Money(quotient(&x, &y)?, c)),
            (Value::Money(x, c), Value::Money(y, d)) => {
                let x = self.exchange(&x, &c, &d)?;
                Ok(Value::Number(quotient(&x, &y)?, Base::Dec))
            }
            (Value::Percent(p), Value::Number(y, _)) => Ok(Value::Percent(quotient(&p, &y)?)),
            _ => fail("these values cannot be divided"),
        }
    }

    /// `50 km/h * 3 h` is 150 km and `6 L/100km * 300 km` is 18 L; other
    /// products keep both units, and `2 kW * 3 h` reads 6 kWh.
    fn product(&self, x: Num, u: Unit, y: Num, v: Unit) -> Res {
        for (rate_value, rate, other_value, other) in [(&x, &u, &y, &v), (&y, &v, &x, &u)] {
            if let Some((numerator, denominator)) = units::split_rate(rate) {
                if denominator.dim == other.dim {
                    if let Some(count) = units::convert(other_value, other, &denominator) {
                        return Ok(Value::Quantity(num::mul(rate_value, &count), numerator));
                    }
                }
            }
        }
        let unit = units::product(&u, &v).ok_or("no unit for this product")?;
        Ok(Value::Quantity(num::mul(&x, &y), unit))
    }

    fn pow(&self, a: Value, b: Value) -> Res {
        let exponent = plain(&b).map_err(|_| "the exponent must be a plain number".to_string())?;
        match a {
            Value::Number(x, base) => {
                Ok(Value::Number(num::pow(&x, &exponent).ok_or("result out of range")?, base))
            }
            Value::Quantity(x, u) => {
                let power = num::to_i64(&exponent).and_then(|p| i8::try_from(p).ok()).ok_or("no unit for this power")?;
                let unit = u.power(power).ok_or("no unit for this power")?;
                Ok(Value::Quantity(num::pow(&x, &exponent).ok_or("result out of range")?, unit))
            }
            _ => fail("only numbers can be raised to a power"),
        }
    }

    fn modulo(&self, a: Value, b: Value) -> Res {
        let y = plain(&b)?;
        let x = a.magnitude().ok_or("mod needs numbers")?;
        let quotient = num::div(x, &y).ok_or("division by zero")?;
        let rest = num::sub(x, &num::mul(&y, &num::floor(&quotient)));
        a.with_magnitude(rest).ok_or_else(|| "mod needs numbers".into())
    }

    fn bitwise(&self, op: BinOp, a: Value, b: Value) -> Res {
        let (Value::Number(x, base), Value::Number(y, _)) = (a, b) else {
            return fail("bit operators need whole numbers");
        };
        let (Some(x), Some(y)) = (num::to_bigint(&x), num::to_bigint(&y)) else {
            return fail("bit operators need whole numbers");
        };
        let shift = || y.to_usize().filter(|s| *s <= 100_000).ok_or("shift out of range");
        let result = match op {
            BinOp::Shl => x << shift()?,
            BinOp::Shr => x >> shift()?,
            BinOp::And => x & y,
            BinOp::Xor => x ^ y,
            _ => x | y,
        };
        Ok(Value::Number(num::from_bigint(result), base))
    }

    /// Both values as numbers in the unit or currency of the second.
    fn same_kind(&self, a: Value, b: Value) -> Result<(Num, Num), String> {
        match (a, b) {
            (Value::Money(x, c), Value::Money(y, d)) => Ok((self.exchange(&x, &c, &d)?, y)),
            (Value::Quantity(x, u), Value::Quantity(y, v)) => {
                Ok((units::convert(&x, &u, &v).ok_or_else(|| incompatible(&u, &v))?, y))
            }
            (a, b) => match (a.magnitude(), b.magnitude()) {
                (Some(x), Some(y)) if std::mem::discriminant(&a) == std::mem::discriminant(&b)
                    || matches!(a, Value::Number(..))
                    || matches!(b, Value::Number(..)) =>
                {
                    Ok((x.clone(), y.clone()))
                }
                _ => fail("these values cannot be compared"),
            },
        }
    }

    fn percent(&self, op: PctOp, a: Value, b: Value) -> Res {
        let one = num::one();
        match op {
            PctOp::AsOf | PctOp::AsOn | PctOp::AsOff => {
                let (x, y) = self.same_kind(a, b)?;
                let ratio = match op {
                    PctOp::AsOf => num::div(&x, &y),
                    PctOp::AsOn => num::div(&num::sub(&x, &y), &y),
                    _ => num::div(&num::sub(&y, &x), &y),
                }
                .ok_or("division by zero")?;
                Ok(Value::Percent(num::mul(&ratio, &num::int(100))))
            }
            _ => {
                let Value::Percent(p) = a else {
                    return fail("a percentage goes before of, on and off");
                };
                let r = percent_ratio(&p);
                let factor = match op {
                    PctOp::Of => Some(r),
                    PctOp::On => Some(num::add(&one, &r)),
                    PctOp::Off => Some(num::sub(&one, &r)),
                    PctOp::OfWhat => num::div(&one, &r),
                    PctOp::OnWhat => num::div(&one, &num::add(&one, &r)),
                    _ => num::div(&one, &num::sub(&one, &r)),
                }
                .ok_or("division by zero")?;
                scale(&b, &factor)
            }
        }
    }

    fn convert(&self, value: Value, target: &Target) -> Res {
        match target {
            Target::Unit(spec) => {
                let unit = spec.resolve(self.ppi, self.em_px).ok_or("invalid unit")?;
                match value {
                    Value::Quantity(n, from) => {
                        let converted = units::convert(&n, &from, &unit).ok_or_else(|| incompatible(&from, &unit))?;
                        Ok(Value::Quantity(converted, unit))
                    }
                    // A plain number takes the target unit: `20 in cm` is 20 cm.
                    Value::Number(n, _) => Ok(Value::Quantity(n, unit)),
                    _ => fail("only measures convert to units"),
                }
            }
            Target::Currency(code) => match value {
                Value::Money(n, from) => Ok(Value::Money(self.exchange(&n, &from, code)?, code.clone())),
                Value::Number(n, _) => Ok(Value::Money(n, code.clone())),
                _ => fail("only money converts to a currency"),
            },
            Target::Base(base) => match value {
                Value::Number(n, _) => Ok(Value::Number(n, *base)),
                _ => fail("only plain numbers change base"),
            },
            Target::Zone(zone) => match value {
                Value::Instant(mut t) => {
                    t.zone = *zone;
                    Ok(Value::Instant(t))
                }
                _ => fail("only times convert to a time zone"),
            },
        }
    }

    fn exchange(&self, n: &Num, from: &str, to: &str) -> Result<Num, String> {
        if from == to {
            return Ok(n.clone());
        }
        let rates = match self.rates_date {
            Some(date) => self.rates_on(date)?,
            None => self.rates().ok_or("no exchange rates yet")?,
        };
        let from_rate = rates.usd_rate(from).ok_or_else(|| format!("no exchange rate for {from}"))?;
        let to_rate = rates.usd_rate(to).ok_or_else(|| format!("no exchange rate for {to}"))?;
        let stale = self.rates_date.is_none()
            && rates.data_time().is_some_and(|t| self.ctx.now.timestamp() - t > 48 * 3600);
        self.flag(|f| {
            f.uses_rates = true;
            f.stale_rates |= stale;
        });
        let usd = num::div(n, &from_rate).ok_or("invalid exchange rate")?;
        Ok(num::mul(&usd, &to_rate))
    }

    /// Historical rates, which the app downloads when a line asks for them.
    fn rates_on(&self, date: NaiveDate) -> Result<&Rates, String> {
        if date > self.ctx.today() {
            return fail("there are no exchange rates for days to come");
        }
        match self.ctx.history.get(&date) {
            Some(rates) => Ok(rates),
            None => {
                self.flag(|f| f.rates_needed = Some(date));
                fail(format!("exchange rates of {date} not downloaded yet"))
            }
        }
    }

    fn radians(&self, value: &Value) -> Result<f64, String> {
        match value {
            Value::Number(n, _) => Ok(num::to_f64(n)),
            Value::Quantity(n, unit) if unit.dim == Dim::ANGLE => Ok(num::to_f64(&unit.to_base(n))),
            _ => fail("an angle is expected"),
        }
    }

    fn call(&self, func: Func, args: &[Value]) -> Res {
        let first = args.first().ok_or("missing argument")?;
        let keep_unit = |f: &dyn Fn(&Num) -> Num| -> Res {
            let n = first.magnitude().ok_or("a number is expected")?;
            first.with_magnitude(f(n)).ok_or_else(|| "a number is expected".into())
        };
        match func {
            Func::Sqrt => nth_root(&plain(first)?, &num::int(2)),
            Func::Cbrt => nth_root(&plain(first)?, &num::int(3)),
            Func::Root => match args {
                [x] => nth_root(&plain(x)?, &num::int(2)),
                [n, x] => nth_root(&plain(x)?, &plain(n)?),
                _ => fail("root takes the degree and the number"),
            },
            Func::Abs => keep_unit(&num::abs),
            Func::Round => match args {
                [_] => keep_unit(&|n| num::round_places(n, 0)),
                [_, places] => {
                    let places = num::to_i64(&plain(places)?).ok_or("decimals must be a whole number")?;
                    keep_unit(&|n| num::round_places(n, places))
                }
                _ => fail("round takes a number and the decimals"),
            },
            Func::Ceil => keep_unit(&num::ceil),
            Func::Floor => keep_unit(&num::floor),
            Func::Log => {
                let (base, x) = match args {
                    [x] => (10.0, num::to_f64(&plain(x)?)),
                    [b, x] => (num::to_f64(&plain(b)?), num::to_f64(&plain(x)?)),
                    _ => return fail("log takes the base and the number"),
                };
                if x <= 0.0 || base <= 0.0 || base == 1.0 {
                    return fail("outside the domain of log");
                }
                real(if base == 10.0 { x.log10() } else { x.ln() / base.ln() })
            }
            Func::Ln => {
                let x = num::to_f64(&plain(first)?);
                if x <= 0.0 {
                    return fail("outside the domain of ln");
                }
                real(x.ln())
            }
            Func::Fact => factorial(&plain(first)?),
            Func::Sin => real(self.radians(first)?.sin()),
            Func::Cos => real(self.radians(first)?.cos()),
            Func::Tan => real(self.radians(first)?.tan()),
            Func::Asin | Func::Acos | Func::Atan => {
                let x = num::to_f64(&plain(first)?);
                let angle = match func {
                    Func::Asin => x.asin(),
                    Func::Acos => x.acos(),
                    _ => x.atan(),
                };
                let Value::Number(n, _) = real(angle)? else { unreachable!() };
                Ok(Value::Quantity(n, named_unit("rad")))
            }
            Func::Sinh => real(num::to_f64(&plain(first)?).sinh()),
            Func::Cosh => real(num::to_f64(&plain(first)?).cosh()),
            Func::Tanh => real(num::to_f64(&plain(first)?).tanh()),
            Func::FromUnix => {
                let millis = num::to_i64(&num::round_places(&num::mul(&plain(first)?, &num::int(1000)), 0))
                    .ok_or("time out of range")?;
                let utc = DateTime::from_timestamp_millis(millis).ok_or("time out of range")?;
                Ok(Value::Instant(Instant { utc, zone: self.ctx.zone, time_only: false, minutes: false }))
            }
        }
    }
}
