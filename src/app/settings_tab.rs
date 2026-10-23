//! The Settings tab: where episodes are saved and how they are named.

use super::*;
use crate::settings::Naming;

impl PodApp {
    pub(super) fn browse_folder(&mut self) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("Choose the download folder")
            .set_directory(&self.settings.download_dir)
            .pick_folder()
        else {
            return;
        };
        self.folder_input = dir.display().to_string();
        self.settings.download_dir = dir;
        self.save_settings();
        self.announce(format!("Episodes will be saved in {}.", self.folder_input));
        self.focus = Some(Target::FolderField);
    }

    pub(super) fn open_download_folder(&mut self) {
        let dir = self.settings.download_dir.clone();
        if let Err(e) = open_folder(&dir) {
            self.show_error(format!("Could not open {}: {e}", dir.display()));
        }
    }

    pub(super) fn open_data_folder(&mut self) {
        let dir = crate::paths::config_dir();
        if let Err(e) = open_folder(&dir) {
            self.show_error(format!("Could not open {}: {e}", dir.display()));
        }
    }

    pub(super) fn check_for_update(&mut self) {
        if self.checking_for_update {
            return;
        }
        self.checking_for_update = true;
        self.announce("Checking for a newer version…");
        self.rep.spawn(worker::Job::UpdateCheck, |rep| worker::check_for_update(rep, true));
    }

    pub(super) fn update_checked(&mut self, result: Result<Option<String>, String>, asked: bool) {
        self.checking_for_update = false;
        match result {
            Ok(Some(version)) => {
                // The check made at startup doesn't talk over a job the user
                // has already started; the Settings tab still says.
                if asked || !self.is_busy() {
                    self.announce(format!(
                        "Version {version} is available. The Settings tab has a button to download it."
                    ));
                }
                self.new_version = Some(version);
            }
            Ok(None) if asked => {
                self.announce(format!("You have the newest version, {}.", env!("CARGO_PKG_VERSION")));
            }
            // A failed check at startup is only logged, as the app works
            // just as well without it.
            Err(e) if asked => self.show_error(format!("Could not check for a newer version: {e}")),
            _ => {}
        }
    }

    fn open_release_page(&mut self) {
        if let Err(e) = open_url(crate::update::LATEST_RELEASE_PAGE) {
            self.show_error(format!("Could not open {}: {e}", crate::update::LATEST_RELEASE_PAGE));
        }
    }

    pub(super) fn settings_tab(&mut self, ui: &mut Ui) {
        heading(ui, "Settings");

        let label = format!("Download folder{}", self.keymap.hint(Action::FolderField));
        let resp = text_field(ui, field_id(Target::FolderField), &label, &mut self.folder_input, true);
        self.focus_if_wanted(Target::FolderField, &resp);
        if resp.lost_focus() {
            let typed = self.folder_input.trim().to_owned();
            if typed.is_empty() {
                self.folder_input = self.settings.download_dir.display().to_string();
            } else if !std::path::Path::new(&typed).is_absolute() {
                // A relative folder would depend on how the app was started.
                self.folder_input = self.settings.download_dir.display().to_string();
                self.show_error(format!(
                    "{typed} is not a full path to a folder, so the download folder is still {}. Type the whole \
                     path, or choose the folder{}.",
                    self.folder_input,
                    self.keymap.hint(Action::Browse)
                ));
                self.focus = Some(Target::FolderField);
            } else if std::path::Path::new(&typed) != self.settings.download_dir {
                self.settings.download_dir = (&typed).into();
                self.save_settings();
                self.announce(format!("Episodes will be saved in {typed}."));
            }
        }
        if full_button(ui, &format!("Choose the download folder…{}", self.keymap.hint(Action::Browse)), true).clicked()
        {
            self.browse_folder();
        }
        if full_button(ui, &format!("Open the download folder{}", self.keymap.hint(Action::OpenFolder)), true).clicked()
        {
            self.open_download_folder();
        }

        let names = Naming::ALL.map(Naming::label);
        let mut chosen = Naming::ALL.iter().position(|n| *n == self.settings.naming).unwrap_or(0);
        let label = format!("File names{}", self.keymap.hint(Action::Naming));
        let (resp, changed) = dropdown(ui, "naming", &label, &names, &mut chosen, true);
        self.focus_if_wanted(Target::Naming, &resp);
        if changed {
            self.settings.naming = Naming::ALL[chosen];
            self.save_settings();
        }
        ui.label("Dates and times are when each episode was published, in this computer's time zone.");

        let label = format!("Put each podcast in a folder of its own{}", self.keymap.hint(Action::FolderPerPodcast));
        let resp = ui.checkbox(&mut self.settings.folder_per_podcast, label);
        self.focus_if_wanted(Target::FolderPerPodcast, &resp);
        if resp.changed() {
            self.save_settings();
        }

        ui.add_space(6.0);
        heading(ui, "Saved feeds");
        ui.label(
            "Every feed you import or add is kept in this file, with when it was last fetched and which \
             episodes have been downloaded, so the list survives losing the OPML file.",
        );
        let mut path = crate::paths::feeds_file().display().to_string();
        text_field(ui, Id::new("feeds_file"), "Feeds file", &mut path, false);
        ui.label("Problems are written to a log file in the same folder, which helps when reporting them.");
        let mut path = crate::paths::log_file().display().to_string();
        text_field(ui, Id::new("log_file"), "Log file", &mut path, false);
        if full_button(ui, &format!("Open the folder holding feeds.json{}", self.keymap.hint(Action::DataFolder)), true)
            .clicked()
        {
            self.open_data_folder();
        }

        ui.add_space(6.0);
        heading(ui, "Updates");
        ui.label(match &self.new_version {
            Some(version) => format!("Version {version} is available. This is version {}.", env!("CARGO_PKG_VERSION")),
            None => format!("This is version {}.", env!("CARGO_PKG_VERSION")),
        });
        if self.new_version.is_some() && full_button(ui, "Open the download page", true).clicked() {
            self.open_release_page();
        }
        let label =
            if self.checking_for_update { "Checking for a newer version…" } else { "Check for a newer version now" };
        if full_button(ui, label, !self.checking_for_update).clicked() {
            self.check_for_update();
        }
        if ui.checkbox(&mut self.settings.check_for_updates, "Check for a newer version when the app starts").changed()
        {
            self.save_settings();
        }
    }
}
