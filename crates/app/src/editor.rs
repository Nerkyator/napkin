//! The editor: a text view that draws each result in its right margin, at
//! the height of the first visual line of its line.

use crate::i18n::tr;
use crate::theme::Colors;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};
use std::cell::{Cell, RefCell};
use std::time::Duration;

/// Space around the results column, in pixels.
const PAD: i32 = 16;
const MIN_COLUMN: i32 = 140;

/// What the results column shows for one line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub text: Option<String>,
    /// Why the line has no result, shown as a tooltip.
    pub error: Option<String>,
    /// Converted with rates older than 48 hours.
    pub stale: Option<String>,
}

mod imp {
    use super::*;

    pub struct Editor {
        pub slots: RefCell<Vec<Slot>>,
        pub hover: Cell<Option<usize>>,
        pub copied: Cell<Option<usize>>,
        pub colors: RefCell<Colors>,
        pub column: Cell<i32>,
    }

    impl Default for Editor {
        fn default() -> Self {
            Editor {
                slots: RefCell::default(),
                hover: Cell::default(),
                copied: Cell::default(),
                colors: RefCell::new(crate::theme::colors(None, true, gdk::RGBA::BLUE)),
                column: Cell::new(MIN_COLUMN),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Editor {
        const NAME: &'static str = "NapkinEditor";
        type Type = super::Editor;
        type ParentType = gtk::TextView;
    }

    impl ObjectImpl for Editor {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for Editor {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.obj().update_column();
        }
    }

    impl TextViewImpl for Editor {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            self.parent_snapshot_layer(layer, snapshot.clone());
            if layer == gtk::TextViewLayer::AboveText {
                self.obj().draw_results(&snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct Editor(ObjectSubclass<imp::Editor>)
        @extends gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Default for Editor {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Editor {
    fn setup(&self) {
        self.add_css_class("napkin");
        self.set_monospace(true);
        self.set_wrap_mode(gtk::WrapMode::WordChar);
        self.set_left_margin(PAD);
        self.set_top_margin(PAD / 2);
        self.set_bottom_margin(PAD);
        self.set_right_margin(MIN_COLUMN);
        self.set_has_tooltip(true);

        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        click.connect_pressed(move |gesture, _, x, y| {
            let Some(editor) = weak.upgrade() else { return };
            if let Some(line) = editor.result_at(x, y) {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                editor.copy_result(line);
            }
        });
        self.add_controller(click);

        let motion = gtk::EventControllerMotion::new();
        let weak = self.downgrade();
        motion.connect_motion(move |_, x, y| {
            if let Some(editor) = weak.upgrade() {
                editor.set_hover(editor.result_at(x, y));
            }
        });
        let weak = self.downgrade();
        motion.connect_leave(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.set_hover(None);
            }
        });
        self.add_controller(motion);

        self.connect_query_tooltip(|editor, x, y, _, tooltip| {
            let Some(line) = editor.line_in_column(x as f64, y as f64) else { return false };
            let slots = editor.imp().slots.borrow();
            let Some(slot) = slots.get(line) else { return false };
            let text = slot.error.as_deref().or(slot.stale.as_deref());
            if let Some(text) = text {
                tooltip.set_text(Some(text));
                true
            } else {
                false
            }
        });
    }

    pub fn set_slots(&self, slots: Vec<Slot>) {
        *self.imp().slots.borrow_mut() = slots;
        self.update_column();
        self.queue_draw();
    }

    pub fn slot_text(&self, line: usize) -> Option<String> {
        self.imp().slots.borrow().get(line).and_then(|slot| slot.text.clone())
    }

    pub fn set_colors(&self, colors: Colors) {
        *self.imp().colors.borrow_mut() = colors;
        self.queue_draw();
    }

    /// The column fits the widest result, up to 45% of the width; text wraps before it.
    pub fn update_column(&self) {
        let widest = self
            .imp()
            .slots
            .borrow()
            .iter()
            .filter_map(|slot| slot.text.as_deref())
            .map(|text| self.create_pango_layout(Some(text)).pixel_size().0)
            .max()
            .unwrap_or(0);
        let limit = ((self.width() as f32) * 0.45) as i32;
        let column = (widest + 2 * PAD).clamp(MIN_COLUMN, limit.max(MIN_COLUMN));
        self.imp().column.set(column);
        if self.right_margin() != column {
            self.set_right_margin(column);
        }
    }

    fn set_hover(&self, line: Option<usize>) {
        let imp = self.imp();
        if imp.hover.get() != line {
            imp.hover.set(line);
            self.set_cursor_from_name(Some(if line.is_some() { "pointer" } else { "text" }));
            self.queue_draw();
        }
    }

    /// The line whose results cell is under (x, y), in widget coordinates.
    fn line_in_column(&self, x: f64, y: f64) -> Option<usize> {
        let (bx, by) = self.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let visible = self.visible_rect();
        if bx < visible.x() + visible.width() - self.imp().column.get() {
            return None;
        }
        let (start, _) = self.line_at_y(by);
        let first_row = self.iter_location(&start);
        if by < first_row.y() || by > first_row.y() + first_row.height() {
            return None;
        }
        Some(start.line() as usize)
    }

    /// A line with a result under (x, y).
    fn result_at(&self, x: f64, y: f64) -> Option<usize> {
        let line = self.line_in_column(x, y)?;
        self.imp().slots.borrow().get(line)?.text.as_ref()?;
        Some(line)
    }

    /// Copies a result and shows "Copied" in its place for a moment.
    pub fn copy_result(&self, line: usize) -> bool {
        let Some(text) = self.slot_text(line) else { return false };
        self.clipboard().set_text(&text);
        self.imp().copied.set(Some(line));
        self.queue_draw();
        let weak = self.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(1200), move || {
            if let Some(editor) = weak.upgrade() {
                if editor.imp().copied.get() == Some(line) {
                    editor.imp().copied.set(None);
                    editor.queue_draw();
                }
            }
        });
        true
    }

    fn draw_results(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let slots = imp.slots.borrow();
        if slots.is_empty() {
            return;
        }
        let colors = imp.colors.borrow();
        let buffer = self.buffer();
        let visible = self.visible_rect();
        let right = visible.x() + visible.width() - PAD;
        let room = imp.column.get() - 2 * PAD;
        let (first, _) = self.line_at_y(visible.y());
        for line in first.line().max(0) as usize..slots.len() {
            let Some(start) = buffer.iter_at_line(line as i32) else { break };
            let row = self.iter_location(&start);
            if row.y() > visible.y() + visible.height() {
                break;
            }
            let slot = &slots[line];
            let (text, color) = if imp.copied.get() == Some(line) {
                (tr("Copied"), &colors.result)
            } else if let Some(text) = &slot.text {
                let mark = if slot.stale.is_some() { "*" } else { "" };
                (format!("{text}{mark}"), &colors.result)
            } else if slot.error.is_some() {
                ("—".to_string(), &colors.dim)
            } else {
                continue;
            };
            let layout = self.create_pango_layout(Some(&text));
            if room > 0 {
                layout.set_width(room * pango::SCALE);
                layout.set_ellipsize(pango::EllipsizeMode::Middle);
            }
            let (width, height) = layout.pixel_size();
            let x = right - width;
            let y = row.y() + (row.height() - height) / 2;
            if imp.hover.get() == Some(line) && slot.text.is_some() {
                let bounds = graphene::Rect::new(
                    (x - 6) as f32,
                    (row.y() + 1) as f32,
                    (width + 12) as f32,
                    (row.height() - 2).max(1) as f32,
                );
                snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(bounds, 6.0));
                snapshot.append_color(&colors.hover, &bounds);
                snapshot.pop();
            }
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x as f32, y as f32));
            snapshot.append_layout(&layout, color);
            snapshot.restore();
        }
    }
}
