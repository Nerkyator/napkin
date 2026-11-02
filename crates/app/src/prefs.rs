//! The preferences dialog.

use crate::i18n::tr;
use crate::settings::{Clock, Format, Theme};
use crate::theme;
use crate::window::Napkin;
use adw::prelude::*;
use gtk::glib;
use std::rc::Rc;

const THEMES: [Theme; 4] = [Theme::Omarchy, Theme::System, Theme::Light, Theme::Dark];

fn spin(title: &str, subtitle: &str, min: f64, max: f64, value: u32) -> adw::SpinRow {
    let row = adw::SpinRow::with_range(min, max, 1.0);
    row.set_title(title);
    if !subtitle.is_empty() {
        row.set_subtitle(subtitle);
    }
    row.set_value(value as f64);
    row
}

pub fn show(napkin: &Rc<Napkin>) {
    let settings = napkin.settings.borrow().clone();
    let page = adw::PreferencesPage::new();

    let results = adw::PreferencesGroup::builder().title(tr("Results")).build();
    let precision = spin(&tr("Decimals"), &tr("Up to how many decimals results show"), 0.0, 10.0, settings.precision);
    let this = napkin.clone();
    precision.connect_value_notify(move |row| this.update_settings(|s| s.precision = row.value() as u32));
    results.add(&precision);
    let format = adw::ComboRow::builder()
        .title(tr("Number format"))
        .subtitle(tr("Both for reading and for showing numbers"))
        // The examples say it in every language, and fit beside the title.
        .model(&gtk::StringList::new(&["1,234.56", "1.234,56"]))
        .selected(if settings.format == Format::Italian { 1 } else { 0 })
        .build();
    let this = napkin.clone();
    format.connect_selected_notify(move |row| {
        this.update_settings(|s| s.format = if row.selected() == 1 { Format::Italian } else { Format::English })
    });
    results.add(&format);
    let clock = adw::ComboRow::builder()
        .title(tr("Clock"))
        .model(&gtk::StringList::new(&["15:00", "3:00 PM"]))
        .selected(if settings.clock == Clock::TwelveHour { 1 } else { 0 })
        .build();
    let this = napkin.clone();
    clock.connect_selected_notify(move |row| {
        this.update_settings(|s| s.clock = if row.selected() == 1 { Clock::TwelveHour } else { Clock::TwentyFourHour })
    });
    results.add(&clock);
    page.add(&results);

    let looks = adw::PreferencesGroup::builder().title(tr("Appearance")).build();
    let size = spin(&tr("Text size"), &tr("In points"), 10.0, 32.0, settings.font_size);
    let this = napkin.clone();
    size.connect_value_notify(move |row| this.update_settings(|s| s.font_size = row.value() as u32));
    looks.add(&size);
    let omarchy = if theme::omarchy_available() { "Omarchy".to_string() } else { tr("Omarchy (not found)") };
    let style = adw::ComboRow::builder()
        .title(tr("Colors"))
        .model(&gtk::StringList::new(&[&omarchy, &tr("System"), &tr("Light"), &tr("Dark")]))
        .selected(THEMES.iter().position(|t| *t == settings.theme).unwrap_or(1) as u32)
        .build();
    let this = napkin.clone();
    style.connect_selected_notify(move |row| {
        let choice = THEMES.get(row.selected() as usize).copied().unwrap_or(Theme::System);
        this.update_settings(|s| s.theme = choice)
    });
    looks.add(&style);
    page.add(&looks);

    let css = adw::PreferencesGroup::builder()
        .title(tr("CSS units"))
        .description(tr("Starting values; a line such as ppi = 220 changes them from there down"))
        .build();
    let ppi = spin(&tr("Pixels per inch"), "ppi", 1.0, 1000.0, settings.ppi);
    let this = napkin.clone();
    ppi.connect_value_notify(move |row| this.update_settings(|s| s.ppi = row.value() as u32));
    css.add(&ppi);
    let em = spin(&tr("Font size"), &tr("em, in pixels"), 1.0, 200.0, settings.em);
    let this = napkin.clone();
    em.connect_value_notify(move |row| this.update_settings(|s| s.em = row.value() as u32));
    css.add(&em);
    page.add(&css);

    let rates = adw::PreferencesGroup::builder().title(tr("Exchange rates")).build();
    let date = napkin.rates.borrow().as_ref().map(|r| r.date.clone()).filter(|d| !d.is_empty());
    let attribution = tr(napkin_rates::ATTRIBUTION);
    let subtitle = match date {
        Some(date) => {
            tr("{attribution}. Current data: {date}").replace("{attribution}", &attribution).replace("{date}", &date)
        }
        None => attribution,
    };
    let update = adw::SwitchRow::builder()
        .title(tr("Update exchange rates"))
        .subtitle(glib::markup_escape_text(&subtitle))
        .active(settings.update_rates)
        .build();
    let this = napkin.clone();
    update.connect_active_notify(move |row| this.update_settings(|s| s.update_rates = row.is_active()));
    rates.add(&update);
    page.add(&rates);

    let startup = adw::PreferencesGroup::builder().title(tr("Startup")).build();
    let login = adw::SwitchRow::builder()
        .title(tr("Start at login"))
        .subtitle(tr("Napkin starts hidden; the shortcut shows it"))
        .active(settings.autostart)
        .build();
    let this = napkin.clone();
    login.connect_active_notify(move |row| this.update_settings(|s| s.autostart = row.is_active()));
    startup.add(&login);
    page.add(&startup);

    // libadwaita's own title would keep the language Napkin started in.
    let dialog = adw::PreferencesDialog::builder().title(tr("Preferences")).build();
    dialog.add(&page);
    dialog.present(Some(&napkin.window));
}
