//! The main window: editor, results and grand total, with the sheet evaluated
//! off the main thread.

use crate::editor::{Editor, Slot};
use crate::settings::{self, Settings, Theme};
use crate::i18n::{self, tr};
use crate::{autostart, context, font, prefs, rates_task, storage, theme};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use napkin_engine::{Hl, LineKind, Rates, Sheet};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

struct Job {
    generation: u64,
    text: String,
    ctx: napkin_engine::Context,
}

pub struct Napkin {
    pub window: adw::ApplicationWindow,
    pub editor: Editor,
    total: gtk::Label,
    total_button: gtk::Button,
    total_caption: gtk::Label,
    toasts: adw::ToastOverlay,
    pub settings: RefCell<Settings>,
    pub rates: RefCell<Option<Arc<Rates>>>,
    generation: Cell<u64>,
    jobs: mpsc::Sender<Job>,
    sheet: RefCell<Option<Sheet>>,
    save_timer: RefCell<Option<glib::SourceId>>,
    theme_timer: RefCell<Option<glib::SourceId>>,
    minute: Cell<i64>,
    font_css: gtk::CssProvider,
    /// fontconfig's monospace family, read again when its file changes.
    font_family: RefCell<Option<String>>,
    font_monitor: RefCell<Option<gio::FileMonitor>>,
    font_timer: RefCell<Option<glib::SourceId>>,
    theme_css: gtk::CssProvider,
    /// Corners as round as Hyprland's windows.
    shape_css: gtk::CssProvider,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    language_monitor: RefCell<Option<gio::FileMonitor>>,
    language_timer: RefCell<Option<glib::SourceId>>,
    /// The last Omarchy palette read, kept while a theme switch is under way.
    palette: RefCell<Option<theme::Palette>>,
    pub rates_enabled: Arc<AtomicBool>,
    rates_wake: mpsc::Sender<()>,
    history: RefCell<crate::History>,
    /// Days being downloaded, or that failed: not asked again this session.
    history_asked: RefCell<HashSet<chrono::NaiveDate>>,
    history_sender: async_channel::Sender<(chrono::NaiveDate, Result<Rates, String>)>,
}

const TAGS: &[(&str, Hl)] = &[
    ("hl-unit", Hl::Unit),
    ("hl-keyword", Hl::Keyword),
    ("hl-function", Hl::Function),
    ("hl-variable", Hl::Variable),
    ("hl-comment", Hl::Comment),
    ("hl-label", Hl::Label),
];

const BASE_CSS: &str = "
.napkin-footer { padding: 4px 16px 6px 16px; }
.napkin-total { font-weight: bold; color: var(--accent-color); }
";

fn tag_name(hl: Hl) -> Option<&'static str> {
    TAGS.iter().find(|(_, h)| *h == hl).map(|(name, _)| *name)
}

/// The sheet is evaluated on its own thread; a request superseded by a newer
/// one is dropped before it starts.
fn spawn_worker(results: async_channel::Sender<(u64, Sheet)>) -> mpsc::Sender<Job> {
    let (sender, receiver) = mpsc::channel::<Job>();
    std::thread::spawn(move || {
        while let Ok(mut job) = receiver.recv() {
            while let Ok(newer) = receiver.try_recv() {
                job = newer;
            }
            let sheet = napkin_engine::evaluate(&job.text, &job.ctx);
            if results.send_blocking((job.generation, sheet)).is_err() {
                break;
            }
        }
    });
    sender
}

fn now_minute() -> i64 {
    chrono::Utc::now().timestamp() / 60
}

/// No header bar: the window manager owns the frame, and the menu lives in
/// the editor's context menu (right click, F10, Shift+F10 or the Menu key).
fn context_menu() -> gio::Menu {
    let items = gio::Menu::new();
    items.append(Some(&tr("Copy All Results")), Some("win.copy-all"));
    items.append(Some(&tr("Preferences")), Some("win.preferences"));
    items.append(Some(&tr("Keyboard Shortcuts")), Some("win.shortcuts"));
    items.append(Some(&tr("About Napkin")), Some("win.about"));
    let menu = gio::Menu::new();
    menu.append_section(None, &items);
    menu
}

/// The sheet reads left to right in every language, as its calculations do:
/// results keep to the right and the total stays under them. Menus and
/// dialogs still mirror for Arabic, Hebrew, Persian and Urdu.
fn left_to_right(widget: &gtk::Widget) {
    widget.set_direction(gtk::TextDirection::Ltr);
    let mut child = widget.first_child();
    while let Some(widget) = child {
        left_to_right(&widget);
        child = widget.next_sibling();
    }
}

pub fn build(app: &adw::Application) -> Rc<Napkin> {
    i18n::set(&i18n::wanted());
    let settings = settings::load();
    autostart::migrate();

    let editor = Editor::default();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&editor)
        .vexpand(true)
        .build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&scrolled));

    let total = gtk::Label::new(None);
    total.add_css_class("napkin-total");
    let total_button = gtk::Button::builder().child(&total).css_classes(["flat"]).visible(false).build();
    let total_caption =
        gtk::Label::builder().css_classes(["dim-label"]).hexpand(true).xalign(0.0).visible(false).build();
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("napkin-footer");
    footer.append(&total_caption);
    footer.append(&total_button);
    left_to_right(scrolled.upcast_ref());
    left_to_right(footer.upcast_ref());

    let toolbar = adw::ToolbarView::new();
    toolbar.set_content(Some(&toasts));
    toolbar.add_bottom_bar(&footer);

    let window = adw::ApplicationWindow::builder()
        .title("Napkin")
        .default_width(720)
        .default_height(520)
        .content(&toolbar)
        .build();

    // A window realized inside a GtkApplication takes the D-Bus name as its
    // Wayland app id. Realized first, it takes the program name, `napkin`,
    // which Omarchy's window rules, dock and launcher match on.
    WidgetExt::realize(&window);
    window.set_application(Some(app));

    let display = gdk::Display::default().expect("a display");
    let base_css = gtk::CssProvider::new();
    base_css.load_from_string(BASE_CSS);
    let font_css = gtk::CssProvider::new();
    let theme_css = gtk::CssProvider::new();
    let shape_css = gtk::CssProvider::new();
    for provider in [&base_css, &font_css, &theme_css, &shape_css] {
        gtk::style_context_add_provider_for_display(&display, provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }

    let (results_sender, results) = async_channel::unbounded();
    let (rates_sender, rates_updates) = async_channel::unbounded();
    let (rates_wake, rates_woken) = mpsc::channel();
    let (history_sender, history_results) = async_channel::unbounded();
    let rates_enabled = Arc::new(AtomicBool::new(settings.update_rates));
    let cached = napkin_rates::load_cache().filter(|cache| !cache.is_empty()).map(|cache| Arc::new(cache.rates));

    let napkin = Rc::new(Napkin {
        window,
        editor,
        total,
        total_button,
        total_caption,
        toasts,
        settings: RefCell::new(settings),
        rates: RefCell::new(cached),
        generation: Cell::new(0),
        jobs: spawn_worker(results_sender),
        sheet: RefCell::new(None),
        save_timer: RefCell::new(None),
        theme_timer: RefCell::new(None),
        minute: Cell::new(0),
        font_css,
        font_family: RefCell::new(font::monospace()),
        font_monitor: RefCell::new(None),
        font_timer: RefCell::new(None),
        theme_css,
        shape_css,
        monitors: RefCell::new(Vec::new()),
        language_monitor: RefCell::new(None),
        language_timer: RefCell::new(None),
        palette: RefCell::new(None),
        rates_enabled: rates_enabled.clone(),
        rates_wake,
        history: RefCell::new(crate::load_history()),
        history_asked: RefCell::new(HashSet::new()),
        history_sender,
    });

    let buffer = napkin.editor.buffer();
    for (name, _) in TAGS {
        buffer.tag_table().add(&gtk::TextTag::builder().name(*name).build());
    }
    let header_tag = gtk::TextTag::builder().name("hl-header").weight(700).build();
    buffer.tag_table().add(&header_tag);
    buffer.set_text(&storage::load().unwrap_or_else(storage::welcome));
    buffer.place_cursor(&buffer.end_iter());

    let this = napkin.clone();
    buffer.connect_changed(move |_| {
        this.evaluate();
        this.schedule_save();
    });

    let this = napkin.clone();
    glib::spawn_future_local(async move {
        while let Ok((generation, sheet)) = results.recv().await {
            if generation == this.generation.get() {
                this.fetch_history(&sheet);
                this.apply(sheet);
            }
        }
    });

    let this = napkin.clone();
    glib::spawn_future_local(async move {
        while let Ok((day, result)) = history_results.recv().await {
            match result {
                Ok(rates) => {
                    this.history.borrow_mut().insert(day, Arc::new(rates));
                    let plain = this.history.borrow().iter().map(|(d, r)| (*d, (**r).clone())).collect();
                    if let Err(e) = napkin_rates::save_history(&plain) {
                        eprintln!("napkin: cannot save past exchange rates: {e}");
                    }
                    this.evaluate();
                }
                Err(e) => {
                    let text = tr("Exchange rates of {day} not available: {error}")
                        .replace("{day}", &day.to_string())
                        .replace("{error}", &e);
                    this.toasts.add_toast(adw::Toast::new(&text))
                }
            }
        }
    });

    let this = napkin.clone();
    glib::spawn_future_local(async move {
        while let Ok(rates) = rates_updates.recv().await {
            *this.rates.borrow_mut() = Some(Arc::new(rates));
            this.evaluate();
        }
    });
    rates_task::spawn(rates_enabled, rates_woken, rates_sender);

    // Lines that depend on the clock refresh at least once a minute.
    let this = napkin.clone();
    glib::timeout_add_seconds_local(5, move || {
        let uses_time = this.sheet.borrow().as_ref().is_some_and(|s| s.lines.iter().any(|l| l.flags.uses_time));
        if uses_time && now_minute() != this.minute.get() {
            this.evaluate();
        }
        glib::ControlFlow::Continue
    });

    let this = napkin.clone();
    napkin.total_button.connect_clicked(move |_| {
        let text = this.total.text();
        if !text.is_empty() {
            this.window.clipboard().set_text(&text);
            this.toasts.add_toast(adw::Toast::new(&tr("Total copied")));
        }
    });

    // Ctrl+C, and so Omarchy's Super+C, copies the current line's result when
    // nothing is selected, as Omacalc copies its result.
    let this = napkin.clone();
    napkin.editor.connect_copy_clipboard(move |editor| {
        if !editor.buffer().has_selection() && editor.copy_result(this.cursor_line()) {
            editor.stop_signal_emission_by_name("copy-clipboard");
        }
    });

    let this = napkin.clone();
    napkin.window.connect_close_request(move |_| {
        this.save_now();
        glib::Propagation::Proceed
    });

    let this = napkin.clone();
    adw::StyleManager::default().connect_dark_notify(move |_| this.apply_colors());

    napkin.add_actions(app);
    napkin.apply_language();
    napkin.watch_language();
    napkin.apply_font();
    napkin.watch_font();
    napkin.apply_shape();
    napkin.apply_theme();
    napkin.watch_theme();
    napkin.evaluate();
    napkin
}

impl Napkin {
    pub fn evaluate(&self) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.minute.set(now_minute());
        let buffer = self.editor.buffer();
        let (start, end) = buffer.bounds();
        let text = buffer.text(&start, &end, false).to_string();
        let ctx = context(&self.settings.borrow(), self.rates.borrow().clone(), &self.history.borrow());
        let _ = self.jobs.send(Job { generation, text, ctx });
    }

    fn apply(&self, sheet: Sheet) {
        let slots = sheet
            .lines
            .iter()
            .map(|line| Slot {
                text: line.text.clone(),
                error: line.error.as_deref().map(i18n::tr_message),
                stale: line.flags.stale_rates.then(|| self.stale_note()),
            })
            .collect();
        self.editor.set_slots(slots);
        self.highlight(&sheet);
        match &sheet.total {
            Some(total) => {
                self.total.set_text(total);
                self.total_button.set_visible(true);
                self.total_caption.set_visible(true);
            }
            None => {
                self.total.set_text("");
                self.total_button.set_visible(false);
                self.total_caption.set_visible(false);
            }
        }
        *self.sheet.borrow_mut() = Some(sheet);
    }

    /// Downloads the rates of the past days that lines ask for (`on Jan 15`).
    fn fetch_history(&self, sheet: &Sheet) {
        if !self.settings.borrow().update_rates {
            return;
        }
        for day in sheet.lines.iter().filter_map(|line| line.flags.rates_needed) {
            if self.history.borrow().contains_key(&day) || !self.history_asked.borrow_mut().insert(day) {
                continue;
            }
            let sender = self.history_sender.clone();
            std::thread::spawn(move || {
                let result = napkin_rates::fetch_on(day).map_err(|e| e.to_string());
                let _ = sender.send_blocking((day, result));
            });
        }
    }

    fn stale_note(&self) -> String {
        let date = self.rates.borrow().as_ref().map(|r| r.date.clone()).unwrap_or_default();
        tr("Exchange rates of {date}, more than 48 hours old").replace("{date}", &date)
    }

    fn highlight(&self, sheet: &Sheet) {
        let buffer = self.editor.buffer();
        let (start, end) = buffer.bounds();
        buffer.remove_all_tags(&start, &end);
        for (index, line) in sheet.lines.iter().enumerate() {
            let row = index as i32;
            for span in &line.spans {
                let name = if line.kind == LineKind::Header { Some("hl-header") } else { tag_name(span.hl) };
                let Some(name) = name else { continue };
                let (Some(from), Some(to)) =
                    (buffer.iter_at_line_index(row, span.start as i32), buffer.iter_at_line_index(row, span.end as i32))
                else {
                    continue;
                };
                buffer.apply_tag_by_name(name, &from, &to);
                if line.kind == LineKind::Header {
                    buffer.apply_tag_by_name("hl-comment", &from, &to);
                }
            }
        }
    }

    fn schedule_save(self: &Rc<Self>) {
        if let Some(timer) = self.save_timer.borrow_mut().take() {
            timer.remove();
        }
        let this = self.clone();
        let timer = glib::timeout_add_local_once(Duration::from_secs(1), move || {
            this.save_timer.borrow_mut().take();
            this.save_now();
        });
        *self.save_timer.borrow_mut() = Some(timer);
    }

    pub fn save_now(&self) {
        let buffer = self.editor.buffer();
        let (start, end) = buffer.bounds();
        if let Err(e) = storage::save(&buffer.text(&start, &end, false)) {
            eprintln!("napkin: cannot save the sheet: {e}");
        }
    }

    pub fn apply_font(&self) {
        let size = self.settings.borrow().font_size;
        let family = font::css_family(self.font_family.borrow().as_deref());
        self.font_css.load_from_string(&format!("textview.napkin {{ font-family: {family}; font-size: {size}pt; }}"));
        self.editor.update_column();
    }

    /// Menus, dialogs, lists and buttons take Hyprland's corner radius, square
    /// on Omarchy, instead of libadwaita's. Application priority wins over the
    /// theme's more specific rules, so one rule covers every widget.
    fn apply_shape(&self) {
        let css = crate::hyprland::rounding()
            .map(|r| format!(":root {{ --window-radius: {r}px; }}\n* {{ border-radius: {r}px; }}\n"))
            .unwrap_or_default();
        self.shape_css.load_from_string(&css);
    }

    /// Follows `omarchy font set` without a restart.
    fn watch_font(self: &Rc<Self>) {
        let Some(path) = font::user_config() else { return };
        // The file may not exist yet: GIO watches for it to appear.
        let file = gio::File::for_path(path);
        if let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            let this = self.clone();
            monitor.connect_changed(move |_, _, _, _| this.schedule_font_reload());
            *self.font_monitor.borrow_mut() = Some(monitor);
        }
    }

    fn schedule_font_reload(self: &Rc<Self>) {
        if let Some(timer) = self.font_timer.borrow_mut().take() {
            timer.remove();
        }
        let this = self.clone();
        let timer = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            this.font_timer.borrow_mut().take();
            let family = font::monospace();
            if *this.font_family.borrow() != family {
                *this.font_family.borrow_mut() = family;
                this.apply_font();
            }
        });
        *self.font_timer.borrow_mut() = Some(timer);
    }

    /// The Omarchy palette to use now. While Omarchy swaps the theme folder,
    /// `colors.toml` is missing for a moment: the last colours stay.
    fn omarchy_palette(&self) -> Option<theme::Palette> {
        if self.settings.borrow().theme != Theme::Omarchy {
            return None;
        }
        if let Some(fresh) = theme::load_omarchy() {
            *self.palette.borrow_mut() = Some(fresh);
        }
        self.palette.borrow().clone()
    }

    pub fn apply_theme(&self) {
        let choice = self.settings.borrow().theme;
        let manager = adw::StyleManager::default();
        let palette = self.omarchy_palette();
        let scheme = match (&palette, choice) {
            (Some(p), _) if p.dark => adw::ColorScheme::ForceDark,
            (Some(_), _) | (None, Theme::Light) => adw::ColorScheme::ForceLight,
            (None, Theme::Dark) => adw::ColorScheme::ForceDark,
            (None, _) => adw::ColorScheme::Default,
        };
        manager.set_color_scheme(scheme);
        self.theme_css.load_from_string(&palette.as_ref().map(theme::css).unwrap_or_default());
        self.apply_colors();
    }

    fn apply_colors(&self) {
        let manager = adw::StyleManager::default();
        let palette = self.omarchy_palette();
        let colors = theme::colors(palette.as_ref(), manager.is_dark(), manager.accent_color_rgba());
        let table = self.editor.buffer().tag_table();
        let pairs = [
            ("hl-unit", &colors.unit),
            ("hl-keyword", &colors.keyword),
            ("hl-function", &colors.function),
            ("hl-variable", &colors.variable),
            ("hl-comment", &colors.comment),
            ("hl-label", &colors.label),
        ];
        for (name, color) in pairs {
            if let Some(tag) = table.lookup(name) {
                tag.set_foreground_rgba(Some(color));
            }
        }
        if let Some(tag) = table.lookup("hl-label") {
            tag.set_style(gtk::pango::Style::Italic);
        }
        self.editor.set_colors(colors);
    }

    /// Follows Omarchy theme changes without a restart.
    fn watch_theme(self: &Rc<Self>) {
        let Some(dir) = theme::omarchy_dir() else { return };
        let mut monitors = Vec::new();
        for path in [dir.clone(), dir.join("theme")] {
            let file = gio::File::for_path(&path);
            if let Ok(monitor) = file.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
                let this = self.clone();
                monitor.connect_changed(move |_, _, _, _| this.schedule_theme_reload());
                monitors.push(monitor);
            }
        }
        *self.monitors.borrow_mut() = monitors;
    }

    fn schedule_theme_reload(self: &Rc<Self>) {
        if self.settings.borrow().theme != Theme::Omarchy {
            return;
        }
        if let Some(timer) = self.theme_timer.borrow_mut().take() {
            timer.remove();
        }
        let this = self.clone();
        let timer = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            this.theme_timer.borrow_mut().take();
            this.apply_theme();
            this.apply_shape();
            // The theme folder may have been replaced: watch the new one.
            this.watch_theme();
        });
        *self.theme_timer.borrow_mut() = Some(timer);
    }

    /// Puts the menu and the footer in the interface language, and lays
    /// menus and dialogs out in its direction. The results' notes take it
    /// at the next evaluation, dialogs when they next open.
    fn apply_language(&self) {
        let rtl = i18n::right_to_left(&i18n::current());
        gtk::Widget::set_default_direction(if rtl { gtk::TextDirection::Rtl } else { gtk::TextDirection::Ltr });
        self.editor.set_extra_menu(Some(&context_menu()));
        self.total_caption.set_label(&tr("Total"));
        self.total_button.set_tooltip_text(Some(&tr("Copy the total")));
        self.editor.queue_draw();
    }

    /// Follows the language picked under Omarchy's Setup > Language without
    /// a restart. `omarchy-language` rewrites the file, or creates it on a
    /// first pick, and GIO watches for it to appear.
    fn watch_language(self: &Rc<Self>) {
        let Some(path) = i18n::language_file() else { return };
        let file = gio::File::for_path(path);
        if let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            let this = self.clone();
            monitor.connect_changed(move |_, _, _, _| this.schedule_language_reload());
            *self.language_monitor.borrow_mut() = Some(monitor);
        }
    }

    fn schedule_language_reload(self: &Rc<Self>) {
        if let Some(timer) = self.language_timer.borrow_mut().take() {
            timer.remove();
        }
        let this = self.clone();
        let timer = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            this.language_timer.borrow_mut().take();
            let wanted = i18n::wanted();
            if wanted != i18n::current() {
                i18n::set(&wanted);
                this.apply_language();
                this.evaluate();
            }
        });
        *self.language_timer.borrow_mut() = Some(timer);
    }

    /// Changes the settings, saves them and applies what changed.
    pub fn update_settings(self: &Rc<Self>, change: impl FnOnce(&mut Settings)) {
        let before = self.settings.borrow().clone();
        change(&mut self.settings.borrow_mut());
        let after = self.settings.borrow().clone();
        if before == after {
            return;
        }
        settings::save(&after);
        if before.font_size != after.font_size {
            self.apply_font();
        }
        if before.theme != after.theme {
            self.apply_theme();
        }
        if before.update_rates != after.update_rates {
            self.rates_enabled.store(after.update_rates, Ordering::Relaxed);
            let _ = self.rates_wake.send(());
        }
        if before.autostart != after.autostart {
            if let Err(e) = autostart::set(after.autostart) {
                let text = tr("Cannot change start at login: {error}").replace("{error}", &e.to_string());
                self.toasts.add_toast(adw::Toast::new(&text));
            }
        }
        self.evaluate();
    }

    fn cursor_line(&self) -> usize {
        let buffer = self.editor.buffer();
        buffer.iter_at_offset(buffer.cursor_position()).line().max(0) as usize
    }

    fn copy_all(&self) {
        let buffer = self.editor.buffer();
        let (start, end) = buffer.bounds();
        let text = buffer.text(&start, &end, false).to_string();
        let sheet = self.sheet.borrow();
        let lines: Vec<String> = text
            .split('\n')
            .enumerate()
            .map(|(i, line)| {
                let result = sheet.as_ref().and_then(|s| s.lines.get(i)).and_then(|l| l.text.clone());
                match result {
                    Some(result) => format!("{line}\t{result}"),
                    None => line.to_string(),
                }
            })
            .collect();
        self.window.clipboard().set_text(&lines.join("\n"));
        self.toasts.add_toast(adw::Toast::new(&tr("All results copied")));
    }

    fn wrap_selection(&self) {
        let buffer = self.editor.buffer();
        buffer.begin_user_action();
        if let Some((mut start, mut end)) = buffer.selection_bounds() {
            let end_mark = buffer.create_mark(None, &end, false);
            buffer.insert(&mut start, "(");
            end = buffer.iter_at_mark(&end_mark);
            buffer.insert(&mut end, ")");
            buffer.delete_mark(&end_mark);
        } else {
            buffer.insert_at_cursor("()");
            let mut cursor = buffer.iter_at_offset(buffer.cursor_position());
            cursor.backward_char();
            buffer.place_cursor(&cursor);
        }
        buffer.end_user_action();
    }

    fn clear(&self) {
        let buffer = self.editor.buffer();
        let (mut start, mut end) = buffer.bounds();
        buffer.begin_user_action();
        buffer.delete(&mut start, &mut end);
        buffer.end_user_action();
    }

    fn zoom(self: &Rc<Self>, step: i32) {
        self.update_settings(|s| s.font_size = (s.font_size as i32 + step).clamp(10, 32) as u32);
    }

    fn show_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name("Napkin")
            .application_icon(crate::APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("Nerkyator")
            .license_type(gtk::License::MitX11)
            .website(env!("CARGO_PKG_REPOSITORY"))
            .issue_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues"))
            .comments(tr("A calculator you write in plain words: units, currencies, dates and time zones."))
            .build();
        about.add_credit_section(
            Some(&tr("Data")),
            &[
                &format!("{} https://www.exchangerate-api.com", tr("Exchange rates by Exchange Rate API")),
                &format!("{} https://frankfurter.dev", tr("ECB rates via Frankfurter")),
                &format!("{} https://www.coingecko.com", tr("Crypto prices by CoinGecko")),
                &format!("{} https://www.geonames.org", tr("Cities by GeoNames, CC BY 4.0")),
            ],
        );
        about.present(Some(&self.window));
    }

    fn show_shortcuts(&self) {
        let groups = [
            (
                tr("Results"),
                vec![
                    (tr("Copy the result of the current line"), "<Control><Shift>c"),
                    (tr("Copy the result, when nothing is selected"), "<Control>c"),
                    (tr("Copy every line with its result"), "<Control><Alt>c"),
                ],
            ),
            (
                tr("Editing"),
                vec![
                    (tr("Wrap the selection in parentheses"), "<Control><Shift>0"),
                    (tr("Clear the sheet"), "<Control><Alt>BackSpace"),
                ],
            ),
            (
                tr("Window"),
                vec![
                    (tr("Menu"), "F10"),
                    (tr("Preferences"), "<Control>comma"),
                    (tr("Keyboard shortcuts"), "F1"),
                    (tr("Bigger text"), "<Control>plus"),
                    (tr("Smaller text"), "<Control>minus"),
                    (tr("Hide the window"), "Escape"),
                    (tr("Quit"), "<Control>q"),
                ],
            ),
        ];
        let dialog = adw::ShortcutsDialog::builder().title(tr("Keyboard Shortcuts")).build();
        for (title, items) in groups {
            let section = adw::ShortcutsSection::new(Some(&title));
            for (label, accel) in items {
                section.add(adw::ShortcutsItem::new(&label, accel));
            }
            dialog.add(section);
        }
        dialog.present(Some(&self.window));
    }

    fn add_actions(self: &Rc<Self>, app: &adw::Application) {
        let action = |name: &str, accels: &[&str], run: Box<dyn Fn()>| {
            let simple = gio::SimpleAction::new(name, None);
            simple.connect_activate(move |_, _| run());
            self.window.add_action(&simple);
            if !accels.is_empty() {
                app.set_accels_for_action(&format!("win.{name}"), accels);
            }
        };
        let this = self.clone();
        action(
            "copy-line",
            &["<Control><Shift>c"],
            Box::new(move || {
                if !this.editor.copy_result(this.cursor_line()) {
                    this.toasts.add_toast(adw::Toast::new(&tr("This line has no result")));
                }
            }),
        );
        let this = self.clone();
        action("copy-all", &["<Control><Alt>c"], Box::new(move || this.copy_all()));
        let this = self.clone();
        action(
            "wrap",
            &["<Control>parenright", "<Control><Shift>parenright", "<Control><Shift>0", "<Control><Shift>equal"],
            Box::new(move || this.wrap_selection()),
        );
        let this = self.clone();
        action("clear", &["<Control><Alt>BackSpace"], Box::new(move || this.clear()));
        let this = self.clone();
        action("zoom-in", &["<Control>plus", "<Control>equal", "<Control>KP_Add"], Box::new(move || this.zoom(1)));
        let this = self.clone();
        action("zoom-out", &["<Control>minus", "<Control>KP_Subtract"], Box::new(move || this.zoom(-1)));
        let this = self.clone();
        action(
            "hide",
            &["Escape"],
            Box::new(move || {
                // Application accels run before the dialog sees the key: Esc
                // closes an open dialog first, as it would anywhere else.
                if let Some(dialog) = this.window.visible_dialog() {
                    dialog.close();
                    return;
                }
                match crate::hyprland::special_workspace() {
                    Some(special) => crate::hyprland::toggle_special(&special),
                    None => this.window.set_visible(false),
                }
            }),
        );
        let this = self.clone();
        action("preferences", &["<Control>comma"], Box::new(move || prefs::show(&this)));
        let this = self.clone();
        action("about", &[], Box::new(move || this.show_about()));
        let this = self.clone();
        action("shortcuts", &["F1", "<Control>question"], Box::new(move || this.show_shortcuts()));
        let this = self.clone();
        action(
            "menu",
            &["F10"],
            Box::new(move || {
                this.editor.grab_focus();
                let _ = this.editor.activate_action("menu.popup", None);
            }),
        );
        let this = self.clone();
        action(
            "quit",
            &["<Control>q"],
            Box::new(move || {
                this.save_now();
                if let Some(app) = this.window.application() {
                    app.quit();
                }
            }),
        );
    }
}
