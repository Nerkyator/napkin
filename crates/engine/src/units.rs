//! Units of measure.
//!
//! Every unit converts to a base unit per dimension: metre, gram, second,
//! kelvin, radian and bit. Values come from SI and NIST tables.

use crate::num::{self, Num};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Exponents of length, mass, time, temperature, angle and data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct Dim(pub [i8; 6]);

impl Dim {
    pub const NONE: Dim = Dim([0, 0, 0, 0, 0, 0]);
    pub const LENGTH: Dim = Dim([1, 0, 0, 0, 0, 0]);
    pub const AREA: Dim = Dim([2, 0, 0, 0, 0, 0]);
    pub const VOLUME: Dim = Dim([3, 0, 0, 0, 0, 0]);
    pub const MASS: Dim = Dim([0, 1, 0, 0, 0, 0]);
    pub const TIME: Dim = Dim([0, 0, 1, 0, 0, 0]);
    pub const TEMPERATURE: Dim = Dim([0, 0, 0, 1, 0, 0]);
    pub const ANGLE: Dim = Dim([0, 0, 0, 0, 1, 0]);
    pub const DATA: Dim = Dim([0, 0, 0, 0, 0, 1]);
    pub const SPEED: Dim = Dim([1, 0, -1, 0, 0, 0]);
    pub const ENERGY: Dim = Dim([2, 1, -2, 0, 0, 0]);
    pub const POWER: Dim = Dim([2, 1, -3, 0, 0, 0]);
    pub const PRESSURE: Dim = Dim([-1, 1, -2, 0, 0, 0]);
    pub const DATA_RATE: Dim = Dim([0, 0, -1, 0, 0, 1]);
    /// Distance per volume (km/L, mpg); its inverse, volume per distance, is L/100km.
    pub const DISTANCE_PER_VOLUME: Dim = Dim([-2, 0, 0, 0, 0, 0]);

    pub fn times(self, other: Dim) -> Dim {
        let mut out = [0i8; 6];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.0[i] + other.0[i];
        }
        Dim(out)
    }

    pub fn over(self, other: Dim) -> Dim {
        self.times(other.inverse())
    }

    pub fn inverse(self) -> Dim {
        Dim(self.0.map(|e| -e))
    }
}

/// Months and years move dates along the calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calendar {
    Month,
    Year,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub symbol: String,
    pub plural: Option<String>,
    pub dim: Dim,
    pub factor: Num,
    pub offset: Num,
    pub attached: bool,
    pub calendar: Option<Calendar>,
}

impl Unit {
    pub fn to_base(&self, value: &Num) -> Num {
        num::add(&num::mul(value, &self.factor), &self.offset)
    }

    pub fn from_base(&self, value: &Num) -> Option<Num> {
        num::div(&num::sub(value, &self.offset), &self.factor)
    }

    pub fn is_affine(&self) -> bool {
        self.offset != num::zero()
    }

    /// Symbol to show after `shown`, the number as displayed.
    pub fn symbol_for(&self, shown: &str) -> &str {
        match &self.plural {
            Some(plural) if shown != "1" => plural,
            _ => &self.symbol,
        }
    }

    /// Square or cube of a length unit: `cm` → `cm²`.
    pub fn power(&self, exponent: i8) -> Option<Unit> {
        if self.dim != Dim::LENGTH || self.is_affine() || !(2..=3).contains(&exponent) {
            return None;
        }
        let mark = if exponent == 2 { '²' } else { '³' };
        let factor = num::pow(&self.factor, &num::int(exponent as i64))?;
        Some(Unit {
            symbol: format!("{}{mark}", self.symbol),
            plural: None,
            dim: Dim([exponent, 0, 0, 0, 0, 0]),
            factor,
            offset: num::zero(),
            attached: false,
            calendar: None,
        })
    }
}

pub fn convert(value: &Num, from: &Unit, to: &Unit) -> Option<Num> {
    if from.dim == to.dim {
        if from.symbol == to.symbol {
            return Some(value.clone());
        }
        return to.from_base(&from.to_base(value));
    }
    // Inverse measures convert through the reciprocal: L/100km and mpg.
    if from.dim == to.dim.inverse() && from.dim != Dim::NONE && !from.is_affine() && !to.is_affine() {
        let inverse = num::div(&num::one(), &num::mul(value, &from.factor))?;
        return num::div(&inverse, &to.factor);
    }
    None
}

/// A unit divided by another: `km/h`, `MB/s`, `L/100km`.
pub fn compound(numerator: &Unit, denominator: &Unit) -> Option<Unit> {
    if numerator.is_affine() || denominator.is_affine() {
        return None;
    }
    Some(Unit {
        symbol: format!("{}/{}", numerator.symbol, denominator.symbol),
        plural: None,
        dim: numerator.dim.over(denominator.dim),
        factor: num::div(&numerator.factor, &denominator.factor)?,
        offset: num::zero(),
        attached: false,
        calendar: None,
    })
}

/// A unit times another, shown as `a·b`; power times hours reads `kWh`.
pub fn product(a: &Unit, b: &Unit) -> Option<Unit> {
    if a.is_affine() || b.is_affine() {
        return None;
    }
    let symbol = if b.symbol == "h" && a.symbol.ends_with('W') {
        format!("{}h", a.symbol)
    } else {
        format!("{}·{}", a.symbol, b.symbol)
    };
    Some(Unit {
        symbol,
        plural: None,
        dim: a.dim.times(b.dim),
        factor: num::mul(&a.factor, &b.factor),
        offset: num::zero(),
        attached: false,
        calendar: None,
    })
}

fn fixed(spec: UnitSpec) -> Option<Unit> {
    match spec {
        UnitSpec::Fixed(unit) => Some(unit),
        _ => None,
    }
}

/// A denominator with an optional count in front: `h`, `100km`.
fn scaled(scale: Option<&Num>, word: &str) -> Option<Unit> {
    let unit = fixed(lookup(word)?)?;
    Some(match scale {
        Some(scale) => Unit {
            symbol: format!("{}{}", num::plain(scale, num::NumberFormat::English), unit.symbol),
            plural: None,
            factor: num::mul(&unit.factor, scale),
            ..unit
        },
        None => unit,
    })
}

/// `km/h`, `MB/s`, `L/100km`, written without spaces.
pub fn lookup_compound(numerator: &str, scale: Option<&Num>, denominator: &str) -> Option<Unit> {
    compound(&fixed(lookup(numerator)?)?, &scaled(scale, denominator)?)
}

/// The two parts of a rate, so that `50 km/h * 3 h` is 150 km.
pub fn split_rate(unit: &Unit) -> Option<(Unit, Unit)> {
    let (numerator, denominator) = match unit.symbol.as_str() {
        "mph" => ("mi", "h"),
        "kn" => ("nmi", "h"),
        "mpg" => ("mi", "gal"),
        symbol => symbol.split_once('/')?,
    };
    let digits: String = denominator.chars().take_while(|c| c.is_ascii_digit()).collect();
    let scale = if digits.is_empty() { None } else { num::parse(&digits) };
    Some((fixed(lookup(numerator)?)?, scaled(scale.as_ref(), &denominator[digits.len()..])?))
}

/// A unit as written; pixels and ems depend on `ppi` and `em`.
#[derive(Clone, Debug, PartialEq)]
pub enum UnitSpec {
    Fixed(Unit),
    Pixel,
    Em,
}

impl UnitSpec {
    pub fn resolve(&self, ppi: &Num, em_px: &Num) -> Option<Unit> {
        match self {
            UnitSpec::Fixed(unit) => Some(unit.clone()),
            UnitSpec::Pixel => Some(css_unit("px", num::div(&num::lit("0.0254"), ppi)?)),
            UnitSpec::Em => Some(css_unit("em", num::mul(em_px, &num::div(&num::lit("0.0254"), ppi)?))),
        }
    }

    pub fn dim(&self) -> Dim {
        match self {
            UnitSpec::Fixed(unit) => unit.dim,
            UnitSpec::Pixel | UnitSpec::Em => Dim::LENGTH,
        }
    }
}

fn css_unit(symbol: &str, factor: Num) -> Unit {
    Unit {
        symbol: symbol.to_string(),
        plural: None,
        dim: Dim::LENGTH,
        factor,
        offset: num::zero(),
        attached: false,
        calendar: None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefixes {
    None,
    Si,
    SiIec,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Css {
    Pixel,
    Em,
}

struct Def {
    display: &'static str,
    plural: Option<&'static str>,
    dim: Dim,
    factor: Num,
    offset: Num,
    prefixes: Prefixes,
    attached: bool,
    calendar: Option<Calendar>,
    css: Option<Css>,
}

impl Def {
    /// Bits and bytes take only the multiples, from kilo up: `MB` yes, `mB` no.
    fn allows_si(&self, power: i64) -> bool {
        match self.prefixes {
            Prefixes::None => false,
            Prefixes::Si => true,
            Prefixes::SiIec => power >= 3,
        }
    }
}

struct Catalog {
    defs: Vec<Def>,
    symbols: HashMap<&'static str, usize>,
    names: HashMap<&'static str, usize>,
}

/// (written, shown, power of ten)
const SI_SHORT: &[(&str, &str, i64)] = &[
    ("da", "da", 1),
    ("E", "E", 18),
    ("P", "P", 15),
    ("T", "T", 12),
    ("G", "G", 9),
    ("M", "M", 6),
    ("k", "k", 3),
    ("h", "h", 2),
    ("d", "d", -1),
    ("c", "c", -2),
    ("m", "m", -3),
    ("µ", "µ", -6),
    ("μ", "µ", -6),
    ("u", "µ", -6),
    ("n", "n", -9),
    ("p", "p", -12),
    ("f", "f", -15),
    ("a", "a", -18),
];

const SI_LONG: &[(&str, &str, i64)] = &[
    ("exa", "E", 18),
    ("peta", "P", 15),
    ("tera", "T", 12),
    ("giga", "G", 9),
    ("mega", "M", 6),
    ("kilo", "k", 3),
    ("hecto", "h", 2),
    ("deca", "da", 1),
    ("deka", "da", 1),
    ("deci", "d", -1),
    ("centi", "c", -2),
    ("milli", "m", -3),
    ("micro", "µ", -6),
    ("nano", "n", -9),
    ("pico", "p", -12),
    ("femto", "f", -15),
    ("atto", "a", -18),
];

/// (written, shown, power of two)
const IEC_SHORT: &[(&str, &str, i64)] = &[
    ("Ki", "Ki", 10),
    ("Mi", "Mi", 20),
    ("Gi", "Gi", 30),
    ("Ti", "Ti", 40),
    ("Pi", "Pi", 50),
    ("Ei", "Ei", 60),
];

const IEC_LONG: &[(&str, &str, i64)] = &[
    ("kibi", "Ki", 10),
    ("mebi", "Mi", 20),
    ("gibi", "Gi", 30),
    ("tebi", "Ti", 40),
    ("pebi", "Pi", 50),
    ("exbi", "Ei", 60),
];

/// Prefixed symbols that mean something else: times of day and `as a %`.
const NOT_PREFIXED: &[&str] = &["am", "pm", "as"];

impl Catalog {
    fn add(&mut self, def: Def, symbols: &[&'static str], names: &[&'static str]) {
        let index = self.defs.len();
        self.defs.push(def);
        for symbol in symbols {
            self.symbols.insert(symbol, index);
        }
        for name in names {
            self.names.insert(name, index);
        }
    }

    fn make(&self, index: usize, prefix: Option<(&str, Num)>) -> UnitSpec {
        let def = &self.defs[index];
        match def.css {
            Some(Css::Pixel) => return UnitSpec::Pixel,
            Some(Css::Em) => return UnitSpec::Em,
            None => {}
        }
        let (symbol, factor) = match prefix {
            Some((shown, scale)) => (format!("{shown}{}", def.display), num::mul(&def.factor, &scale)),
            None => (def.display.to_string(), def.factor.clone()),
        };
        UnitSpec::Fixed(Unit {
            symbol,
            plural: def.plural.map(str::to_string),
            dim: def.dim,
            factor,
            offset: def.offset.clone(),
            attached: def.attached,
            calendar: def.calendar,
        })
    }

    fn prefixed(&self, word: &str) -> Option<UnitSpec> {
        if NOT_PREFIXED.contains(&word) {
            return None;
        }
        for (written, shown, power) in IEC_SHORT {
            if let Some(rest) = word.strip_prefix(written) {
                if let Some(&index) = self.symbols.get(rest) {
                    if self.defs[index].prefixes == Prefixes::SiIec {
                        return Some(self.make(index, Some((shown, num::pow(&num::int(2), &num::int(*power))?))));
                    }
                }
            }
        }
        for (written, shown, power) in SI_SHORT {
            if let Some(rest) = word.strip_prefix(written) {
                if let Some(&index) = self.symbols.get(rest) {
                    if self.defs[index].allows_si(*power) {
                        return Some(self.make(index, Some((shown, num::pow(&num::int(10), &num::int(*power))?))));
                    }
                }
            }
        }
        let lower = word.to_lowercase();
        for (written, shown, power) in IEC_LONG {
            if let Some(rest) = lower.strip_prefix(written) {
                if let Some(&index) = self.names.get(rest) {
                    if self.defs[index].prefixes == Prefixes::SiIec {
                        return Some(self.make(index, Some((shown, num::pow(&num::int(2), &num::int(*power))?))));
                    }
                }
            }
        }
        for (written, shown, power) in SI_LONG {
            if let Some(rest) = lower.strip_prefix(written) {
                if let Some(&index) = self.names.get(rest) {
                    if self.defs[index].allows_si(*power) {
                        return Some(self.make(index, Some((shown, num::pow(&num::int(10), &num::int(*power))?))));
                    }
                }
            }
        }
        None
    }
}

fn def(display: &'static str, plural: Option<&'static str>, dim: Dim, factor: Num) -> Def {
    Def {
        display,
        plural,
        dim,
        factor,
        offset: num::zero(),
        prefixes: Prefixes::None,
        attached: false,
        calendar: None,
        css: None,
    }
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(build)
}

fn build() -> Catalog {
    let mut c = Catalog { defs: Vec::new(), symbols: HashMap::new(), names: HashMap::new() };
    let l = num::lit;
    let inch = l("0.0254");
    let ratio = |a: &str, b: i64| num::div(&l(a), &num::int(b)).expect("non-zero divisor");

    // Length
    c.add(Def { prefixes: Prefixes::Si, ..def("m", None, Dim::LENGTH, l("1")) }, &["m"], &["meter", "meters", "metre", "metres"]);
    c.add(def("mil", Some("mils"), Dim::LENGTH, l("0.0000254")), &[], &["mil", "mils"]);
    c.add(def("pt", None, Dim::LENGTH, ratio("0.0254", 72)), &["pt"], &["point", "points"]);
    c.add(def("line", Some("lines"), Dim::LENGTH, ratio("0.0254", 12)), &[], &["line", "lines"]);
    c.add(def("in", None, Dim::LENGTH, inch.clone()), &["in"], &["inch", "inches"]);
    c.add(def("hand", Some("hands"), Dim::LENGTH, l("0.1016")), &[], &["hand", "hands"]);
    c.add(def("ft", None, Dim::LENGTH, l("0.3048")), &["ft"], &["foot", "feet"]);
    c.add(def("yd", None, Dim::LENGTH, l("0.9144")), &["yd"], &["yard", "yards"]);
    c.add(def("rod", Some("rods"), Dim::LENGTH, l("5.0292")), &[], &["rod", "rods"]);
    c.add(def("chain", Some("chains"), Dim::LENGTH, l("20.1168")), &[], &["chain", "chains"]);
    c.add(def("furlong", Some("furlongs"), Dim::LENGTH, l("201.168")), &[], &["furlong", "furlongs"]);
    c.add(def("mi", None, Dim::LENGTH, l("1609.344")), &["mi"], &["mile", "miles"]);
    c.add(def("cable", Some("cables"), Dim::LENGTH, l("185.3184")), &[], &["cable", "cables"]);
    c.add(def("nmi", None, Dim::LENGTH, l("1852")), &["nmi"], &[]);
    c.add(def("league", Some("leagues"), Dim::LENGTH, l("4828.032")), &[], &["league", "leagues"]);
    c.add(Def { css: Some(Css::Pixel), ..def("px", None, Dim::LENGTH, l("1")) }, &["px"], &["pixel", "pixels"]);
    c.add(Def { css: Some(Css::Em), ..def("em", None, Dim::LENGTH, l("1")) }, &["em"], &[]);

    // Area
    c.add(def("are", Some("ares"), Dim::AREA, l("100")), &[], &["are", "ares"]);
    c.add(def("ha", None, Dim::AREA, l("10000")), &["ha"], &["hectare", "hectares"]);
    c.add(def("ac", None, Dim::AREA, l("4046.8564224")), &["ac"], &["acre", "acres"]);
    c.add(def("m²", None, Dim::AREA, l("1")), &["sqm"], &[]);

    // Volume
    c.add(def("m³", None, Dim::VOLUME, l("1")), &["cbm"], &[]);
    c.add(Def { prefixes: Prefixes::Si, ..def("L", None, Dim::VOLUME, l("0.001")) }, &["l", "L"], &["liter", "liters", "litre", "litres"]);
    c.add(def("tsp", None, Dim::VOLUME, l("0.00000492892159375")), &["tsp"], &["teaspoon", "teaspoons"]);
    c.add(def("tbsp", None, Dim::VOLUME, l("0.00001478676478125")), &["tbsp"], &["tablespoon", "tablespoons"]);
    c.add(def("cup", Some("cups"), Dim::VOLUME, l("0.0002365882365")), &[], &["cup", "cups"]);
    c.add(def("pint", Some("pints"), Dim::VOLUME, l("0.000473176473")), &[], &["pint", "pints"]);
    c.add(def("qt", None, Dim::VOLUME, l("0.000946352946")), &["qt"], &["quart", "quarts"]);
    c.add(def("gal", None, Dim::VOLUME, l("0.003785411784")), &["gal"], &["gallon", "gallons"]);

    // Mass
    c.add(Def { prefixes: Prefixes::Si, ..def("g", None, Dim::MASS, l("1")) }, &["g"], &["gram", "grams", "gramme", "grammes"]);
    c.add(def("t", None, Dim::MASS, l("1000000")), &["t"], &["tonne", "tonnes"]);
    c.add(def("ct", None, Dim::MASS, l("0.2")), &["ct"], &["carat", "carats"]);
    c.add(def("centner", Some("centners"), Dim::MASS, l("100000")), &[], &["centner", "centners", "quintal", "quintals"]);
    c.add(def("lb", None, Dim::MASS, l("453.59237")), &["lb", "lbs"], &["pound", "pounds"]);
    c.add(def("st", None, Dim::MASS, l("6350.29318")), &["st"], &["stone", "stones"]);
    c.add(def("oz", None, Dim::MASS, l("28.349523125")), &["oz"], &["ounce", "ounces"]);

    // Temperature
    let five_ninths = ratio("5", 9);
    c.add(def("K", None, Dim::TEMPERATURE, l("1")), &["K"], &["kelvin", "kelvins"]);
    c.add(Def { offset: l("273.15"), ..def("°C", None, Dim::TEMPERATURE, l("1")) }, &["°C", "C"], &["celsius"]);
    let fahrenheit_offset = num::sub(&l("273.15"), &num::mul(&l("32"), &five_ninths));
    c.add(Def { offset: fahrenheit_offset, ..def("°F", None, Dim::TEMPERATURE, five_ninths) }, &["°F", "F"], &["fahrenheit"]);

    // Time
    c.add(Def { prefixes: Prefixes::Si, ..def("s", None, Dim::TIME, l("1")) }, &["s", "sec"], &["second", "seconds", "sec", "secs"]);
    c.add(def("min", None, Dim::TIME, l("60")), &["min"], &["minute", "minutes", "mins"]);
    c.add(def("h", None, Dim::TIME, l("3600")), &["h", "hr"], &["hour", "hours", "hr", "hrs"]);
    c.add(def("day", Some("days"), Dim::TIME, l("86400")), &["d"], &["day", "days"]);
    c.add(def("week", Some("weeks"), Dim::TIME, l("604800")), &["wk"], &["week", "weeks", "wk", "wks"]);
    c.add(Def { calendar: Some(Calendar::Month), ..def("month", Some("months"), Dim::TIME, l("2628000")) }, &["mo"], &["month", "months"]);
    c.add(Def { calendar: Some(Calendar::Year), ..def("year", Some("years"), Dim::TIME, l("31536000")) }, &["yr", "y"], &["year", "years", "yr", "yrs"]);

    // Angle
    let degree = num::div(&num::pi(), &num::int(180)).expect("non-zero divisor");
    c.add(def("rad", None, Dim::ANGLE, l("1")), &["rad"], &["radian", "radians"]);
    c.add(Def { attached: true, ..def("°", None, Dim::ANGLE, degree) }, &["°", "deg"], &["degree", "degrees"]);

    // Data
    c.add(Def { prefixes: Prefixes::SiIec, ..def("b", None, Dim::DATA, l("1")) }, &["b"], &["bit", "bits"]);
    c.add(Def { prefixes: Prefixes::SiIec, ..def("B", None, Dim::DATA, l("8")) }, &["B"], &["byte", "bytes"]);

    // Beyond Numi: speed, energy, power, pressure, fuel and data rates.
    // Mass is in grams, so a joule is 1000 base units.
    c.add(def("mph", None, Dim::SPEED, ratio("1609.344", 3600)), &["mph"], &[]);
    c.add(def("km/h", None, Dim::SPEED, ratio("1000", 3600)), &["kph", "kmh"], &[]);
    c.add(def("kn", None, Dim::SPEED, ratio("1852", 3600)), &["kn"], &["knot", "knots"]);
    c.add(Def { prefixes: Prefixes::Si, ..def("J", None, Dim::ENERGY, l("1000")) }, &["J"], &["joule", "joules"]);
    c.add(Def { prefixes: Prefixes::Si, ..def("Wh", None, Dim::ENERGY, l("3600000")) }, &["Wh"], &[]);
    c.add(Def { prefixes: Prefixes::Si, ..def("cal", None, Dim::ENERGY, l("4184")) }, &["cal"], &["calorie", "calories"]);
    c.add(def("kcal", None, Dim::ENERGY, l("4184000")), &["Cal"], &[]);
    c.add(
        Def { prefixes: Prefixes::Si, ..def("eV", None, Dim::ENERGY, l("1.602176634e-16")) },
        &["eV"],
        &["electronvolt", "electronvolts"],
    );
    c.add(def("BTU", None, Dim::ENERGY, l("1055055.85262")), &["BTU", "Btu"], &[]);
    c.add(Def { prefixes: Prefixes::Si, ..def("W", None, Dim::POWER, l("1000")) }, &["W"], &["watt", "watts"]);
    c.add(def("hp", None, Dim::POWER, l("745699.87158227022")), &["hp"], &["horsepower"]);
    c.add(def("CV", None, Dim::POWER, l("735498.75")), &["CV", "cv", "PS"], &[]);
    c.add(Def { prefixes: Prefixes::Si, ..def("Pa", None, Dim::PRESSURE, l("1000")) }, &["Pa"], &["pascal", "pascals"]);
    c.add(Def { prefixes: Prefixes::Si, ..def("bar", None, Dim::PRESSURE, l("100000000")) }, &["bar"], &["bars"]);
    c.add(def("atm", None, Dim::PRESSURE, l("101325000")), &["atm"], &["atmosphere", "atmospheres"]);
    c.add(def("psi", None, Dim::PRESSURE, l("6894757.293168361")), &["psi"], &[]);
    c.add(def("mmHg", None, Dim::PRESSURE, l("133322.387415")), &["mmHg"], &[]);
    c.add(def("inHg", None, Dim::PRESSURE, l("3386389")), &["inHg"], &[]);
    c.add(def("torr", None, Dim::PRESSURE, ratio("101325000", 760)), &["torr", "Torr"], &[]);
    let mpg = num::div(&l("1609.344"), &l("0.003785411784")).expect("non-zero divisor");
    c.add(def("mpg", None, Dim::DISTANCE_PER_VOLUME, mpg), &["mpg"], &[]);
    c.add(Def { prefixes: Prefixes::SiIec, ..def("bps", None, Dim::DATA_RATE, l("1")) }, &["bps"], &[]);
    c
}

/// A unit written as one word: `km`, `inches`, `m²`, `kibibytes`.
pub fn lookup(word: &str) -> Option<UnitSpec> {
    let cat = catalog();
    if let Some(&index) = cat.symbols.get(word) {
        return Some(cat.make(index, None));
    }
    let lower = word.to_lowercase();
    if let Some(&index) = cat.names.get(lower.as_str()) {
        return Some(cat.make(index, None));
    }
    if let Some(unit) = cat.prefixed(word) {
        return Some(unit);
    }
    let (base, exponent) = match word.strip_suffix('²').or_else(|| word.strip_suffix('2')) {
        Some(base) => (base, 2),
        None => (word.strip_suffix('³').or_else(|| word.strip_suffix('3'))?, 3),
    };
    if base.is_empty() {
        return None;
    }
    match lookup(base)? {
        UnitSpec::Fixed(unit) => unit.power(exponent).map(UnitSpec::Fixed),
        _ => None,
    }
}

/// A unit written as one or more words; returns the unit and the words used.
pub fn lookup_phrase(words: &[&str]) -> Option<(UnitSpec, usize)> {
    if words.len() >= 2 {
        let first = words[0].to_lowercase();
        let second = words[1].to_lowercase();
        let power = match first.as_str() {
            "sq" | "square" => Some(2),
            "cu" | "cb" | "cubic" => Some(3),
            _ => None,
        };
        if let Some(power) = power {
            if let Some(UnitSpec::Fixed(unit)) = lookup(words[1]) {
                if let Some(result) = unit.power(power) {
                    return Some((UnitSpec::Fixed(result), 2));
                }
            }
        }
        let phrase = match (first.as_str(), second.as_str()) {
            ("nautical", "mile" | "miles") => Some("nmi"),
            ("tea", "spoon" | "spoons") => Some("tsp"),
            ("table", "spoon" | "spoons") => Some("tbsp"),
            _ => None,
        };
        if let Some(symbol) = phrase {
            return lookup(symbol).map(|unit| (unit, 2));
        }
    }
    lookup(words.first()?).map(|unit| (unit, 1))
}

/// Words that may not be used as variable names because they are units.
pub fn is_unit_word(word: &str) -> bool {
    lookup(word).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(word: &str) -> Unit {
        match lookup(word) {
            Some(UnitSpec::Fixed(unit)) => unit,
            other => panic!("{word}: {other:?}"),
        }
    }

    #[test]
    fn prefixes_are_case_sensitive() {
        assert_eq!(fixed("km").symbol, "km");
        assert_eq!(fixed("MB").symbol, "MB");
        assert!(lookup("mB").is_none());
        assert_eq!(fixed("KiB").symbol, "KiB");
        assert_eq!(fixed("kibibytes").symbol, "KiB");
        assert_eq!(fixed("picometers").symbol, "pm");
        assert_eq!(fixed("ml").symbol, "mL");
        assert_eq!(fixed("min").symbol, "min");
        assert!(lookup("pm").is_none());
    }

    #[test]
    fn powers_of_lengths() {
        assert_eq!(fixed("m2").symbol, "m²");
        assert_eq!(fixed("cm³").symbol, "cm³");
        assert_eq!(lookup_phrase(&["sq", "cm"]), Some((UnitSpec::Fixed(fixed("cm2")), 2)));
    }
}
