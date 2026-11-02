//! Napkin: a natural-language calculator for Linux, at home on Omarchy.

mod autostart;
mod cli;
mod editor;
mod font;
mod hyprland;
mod i18n;
mod prefs;
mod rates_task;
mod settings;
mod storage;
mod theme;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};
use napkin_engine::{Clock, Context, NumberFormat, Rates, Zone, num, zones};
use std::process::ExitCode;
use std::sync::Arc;

/// One name for the binary, the launcher entry, the icon and the window class,
/// as Omarchy's own apps do.
pub const APP_ID: &str = "napkin";

/// The D-Bus name that keeps Napkin to one instance. GApplication wants a
/// dotted name, so it cannot be `napkin`; the window class is set apart.
const BUS_NAME: &str = "io.github.nerkyator.Napkin";

/// Rates of past days, for lines that end with `on <date>`.
pub type History = std::collections::HashMap<chrono::NaiveDate, Arc<Rates>>;

/// The past rates downloaded so far, from the cache.
pub fn load_history() -> History {
    napkin_rates::load_history().into_iter().map(|(day, rates)| (day, Arc::new(rates))).collect()
}

/// The engine's context: now, the system time zone, the rates and the settings.
pub fn context(settings: &settings::Settings, rates: Option<Arc<Rates>>, history: &History) -> Context {
    let zone = iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| zones::iana(&name))
        .unwrap_or(Zone::Named(chrono_tz::UTC));
    let mut ctx = Context::new(chrono::Utc::now(), zone);
    ctx.rates = rates;
    ctx.history = history.clone();
    ctx.precision = settings.precision;
    ctx.format = match settings.format {
        settings::Format::English => NumberFormat::English,
        settings::Format::Italian => NumberFormat::Italian,
    };
    ctx.clock = match settings.clock {
        settings::Clock::TwentyFourHour => Clock::TwentyFourHour,
        settings::Clock::TwelveHour => Clock::TwelveHour,
    };
    ctx.ppi = num::int(settings.ppi as i64);
    ctx.em_px = num::int(settings.em as i64);
    ctx
}

fn run_app() -> ExitCode {
    glib::set_prgname(Some(APP_ID));
    let app = adw::Application::builder()
        .application_id(BUS_NAME)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.add_main_option(
        "toggle",
        glib::Char::from(b't'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Show the window, or hide it when it has the focus",
        None,
    );
    app.add_main_option(
        "hidden",
        glib::Char::from(b'H'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Start without showing the window",
        None,
    );
    app.connect_command_line(|app, command_line| {
        let options = command_line.options_dict();
        let existing = app.windows().into_iter().find_map(|w| w.downcast::<adw::ApplicationWindow>().ok());
        let window = match existing {
            Some(window) => window,
            None => {
                let napkin = window::build(app);
                if options.contains("hidden") {
                    return glib::ExitCode::SUCCESS;
                }
                napkin.window.clone()
            }
        };
        if options.contains("toggle") && window.is_visible() {
            // In a scratchpad, toggle the scratchpad so the window stays there.
            if let Some(special) = hyprland::special_workspace() {
                hyprland::toggle_special(&special);
                return glib::ExitCode::SUCCESS;
            }
            if window.is_active() {
                window.set_visible(false);
                return glib::ExitCode::SUCCESS;
            }
        }
        window.present();
        glib::ExitCode::SUCCESS
    });
    let code = app.run();
    if code == glib::ExitCode::SUCCESS { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("eval") => cli::eval(&args[2..]),
        Some("test") => cli::test(&args[2..]),
        Some("help" | "--help" | "-h") => {
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => {
            println!("napkin {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            // SAFETY: GTK and the workers have not started any thread yet.
            unsafe { i18n::export_to_gettext() };
            run_app()
        }
    }
}
