//! The window: four tabs (Feeds, Episodes, Settings, Shortcuts) with every
//! control on its own full-width line, laid out like the Speech Output
//! Engine's.
//!
//! Accessibility notes:
//! * egui publishes the interface to screen readers through AccessKit
//!   (UI Automation on Windows, NSAccessibility on macOS, AT-SPI on Linux).
//! * Every text field and dropdown is linked to its visible label, the tabs
//!   are exposed with the tab role and selected state, and the status line is
//!   a polite live region so progress and errors are announced.
//! * Everything works from the keyboard, and every control has a shortcut of
//!   its own that works from any tab (see `shortcuts.rs`; F1 lists them).
//! * A thick focus ring is drawn around whichever control has focus.

mod episodes_tab;
mod feeds_tab;
mod settings_tab;
mod shortcuts_tab;
mod widgets;

use crate::APP_TITLE;
use crate::feed::Podcast;
use crate::library::Library;
use crate::settings::Settings;
use crate::shortcuts::{self, Action, Command, KeyContext, Keymap, Platform, Shortcut};
use crate::worker::{self, Msg, Outcome, Reporter};
use egui::accesskit::{Live, Role};
use egui::{Button, Color32, EventFilter, Id, Key, Modifiers, RichText, Ui};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use widgets::*;

const CONTROL_HEIGHT: f32 = 36.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Feeds,
    Episodes,
    Settings,
    Shortcuts,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Feeds, Tab::Episodes, Tab::Settings, Tab::Shortcuts];

    fn label(self) -> &'static str {
        match self {
            Tab::Feeds => "Feeds",
            Tab::Episodes => "Episodes",
            Tab::Settings => "Settings",
            Tab::Shortcuts => "Shortcuts",
        }
    }

    fn id(self) -> Id {
        Id::new(("tab", self as u8))
    }
}

/// A control that a shortcut has asked to move focus to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Tab(Tab),
    FeedAddress,
    FeedList,
    PodcastPicker,
    Filter,
    EpisodeList,
    FolderField,
    Naming,
    FolderPerPodcast,
}

/// Ids of the text boxes, so focus can find them.
fn field_id(target: Target) -> Id {
    Id::new(("field", format!("{target:?}")))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Fetching,
    Downloading,
}

struct Job {
    kind: JobKind,
    cancel: Arc<AtomicBool>,
    /// How many feeds or episodes the job has.
    total: usize,
    /// The episode downloading now, counting from 1, and its title.
    current: usize,
    title: String,
    /// Episodes saved, already there, and failed with why.
    saved: usize,
    already: usize,
    failed: Vec<String>,
}

pub struct PodApp {
    settings: Settings,
    library: Library,
    /// Feeds fetched since the app started, by address.
    podcasts: HashMap<String, Podcast>,
    /// Feeds that failed to fetch this session, with why, by address.
    fetch_errors: HashMap<String, String>,
    tab: Tab,
    rep: Reporter,
    rx: Receiver<Msg>,
    job: Option<Job>,
    progress: f32,
    status: String,
    /// The feed chosen on the Feeds and Episodes tabs.
    selected: usize,
    feed_address: String,
    filter: String,
    /// Ticked episodes, by feed address.
    ticked: HashMap<String, HashSet<String>>,
    /// The episode row that last had focus, so the list shortcut returns to it.
    episode_focus: Option<String>,
    /// The feed and how many episodes matched the filter last frame.
    last_shown: Option<(usize, usize)>,
    folder_input: String,
    focus: Option<Target>,
    keymap: Keymap,
    /// Shortcuts pressed since the last frame, taken from the input before
    /// egui saw it.
    pending: Vec<Command>,
    /// The tab buttons' ids last frame, to tell when one has focus.
    tab_buttons: Vec<Id>,
    /// A newer version found on GitHub.
    new_version: Option<String>,
    checking_for_update: bool,
}

impl PodApp {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, library: Library) -> Self {
        setup_style(&cc.egui_ctx);
        let (tx, rx) = channel();
        let keymap = Keymap::new(settings.shortcuts_enabled(), &settings.shortcuts, Platform::CURRENT);
        let mut status = match library.feeds.len() {
            0 => format!("Ready. Import an OPML file to begin{}.", keymap.hint(Action::ImportOpml)),
            1 => "Ready. 1 feed.".to_owned(),
            n => format!("Ready. {n} feeds."),
        };
        if let Some(help) = keymap.active(Action::Help) {
            status.push_str(&format!(" {} lists the shortcuts.", help.short(keymap.platform())));
        }
        let rep = Reporter::new(tx, cc.egui_ctx.clone());
        if settings.check_for_updates {
            rep.spawn(|rep| worker::check_for_update(rep, false));
        }
        Self {
            folder_input: settings.download_dir.display().to_string(),
            checking_for_update: settings.check_for_updates,
            settings,
            library,
            podcasts: HashMap::new(),
            fetch_errors: HashMap::new(),
            tab: Tab::Feeds,
            rep,
            rx,
            job: None,
            progress: 0.0,
            status,
            selected: 0,
            feed_address: String::new(),
            filter: String::new(),
            ticked: HashMap::new(),
            episode_focus: None,
            last_shown: None,
            focus: Some(Target::Tab(Tab::Feeds)),
            keymap,
            pending: Vec::new(),
            tab_buttons: Vec::new(),
            new_version: None,
        }
    }

    fn announce(&mut self, text: impl Into<String>) {
        let mut text = text.into();
        log::info!("status: {text}");
        // Screen readers ignore a live region whose text has not changed, so
        // a repeated message would be silent. A trailing no-break space makes
        // the text differ.
        if text == self.status.trim_end_matches('\u{a0}') && !self.status.ends_with('\u{a0}') {
            text.push('\u{a0}');
        }
        self.status = text;
    }

    /// Reports something that went wrong: in the status line and in an
    /// error dialog, which screen readers read out when it opens.
    fn show_error(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::error!("{text}");
        self.announce(text.clone());
        rfd::MessageDialog::new()
            .set_title(APP_TITLE)
            .set_description(text)
            .set_buttons(rfd::MessageButtons::Ok)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }

    fn confirm(&self, text: &str) -> bool {
        let answer = rfd::MessageDialog::new()
            .set_title(APP_TITLE)
            .set_description(text)
            .set_buttons(rfd::MessageButtons::YesNo)
            .set_level(rfd::MessageLevel::Info)
            .show();
        answer == rfd::MessageDialogResult::Yes
    }

    fn save_library(&mut self) {
        if let Err(e) = self.library.save() {
            self.show_error(format!("Could not save the list of feeds: {e}"));
        }
    }

    fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// Says that something else is running, if it is.
    fn refuse_if_busy(&mut self) -> bool {
        if self.is_busy() {
            self.announce(format!("Please wait for the current job to finish, or cancel it{}.", self.keymap.hint(Action::Cancel)));
        }
        self.is_busy()
    }

    fn start_job(&mut self, kind: JobKind, total: usize) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        self.job = Some(Job {
            kind,
            cancel: cancel.clone(),
            total,
            current: 0,
            title: String::new(),
            saved: 0,
            already: 0,
            failed: Vec::new(),
        });
        self.progress = 0.0;
        cancel
    }

    fn cancel(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
            self.announce("Cancelling…");
        }
    }

    /// Fetches the feeds at `urls` in the background.
    fn fetch(&mut self, urls: Vec<String>) {
        if urls.is_empty() || self.refuse_if_busy() {
            return;
        }
        let count = urls.len();
        let cancel = self.start_job(JobKind::Fetching, count);
        self.announce(if count == 1 { "Fetching 1 feed…".to_owned() } else { format!("Fetching {count} feeds…") });
        self.rep.spawn(move |rep| worker::refresh(rep, urls, cancel));
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::FeedFetched(url, result) => self.feed_fetched(url, result),
                Msg::RefreshDone { fetched, failed, cancelled } => {
                    self.job = None;
                    let mut text = match fetched {
                        1 => "Fetched 1 feed.".to_owned(),
                        n => format!("Fetched {n} feeds."),
                    };
                    if failed > 0 {
                        text.push_str(&format!(" {failed} could not be fetched; the feed list says which."));
                    }
                    if cancelled {
                        text = format!("Cancelled. {text}");
                    }
                    self.announce(text);
                }
                Msg::Downloading(n, title) => {
                    if let Some(job) = &mut self.job {
                        job.current = n;
                        job.title = title.clone();
                        let total = job.total;
                        self.announce(format!("Downloading {n} of {total}: {title}"));
                    }
                }
                Msg::EpisodeDone { feed_url, episode_id, outcome } => self.episode_done(feed_url, episode_id, outcome),
                Msg::DownloadsDone { cancelled } => self.downloads_done(cancelled),
                Msg::Progress(p) => self.progress = p.clamp(0.0, 1.0),
                Msg::UpdateChecked { result, asked } => self.update_checked(result, asked),
            }
        }
    }

    fn feed_fetched(&mut self, url: String, result: Result<Podcast, String>) {
        let Some(i) = self.library.find(&url) else { return };
        match result {
            Ok(podcast) => {
                let feed = &mut self.library.feeds[i];
                if feed.title.trim().is_empty() && !podcast.title.is_empty() {
                    feed.title = podcast.title.clone();
                }
                if feed.website.is_empty() {
                    feed.website = podcast.website.clone();
                }
                feed.last_fetched = Some(chrono::Utc::now());
                self.fetch_errors.remove(&url);
                self.podcasts.insert(url, podcast);
                self.save_library();
            }
            Err(e) => {
                self.fetch_errors.insert(url, e);
            }
        }
    }

    fn episode_done(&mut self, feed_url: String, episode_id: String, outcome: Outcome) {
        let title = self
            .podcasts
            .get(&feed_url)
            .and_then(|p| p.episodes.iter().find(|e| e.id == episode_id))
            .map(|e| e.title.clone())
            .unwrap_or_else(|| episode_id.clone());
        let Some(job) = &mut self.job else { return };
        let mark = match outcome {
            Outcome::Saved(path) => {
                log::info!("saved {}", path.display());
                job.saved += 1;
                true
            }
            Outcome::AlreadyThere(path) => {
                log::info!("already there: {}", path.display());
                job.already += 1;
                true
            }
            Outcome::Failed(e) => {
                job.failed.push(format!("{title}: {e}"));
                false
            }
        };
        if mark && let Some(i) = self.library.find(&feed_url) {
            self.library.feeds[i].downloaded.insert(episode_id.clone());
            if let Some(ticked) = self.ticked.get_mut(&feed_url) {
                ticked.remove(&episode_id);
            }
            self.save_library();
        }
    }

    fn downloads_done(&mut self, cancelled: bool) {
        let Some(job) = self.job.take() else { return };
        let mut text = match job.saved {
            1 => "Downloaded 1 episode.".to_owned(),
            n => format!("Downloaded {n} episodes."),
        };
        if job.already > 0 {
            text.push_str(&format!(" {} were already in the folder.", job.already));
        }
        if cancelled {
            text = format!("Cancelled. {text}");
        }
        if job.failed.is_empty() {
            self.announce(text);
        } else {
            const SHOWN: usize = 10;
            text.push_str(&format!(" {} could not be downloaded:\n", job.failed.len()));
            for line in job.failed.iter().take(SHOWN) {
                text.push_str(&format!("\n{line}"));
            }
            if job.failed.len() > SHOWN {
                text.push_str(&format!("\n…and {} more.", job.failed.len() - SHOWN));
            }
            self.show_error(text);
        }
    }

    /// Says how far along the current job is (F7).
    fn announce_progress(&mut self) {
        let percent = (self.progress * 100.0).round();
        let text = match &self.job {
            Some(job) if job.kind == JobKind::Fetching => format!("Fetching feeds, {percent}% done."),
            Some(job) => format!("Downloading {} of {}, {percent}% done overall. {}", job.current, job.total, job.title),
            None => "Nothing is fetching or downloading.".to_owned(),
        };
        self.announce(text);
    }

    // ----- keyboard ------------------------------------------------------

    fn take_shortcuts(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let focused = ctx.memory(|m| m.focused());
        let cx = KeyContext {
            // Read from memory: this runs between frames, when egui's
            // per-frame record of open popups isn't kept.
            popup_open: egui::Popup::is_any_open(ctx),
            focused_tab: focused.and_then(|id| self.tab_buttons.iter().position(|b| *b == id)),
            tab_count: Tab::ALL.len(),
            busy: self.is_busy(),
        };
        for command in self.keymap.take(&mut raw_input.events, cx) {
            match command {
                Command::CloseList => egui::Popup::close_all(ctx),
                _ => self.pending.push(command),
            }
        }
    }

    fn run_shortcuts(&mut self, ctx: &egui::Context) {
        for command in std::mem::take(&mut self.pending) {
            match command {
                Command::Tab(i) => self.switch_tab(Tab::ALL[i.min(Tab::ALL.len() - 1)]),
                Command::Run(action) => self.run_action(ctx, action),
                Command::CloseList => {}
            }
        }
    }

    fn run_action(&mut self, ctx: &egui::Context, action: Action) {
        // A shortcut for another tab's control goes there first.
        let place_tab = match action.place() {
            shortcuts::Place::Anywhere => None,
            shortcuts::Place::Feeds => Some(Tab::Feeds),
            shortcuts::Place::Episodes => Some(Tab::Episodes),
            shortcuts::Place::Settings => Some(Tab::Settings),
        };
        if let Some(tab) = place_tab
            && tab != self.tab
        {
            self.show_tab(tab);
        }
        egui::Popup::close_all(ctx);
        let count = Tab::ALL.len();
        let current = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        match action {
            Action::GoToTab => {}
            Action::NextTab => self.switch_tab(Tab::ALL[(current + 1) % count]),
            Action::PreviousTab => self.switch_tab(Tab::ALL[(current + count - 1) % count]),
            Action::Help => self.show_shortcuts(),
            Action::Progress => self.announce_progress(),
            Action::RepeatStatus => {
                let status = self.status.trim_end_matches('\u{a0}').to_owned();
                self.announce(status);
            }
            Action::Cancel => self.cancel(),
            Action::ImportOpml => self.import_opml(),
            Action::ExportOpml => self.export_opml(),
            Action::FeedAddress => self.focus = Some(Target::FeedAddress),
            Action::FeedList => self.focus = Some(Target::FeedList),
            Action::RemoveFeed => self.remove_feed(),
            Action::RefreshFeed => self.fetch_selected(),
            Action::RefreshAll => self.fetch_all(),
            Action::PodcastPicker => self.focus = Some(Target::PodcastPicker),
            Action::Filter => self.focus = Some(Target::Filter),
            Action::EpisodeList => self.focus = Some(Target::EpisodeList),
            Action::SelectAll => self.tick_shown(),
            Action::SelectNone => self.untick_all(),
            Action::SelectNew => self.tick_new(),
            Action::DownloadSelected => self.download_ticked(),
            Action::DownloadAll => self.download_all(),
            Action::FolderField => self.focus = Some(Target::FolderField),
            Action::Browse => self.browse_folder(),
            Action::OpenFolder => self.open_download_folder(),
            Action::Naming => self.focus = Some(Target::Naming),
            Action::FolderPerPodcast => {
                self.settings.folder_per_podcast = !self.settings.folder_per_podcast;
                self.settings.save();
                self.focus = Some(Target::FolderPerPodcast);
                self.announce(if self.settings.folder_per_podcast {
                    "Each podcast gets its own folder."
                } else {
                    "Episodes go straight into the download folder."
                });
            }
            Action::DataFolder => self.open_data_folder(),
        }
    }

    fn show_shortcuts(&mut self) {
        let names: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
        rfd::MessageDialog::new()
            .set_title("Keyboard shortcuts")
            .set_description(self.keymap.reference(&names))
            .set_buttons(rfd::MessageButtons::Ok)
            .set_level(rfd::MessageLevel::Info)
            .show();
    }

    /// Goes to `tab` and puts focus on its tab button.
    fn switch_tab(&mut self, tab: Tab) {
        self.show_tab(tab);
        self.focus = Some(Target::Tab(tab));
    }

    /// Shows `tab`, leaving focus to whoever asked.
    fn show_tab(&mut self, tab: Tab) {
        self.tab = tab;
        if tab == Tab::Episodes {
            self.fetch_if_needed();
        }
    }

    /// Moves focus to `resp` if a shortcut asked for `target`.
    fn focus_if_wanted(&mut self, target: Target, resp: &egui::Response) {
        if self.focus == Some(target) {
            resp.request_focus();
            resp.scroll_to_me(None);
            self.focus = None;
        }
    }

    // ----- layout --------------------------------------------------------

    fn tab_bar(&mut self, ui: &mut Ui) {
        self.tab_buttons.clear();
        ui.columns(Tab::ALL.len(), |cols| {
            for (i, (col, tab)) in cols.iter_mut().zip(Tab::ALL).enumerate() {
                let selected = self.tab == tab;
                let text = RichText::new(format!("{}{}", tab.label(), self.keymap.tab_hint(i))).family(tab_font()).size(18.0);
                let text = if selected { text.strong() } else { text };
                let resp = col
                    .push_id(tab.id(), |ui| {
                        ui.add_sized([ui.available_width(), 40.0], Button::selectable(selected, text))
                    })
                    .inner;
                self.tab_buttons.push(resp.id);
                col.ctx().accesskit_node_builder(resp.id, |node| {
                    node.set_role(Role::Tab);
                    node.set_selected(selected);
                });
                self.focus_if_wanted(Target::Tab(tab), &resp);
                if resp.clicked() && !selected {
                    self.switch_tab(tab);
                }
            }
        });
    }

    /// A full-width progress bar. Screen readers get it as a progress
    /// indicator named "Progress", with a percentage value.
    fn progress_bar(&self, ui: &mut Ui) {
        let percent = (self.progress * 100.0).round();
        let (fraction, text) = match &self.job {
            Some(job) if job.kind == JobKind::Fetching => (self.progress, format!("Fetching feeds: {percent}%")),
            Some(job) => (self.progress, format!("Downloading {} of {}: {percent}%", job.current.max(1), job.total)),
            None => (0.0, "Nothing running".to_owned()),
        };
        let bar = egui::ProgressBar::new(fraction).desired_width(ui.available_width()).desired_height(24.0);
        let resp = ui.add(bar);
        // egui draws a bar's own text at the left edge, so the text is drawn
        // here instead, centred.
        let galley = egui::WidgetText::from(text.as_str()).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            resp.rect.width(),
            egui::TextStyle::Button,
        );
        let colour = ui.visuals().override_text_color.unwrap_or(ui.visuals().selection.stroke.color);
        let pos = resp.rect.center() - galley.size() / 2.0;
        ui.painter().with_clip_rect(resp.rect).galley(pos, galley, colour);
        ui.ctx().accesskit_node_builder(resp.id, |node| {
            node.set_label("Progress");
            node.set_value(text);
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(100.0);
            node.set_numeric_value(f64::from(percent));
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        if self.is_busy() {
            if full_button(ui, &format!("Cancel{}", self.keymap.hint(Action::Cancel)), true).clicked() {
                self.cancel();
            }
            ui.add_space(4.0);
        }
        let resp = ui.add(egui::Label::new(RichText::new(&self.status).size(16.0)).wrap());
        ui.ctx().accesskit_node_builder(resp.id, |node| {
            node.set_live(Live::Polite);
        });
    }
}

impl eframe::App for PodApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.take_shortcuts(ctx, raw_input);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_messages();
        self.run_shortcuts(&ctx);

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.add_space(4.0);
            self.tab_bar(ui);
            ui.add_space(4.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(6.0);
            self.progress_bar(ui);
            ui.add_space(4.0);
            self.status_bar(ui);
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                match self.tab {
                    Tab::Feeds => self.feeds_tab(ui),
                    Tab::Episodes => self.episodes_tab(ui),
                    Tab::Settings => self.settings_tab(ui),
                    Tab::Shortcuts => self.shortcuts_tab(ui),
                }
            });
        });

        // A shortcut asked for a control this tab isn't showing just now,
        // such as the filter before the podcast has been fetched.
        if let Some(target) = self.focus.take_if(|t| !matches!(t, Target::Tab(_))) {
            log::info!("{target:?} is not shown");
            self.announce("That control isn't available until a podcast has been fetched.");
        }

        draw_focus_ring(&ctx);
    }
}
