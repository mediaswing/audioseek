//! The controls the tabs are built from, drawn so screen readers and the
//! keyboard work with them, and the window's fonts and colours. These follow
//! the Speech Output Engine's.

use super::*;

/// Font family used for the tab labels (Google Sans Bold).
pub(super) fn tab_font() -> egui::FontFamily {
    egui::FontFamily::Name("tab".into())
}

/// Google Sans (SIL Open Font License, see assets/fonts/OFL.txt): Medium for
/// all body text, Bold for the tabs. egui's built-in fonts stay behind them
/// as fallbacks for symbols and scripts Google Sans does not cover.
fn setup_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "GoogleSans-Medium".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/GoogleSans-Medium.ttf"))),
    );
    fonts.font_data.insert(
        "GoogleSans-Bold".into(),
        Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/GoogleSans-Bold.ttf"))),
    );
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "GoogleSans-Medium".into());
    let mut tab = vec!["GoogleSans-Bold".to_owned()];
    tab.extend(fallbacks);
    fonts.families.insert(tab_font(), tab);
    ctx.set_fonts(fonts);
}

pub(super) fn setup_style(ctx: &egui::Context) {
    use egui::{FontId, TextStyle};
    setup_fonts(ctx);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(24.0)),
            (TextStyle::Body, FontId::proportional(17.0)),
            (TextStyle::Button, FontId::proportional(17.0)),
            (TextStyle::Monospace, FontId::monospace(16.0)),
            (TextStyle::Small, FontId::proportional(14.0)),
        ]
        .into();
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size.y = CONTROL_HEIGHT;
    });
}

/// A heading that screen readers list as a heading, so users can jump to it.
pub(super) fn heading(ui: &mut Ui, text: &str) {
    let resp = ui.heading(text);
    ui.ctx().accesskit_node_builder(resp.id, |node| {
        node.set_role(Role::Heading);
        node.set_level(2);
        // egui stores a plain label's text as its value; headings need a name.
        node.set_label(text);
    });
}

pub(super) fn full_button(ui: &mut Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| ui.add_sized([ui.available_width(), CONTROL_HEIGHT], Button::new(text))).inner
}

/// A labelled, full-width, one-line text box. `id` keeps it the same box
/// whatever its label says, so focus can find it.
pub(super) fn text_field(ui: &mut Ui, id: Id, label: &str, value: &mut String, editable: bool) -> egui::Response {
    let label = ui.label(label);
    let resp = if editable {
        let edit = egui::TextEdit::singleline(value).id(id).desired_width(f32::INFINITY);
        ui.add_sized([ui.available_width(), CONTROL_HEIGHT], edit)
    } else {
        // Read-only, but still focusable so a screen reader can review it.
        let mut text = value.as_str();
        let edit = egui::TextEdit::singleline(&mut text).id(id).desired_width(f32::INFINITY);
        ui.add_sized([ui.available_width(), CONTROL_HEIGHT], edit)
    };
    let resp = resp.labelled_by(label.id);
    if !editable {
        ui.ctx().accesskit_node_builder(resp.id, |node| node.set_read_only());
    }
    resp
}

/// A labelled, full-width dropdown. While it has focus and is closed, the Up,
/// Down, Home and End keys change the choice directly, as in a native combo
/// box; Space, Enter or Alt+Down (Option+Down on a Mac) opens the list, and
/// Alt+Up or Escape closes it. Returns the response, and whether the choice
/// changed.
pub(super) fn dropdown<S: AsRef<str>>(
    ui: &mut Ui,
    id_salt: &str,
    label: &str,
    options: &[S],
    selected: &mut usize,
    enabled: bool,
) -> (egui::Response, bool) {
    let label_resp = ui.label(label);
    let mut changed = false;
    let current = options.get(*selected).map_or("", |o| o.as_ref()).to_owned();
    let inner = ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt(id_salt)
            .width(ui.available_width())
            .height(400.0)
            .selected_text(current.clone())
            .show_ui(ui, |ui| {
                for (i, option) in options.iter().enumerate() {
                    let resp = ui.add_sized(
                        [ui.available_width(), CONTROL_HEIGHT],
                        Button::selectable(i == *selected, option.as_ref()),
                    );
                    if resp.clicked() {
                        *selected = i;
                        changed = true;
                    }
                }
            })
    });
    let combo = inner.inner;
    let resp = combo.response.labelled_by(label_resp.id);
    let popup_open = combo.inner.is_some();
    // Where egui keeps whether this dropdown's list is open.
    let popup_id = resp.id.with("popup");

    if enabled && popup_open && ui.input_mut(|i| consume_exact(i, Modifiers::ALT, Key::ArrowUp)) {
        egui::Popup::close_id(ui.ctx(), popup_id);
    }
    if enabled && resp.has_focus() && !popup_open && !options.is_empty() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(resp.id, EventFilter { vertical_arrows: true, ..Default::default() })
        });
        if ui.input_mut(|i| consume_exact(i, Modifiers::ALT, Key::ArrowDown)) {
            egui::Popup::open_id(ui.ctx(), popup_id);
        }
        let last = options.len() - 1;
        let new = ui.input_mut(|i| {
            if consume_exact(i, Modifiers::NONE, Key::ArrowDown) {
                Some((*selected + 1).min(last))
            } else if consume_exact(i, Modifiers::NONE, Key::ArrowUp) {
                Some(selected.saturating_sub(1))
            } else if consume_exact(i, Modifiers::NONE, Key::Home) {
                Some(0)
            } else if consume_exact(i, Modifiers::NONE, Key::End) {
                Some(last)
            } else {
                None
            }
        });
        if let Some(new) = new
            && new != *selected
        {
            *selected = new;
            changed = true;
            ui.ctx().request_repaint();
        }
    }
    let value = options.get(*selected).map_or("", |o| o.as_ref()).to_owned();
    ui.ctx().accesskit_node_builder(resp.id, |node| {
        node.set_role(Role::ComboBox);
        node.set_value(value);
    });
    (resp, changed)
}

/// Takes a press of `key` with exactly `modifiers` out of the input. egui's
/// own `consume_key` ignores extra Shift and Alt, so Alt+Down would count as
/// Down.
fn consume_exact(input: &mut egui::InputState, modifiers: Modifiers, key: Key) -> bool {
    let before = input.events.len();
    input.events.retain(|e| {
        !matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. } if *k == key && m.matches_exact(modifiers))
    });
    input.events.len() != before
}

/// Draws a thick, high-contrast outline around the focused control.
pub(super) fn draw_focus_ring(ctx: &egui::Context) {
    let Some(id) = ctx.memory(|m| m.focused()) else { return };
    let Some(resp) = ctx.read_response(id) else { return };
    let color = if ctx.global_style().visuals.dark_mode {
        Color32::from_rgb(255, 215, 0)
    } else {
        Color32::from_rgb(0, 70, 200)
    };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("focus_ring")));
    painter.rect_stroke(resp.rect.expand(3.0), 4.0, egui::Stroke::new(3.0, color), egui::StrokeKind::Outside);
}

/// Opens a folder in the file manager.
pub(super) fn open_folder(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    open(dir.as_os_str())
}

/// Opens a web page in the default browser.
pub(super) fn open_url(url: &str) -> std::io::Result<()> {
    open(url.as_ref())
}

/// Opens a folder or web page with whatever the system uses for it.
fn open(target: &std::ffi::OsStr) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(target).spawn().map(|_| ())
}
