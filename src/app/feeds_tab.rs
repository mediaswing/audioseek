//! The Feeds tab: importing and exporting OPML files, adding and removing
//! feeds, and fetching them.

use super::*;
use crate::library::Feed;

impl PodApp {
    pub(super) fn import_opml(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import an OPML file")
            .add_filter("OPML files", &["opml", "xml"])
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return;
        };
        let feeds = std::fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| crate::opml::parse(&crate::text::decode(bytes)));
        let feeds = match feeds {
            Ok(feeds) => feeds,
            Err(e) => {
                self.show_error(format!("Could not import {}: {e:#}", path.display()));
                return;
            }
        };
        if feeds.is_empty() {
            self.show_error(format!("{} lists no podcast feeds.", path.display()));
            return;
        }
        let found = feeds.len();
        let new_urls = self.library.add_all(feeds);
        self.save_library();
        let mut text = format!("Imported {} from {found} in the file.", plural(new_urls.len(), "new feed"));
        if !new_urls.is_empty() && !self.is_busy() {
            text.push_str(" Fetching them now.");
        }
        self.announce(text);
        self.focus = Some(Target::FeedList);
        if !self.is_busy() {
            self.fetch(new_urls);
        }
    }

    pub(super) fn export_opml(&mut self) {
        if self.library.feeds.is_empty() {
            self.announce("There are no feeds to export.");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export the feeds as an OPML file")
            .set_file_name("podcasts.opml")
            .add_filter("OPML files", &["opml"])
            .save_file()
        else {
            return;
        };
        match crate::paths::write_atomic(&path, crate::opml::export(&self.library.feeds).as_bytes()) {
            Ok(()) => self.announce(format!("Exported {} to {}.", plural(self.library.feeds.len(), "feed"), path.display())),
            Err(e) => self.show_error(format!("Could not save {}: {e}", path.display())),
        }
    }

    fn add_feed(&mut self) {
        let url = self.feed_address.trim().to_owned();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            self.show_error("Type the feed's full address, starting with https:// or http://.");
            self.focus = Some(Target::FeedAddress);
            return;
        }
        if let Some(i) = self.library.find(&url) {
            self.selected = i;
            self.announce(format!("{} is already in the list.", self.library.feeds[i].name()));
            return;
        }
        self.library.add_all(vec![Feed::new(url.clone(), String::new(), String::new())]);
        self.selected = self.library.feeds.len() - 1;
        self.save_library();
        self.feed_address.clear();
        self.focus = Some(Target::FeedAddress);
        self.announce("Feed added.");
        self.fetch(vec![url]);
    }

    pub(super) fn remove_feed(&mut self) {
        let Some(feed) = self.library.feeds.get(self.selected) else {
            self.announce("There is no feed to remove.");
            return;
        };
        if self.is_busy() {
            self.refuse_if_busy();
            return;
        }
        let name = feed.name().to_owned();
        if !self.confirm(&format!("Remove {name} from the list? Episodes already downloaded are kept.")) {
            return;
        }
        let feed = self.library.feeds.remove(self.selected);
        self.podcasts.remove(&feed.url);
        self.ticked.remove(&feed.url);
        self.selected = self.selected.min(self.library.feeds.len().saturating_sub(1));
        self.save_library();
        self.announce(format!("Removed {name}."));
        self.focus = Some(Target::FeedList);
    }

    pub(super) fn fetch_selected(&mut self) {
        match self.library.feeds.get(self.selected) {
            Some(feed) => {
                let url = feed.url.clone();
                self.fetch(vec![url]);
            }
            None => self.announce("There is no feed to fetch."),
        }
    }

    pub(super) fn fetch_all(&mut self) {
        let urls: Vec<String> = self.library.feeds.iter().filter(|f| !f.disabled).map(|f| f.url.clone()).collect();
        if self.library.feeds.is_empty() {
            self.announce("There are no feeds to fetch.");
        } else if urls.is_empty() {
            self.announce("Every feed is disabled in feeds.json, so there are none to fetch.");
        }
        self.fetch(urls);
    }

    /// Fetches the chosen feed if it hasn't been this session, so its
    /// episodes can be listed.
    pub(super) fn fetch_if_needed(&mut self) {
        if let Some(feed) = self.library.feeds.get(self.selected)
            && !feed.disabled
            && !self.podcasts.contains_key(&feed.url)
            && !self.fetch_errors.contains_key(&feed.url)
            && !self.is_busy()
        {
            let url = feed.url.clone();
            self.fetch(vec![url]);
        }
    }

    /// One feed's line in the list.
    fn feed_line(&self, feed: &Feed) -> String {
        let mut line = feed.name().to_owned();
        if feed.disabled {
            line.push_str(", disabled");
        }
        if let Some(podcast) = self.podcasts.get(&feed.url) {
            line.push_str(&format!(", {}", plural(podcast.episodes.len(), "episode")));
        }
        if !feed.downloaded.is_empty() {
            line.push_str(&format!(", {} downloaded", feed.downloaded.len()));
        }
        line.push_str(&format!(", {}", feed.fetched_text()));
        if let Some(e) = self.fetch_errors.get(&feed.url) {
            line.push_str(&format!(". Could not fetch: {e}"));
        }
        line
    }

    pub(super) fn feeds_tab(&mut self, ui: &mut Ui) {
        heading(ui, "Feeds");
        let busy = self.is_busy();
        if full_button(ui, &format!("Import an OPML file…{}", self.keymap.hint(Action::ImportOpml)), true).clicked() {
            self.import_opml();
        }
        let has_feeds = !self.library.feeds.is_empty();
        if full_button(ui, &format!("Export the feeds as OPML…{}", self.keymap.hint(Action::ExportOpml)), has_feeds)
            .clicked()
        {
            self.export_opml();
        }

        let label = format!("Add a feed by its address{}, then press Enter", self.keymap.hint(Action::FeedAddress));
        let id = field_id(Target::FeedAddress);
        let resp = text_field(ui, id, &label, &mut self.feed_address, true);
        self.focus_if_wanted(Target::FeedAddress, &resp);
        let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let add_enabled = !self.feed_address.trim().is_empty();
        let clicked = full_button(ui, "Add feed", add_enabled).clicked();
        if add_enabled && entered || clicked {
            self.add_feed();
        }

        let selected_name = self.library.feeds.get(self.selected).map(|f| f.name().to_owned());
        let enabled = selected_name.is_some() && !busy;
        if full_button(ui, &format!("Fetch the selected feed{}", self.keymap.hint(Action::RefreshFeed)), enabled).clicked() {
            self.fetch_selected();
        }
        if full_button(ui, &format!("Fetch every feed{}", self.keymap.hint(Action::RefreshAll)), has_feeds && !busy).clicked() {
            self.fetch_all();
        }
        if full_button(ui, &format!("Remove the selected feed{}", self.keymap.hint(Action::RemoveFeed)), enabled).clicked() {
            self.remove_feed();
        }

        ui.add_space(6.0);
        heading(ui, &format!("Feed list{}", self.keymap.hint(Action::FeedList)));
        if !has_feeds {
            let resp = ui.label("No feeds yet. Import an OPML file or add a feed by its address.");
            if self.focus == Some(Target::FeedList) {
                self.focus = None;
                self.announce("The feed list is empty.");
                resp.scroll_to_me(None);
            }
            return;
        }
        ui.label("Arrow keys move through the list. Enter shows a feed's episodes.");
        let mut open = None;
        for i in 0..self.library.feeds.len() {
            let line = self.feed_line(&self.library.feeds[i]);
            let selected = i == self.selected;
            let resp = ui
                .push_id(("feed", i), |ui| {
                    let size = egui::vec2(ui.available_width(), CONTROL_HEIGHT);
                    ui.add(Button::selectable(selected, line).wrap().min_size(size))
                })
                .inner;
            if selected {
                self.focus_if_wanted(Target::FeedList, &resp);
            }
            // Selection follows focus, so the keys for the selected feed work
            // on the one being read out.
            if resp.gained_focus() {
                self.selected = i;
            }
            if resp.clicked() {
                self.selected = i;
                open = Some(i);
            }
        }
        if open.is_some() {
            self.show_tab(Tab::Episodes);
            self.focus = Some(Target::EpisodeList);
        }
    }
}
