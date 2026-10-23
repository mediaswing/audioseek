//! The Shortcuts tab: turning keyboard shortcuts on or off, and changing
//! each one, as on the Speech Output Engine's Settings tab.

use super::*;

impl PodApp {
    /// Builds the keymap again from the settings, and saves them.
    fn set_shortcuts(&mut self) {
        self.keymap = Keymap::new(self.settings.shortcuts_enabled(), &self.settings.shortcuts, Platform::CURRENT);
        self.save_settings();
    }

    /// Each list offers only shortcuts that pass the conflict checks in
    /// `shortcuts.rs` and that no other action uses, so picking one can never
    /// take a key from somewhere else.
    pub(super) fn shortcuts_tab(&mut self, ui: &mut Ui) {
        heading(ui, "Keyboard shortcuts");
        let options = ["On", "Off"];
        let mut index = usize::from(!self.settings.shortcuts_enabled());
        let (_, changed) = dropdown(ui, "shortcuts_on", "Use keyboard shortcuts", &options, &mut index, true);
        if changed {
            self.settings.shortcuts_on = Some(index == 0);
            self.set_shortcuts();
            self.announce(if index == 0 {
                "Keyboard shortcuts are on."
            } else {
                "Keyboard shortcuts are off. Tab, the arrow keys, Space and Enter still work."
            });
        }

        let platform = self.keymap.platform();
        for place in shortcuts::Place::ALL {
            ui.add_space(6.0);
            heading(ui, place.heading());
            for action in Action::ALL.into_iter().filter(|a| a.place() == place) {
                let choices: Vec<Shortcut> = action
                    .choices(platform)
                    .into_iter()
                    .filter(|s| self.keymap.used_by(*s, action).is_none())
                    .collect();
                let mut labels = vec!["None".to_owned()];
                labels.extend(choices.iter().map(|s| self.keymap.describe(action, *s, Tab::ALL.len())));
                let current = self.keymap.binding(action);
                let mut index = current.and_then(|c| choices.iter().position(|s| *s == c)).map_or(0, |i| i + 1);
                let id = format!("shortcut_{}", action.id());
                let (_, changed) = dropdown(ui, &id, action.label(), &labels, &mut index, true);
                if changed {
                    let chosen = index.checked_sub(1).and_then(|i| choices.get(i).copied());
                    if chosen == action.default_shortcut(platform) {
                        self.settings.shortcuts.remove(action.id());
                    } else {
                        let text = chosen.map_or_else(|| shortcuts::OFF.to_owned(), |s| s.to_text(platform));
                        self.settings.shortcuts.insert(action.id().to_owned(), text);
                    }
                    self.set_shortcuts();
                }
            }
        }

        ui.add_space(6.0);
        ui.label(
            "Each list offers only keys that screen readers, the operating system and typing don't need, \
             and that nothing else here uses. To give a key to something else, set its current owner to \
             None first.",
        );
        if full_button(ui, &format!("Show the list of shortcuts{}", self.keymap.hint(Action::Help)), true).clicked() {
            self.show_shortcuts();
        }
        let changed = !self.settings.shortcuts.is_empty() || self.settings.shortcuts_on.is_some();
        if full_button(ui, "Reset the shortcuts to the defaults", changed).clicked() {
            self.settings.shortcuts.clear();
            self.settings.shortcuts_on = None;
            self.set_shortcuts();
            self.announce("The shortcuts are back to the defaults, and turned on.");
        }
    }
}
