//! Napkin's calculation engine: sheet text plus a context in, results out.
//!
//! The engine never touches the network or the clock: the current time, the
//! exchange rates and the settings arrive in [`Context`], so the same input
//! always gives the same results.

pub mod currency;
pub mod eval;
pub mod golden;
pub mod lexer;
pub mod num;
pub mod parser;
pub mod sheet;
pub mod units;
pub mod value;
pub mod zones;

pub use currency::Rates;
pub use eval::Flags;
pub use num::{Num, NumberFormat};
pub use parser::Hl;
pub use sheet::{Line, LineKind, Sheet, Span, evaluate};
pub use value::Value;
pub use zones::Zone;

use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;
use std::sync::Arc;

/// How times are shown: `15:00` or `3:00 PM`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Clock {
    #[default]
    TwentyFourHour,
    TwelveHour,
}

#[derive(Clone, Debug)]
pub struct Context {
    pub now: DateTime<Utc>,
    /// The system time zone.
    pub zone: Zone,
    pub rates: Option<Arc<Rates>>,
    /// Rates of past days, for lines that end with `on <date>`.
    pub history: HashMap<NaiveDate, Arc<Rates>>,
    /// Decimals shown, 0 to 10.
    pub precision: u32,
    pub format: NumberFormat,
    pub clock: Clock,
    /// Pixels per inch for CSS units.
    pub ppi: Num,
    /// Font size in pixels for `em`.
    pub em_px: Num,
}

impl Context {
    pub fn new(now: DateTime<Utc>, zone: Zone) -> Context {
        Context {
            now,
            zone,
            rates: None,
            history: HashMap::new(),
            precision: 2,
            format: NumberFormat::English,
            clock: Clock::TwentyFourHour,
            ppi: num::int(96),
            em_px: num::int(16),
        }
    }

    pub fn today(&self) -> NaiveDate {
        self.zone.local(&self.now).date()
    }
}
