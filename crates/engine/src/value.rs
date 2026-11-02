//! Results of a line and how they are shown.

use crate::Clock;
use crate::currency::{self, Rates};
use crate::lexer::Base;
use crate::num::{self, Num, NumberFormat};
use crate::units::Unit;
use crate::zones::Zone;
use chrono::{DateTime, NaiveDate, Timelike, Utc};

#[derive(Clone, Debug)]
pub enum Value {
    Number(Num, Base),
    Quantity(Num, Unit),
    Money(Num, String),
    Percent(Num),
    Date(NaiveDate),
    Instant(Instant),
}

/// A moment shown in a time zone.
#[derive(Clone, Debug)]
pub struct Instant {
    pub utc: DateTime<Utc>,
    pub zone: Zone,
    /// Shows only the time while it falls on today (`time`, `3pm`, `Tokyo time`).
    pub time_only: bool,
    /// `now` and `time` stop at minutes.
    pub minutes: bool,
}

/// What the formatter needs besides the value.
pub struct Style<'a> {
    pub precision: u32,
    pub format: NumberFormat,
    pub clock: Clock,
    /// Today in the system time zone.
    pub today: NaiveDate,
    pub rates: Option<&'a Rates>,
}

impl Value {
    pub fn magnitude(&self) -> Option<&Num> {
        match self {
            Value::Number(n, _) | Value::Quantity(n, _) | Value::Money(n, _) | Value::Percent(n) => Some(n),
            Value::Date(_) | Value::Instant(_) => None,
        }
    }

    /// Same kind of value with another number.
    pub fn with_magnitude(&self, n: Num) -> Option<Value> {
        Some(match self {
            Value::Number(_, base) => Value::Number(n, *base),
            Value::Quantity(_, unit) => Value::Quantity(n, unit.clone()),
            Value::Money(_, code) => Value::Money(n, code.clone()),
            Value::Percent(_) => Value::Percent(n),
            Value::Date(_) | Value::Instant(_) => return None,
        })
    }

    pub fn show(&self, style: &Style) -> String {
        let number = |n: &Num| num::format_number(n, style.precision, style.format);
        match self {
            Value::Number(n, base) => match base {
                Base::Hex => num::format_radix(n, 16).unwrap_or_else(|| number(n)),
                Base::Bin => num::format_radix(n, 2).unwrap_or_else(|| number(n)),
                Base::Oct => num::format_radix(n, 8).unwrap_or_else(|| number(n)),
                Base::Sci => num::scientific(n, style.precision, style.format),
                Base::Dec => number(n),
            },
            Value::Quantity(n, unit) => {
                let shown = number(n);
                let symbol = unit.symbol_for(&shown);
                if unit.attached { format!("{shown}{symbol}") } else { format!("{shown} {symbol}") }
            }
            Value::Money(n, code) => {
                currency::format_money(n, code, currency::is_crypto(code, style.rates), style.format)
            }
            Value::Percent(n) => format!("{}%", number(n)),
            Value::Date(date) => date.format("%Y-%m-%d").to_string(),
            Value::Instant(instant) => {
                let local = instant.zone.local(&instant.utc);
                let seconds = local.second() != 0 && !instant.minutes;
                let clock = match (style.clock, seconds) {
                    (Clock::TwentyFourHour, false) => "%H:%M",
                    (Clock::TwentyFourHour, true) => "%H:%M:%S",
                    (Clock::TwelveHour, false) => "%-I:%M %p",
                    (Clock::TwelveHour, true) => "%-I:%M:%S %p",
                };
                if instant.time_only && local.date() == style.today {
                    local.format(clock).to_string()
                } else {
                    format!("{} {}", local.format("%Y-%m-%d"), local.format(clock))
                }
            }
        }
    }
}
