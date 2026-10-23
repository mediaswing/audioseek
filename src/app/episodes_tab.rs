//! The Episodes tab: one podcast's episodes, with boxes to tick the ones to
//! download.

use super::*;
use crate::feed::Episode;
use crate::worker::{Download, Target as DownloadTarget};

impl PodApp {
    fn current_podcast(&self) -> Option<(&crate::library::Feed, Option<&Podcast>)> {
        let feed = self.library.feeds.get(self.selected)?;
        Some((feed, self.podcasts.get(&feed.url)))
    }

    /// The episodes whose titles match the filter.
    fn shown_episodes(&self) -> Vec<&Episode> {
        let Some((_, Some(podcast))) = self.current_podcast() else { return Vec::new() };
        let filter = self.filter.trim().to_lowercase();
        podcast.episodes.iter().filter(|e| filter.is_empty() || e.title.to_lowercase().contains(&filter)).collect()
    }

    fn ticked_here(&mut self) -> Option<&mut HashSet<String>> {
        let url = self.library.feeds.get(self.selected)?.url.clone();
        Some(self.ticked.entry(url).or_default())
    }

    fn say_ticked(&mut self) {
        let n = self.ticked_here().map_or(0, |t| t.len());
        self.announce(match n {
            0 => "No episodes ticked.".to_owned(),
            1 => "1 episode ticked.".to_owned(),
            n => format!("{n} episodes ticked."),
        });
    }

    /// Says why there are no episodes to work on, if there aren't.
    fn no_episodes(&mut self) -> bool {
        let reason = match self.current_podcast() {
            None => Some("There are no feeds yet. Import an OPML file first.".to_owned()),
            Some((feed, None)) => Some(format!("{} hasn't been fetched yet{}.", feed.name(), self.keymap.hint(Action::RefreshFeed))),
            Some((_, Some(p))) if p.episodes.is_empty() => Some("This podcast has no episodes to download.".to_owned()),
            _ => None,
        };
        if let Some(reason) = &reason {
            self.announce(reason.clone());
        }
        reason.is_some()
    }

    pub(super) fn tick_shown(&mut self) {
        if self.no_episodes() {
            return;
        }
        let ids: Vec<String> = self.shown_episodes().iter().map(|e| e.id.clone()).collect();
        if let Some(ticked) = self.ticked_here() {
            ticked.extend(ids);
        }
        self.say_ticked();
    }

    pub(super) fn untick_all(&mut self) {
        if let Some(ticked) = self.ticked_here() {
            ticked.clear();
        }
        self.say_ticked();
    }

    /// Ticks the episodes shown that haven't been downloaded.
    pub(super) fn tick_new(&mut self) {
        if self.no_episodes() {
            return;
        }
        let Some((feed, _)) = self.current_podcast() else { return };
        let done = feed.downloaded.clone();
        let ids: Vec<String> =
            self.shown_episodes().iter().filter(|e| !done.contains(&e.id)).map(|e| e.id.clone()).collect();
        if let Some(ticked) = self.ticked_here() {
            ticked.clear();
            ticked.extend(ids);
        }
        self.say_ticked();
    }

    pub(super) fn download_ticked(&mut self) {
        if self.no_episodes() || self.refuse_if_busy() {
            return;
        }
        let Some((feed, Some(podcast))) = self.current_podcast() else { return };
        let ticked = self.ticked.get(&feed.url).cloned().unwrap_or_default();
        let episodes: Vec<Episode> = podcast.episodes.iter().filter(|e| ticked.contains(&e.id)).cloned().collect();
        if episodes.is_empty() {
            let all = match self.keymap.active(Action::SelectAll) {
                Some(s) => format!(", and {} ticks every one shown", s.short(self.keymap.platform())),
                None => String::new(),
            };
            self.announce(format!("No episodes are ticked. Space ticks the one with focus{all}."));
            return;
        }
        self.start_downloads(episodes);
    }

    pub(super) fn download_all(&mut self) {
        if self.no_episodes() || self.refuse_if_busy() {
            return;
        }
        let Some((feed, Some(podcast))) = self.current_podcast() else { return };
        let episodes = podcast.episodes.clone();
        let question = format!(
            "Download all {} episodes of {} to {}? Episodes already in the folder are skipped.",
            episodes.len(),
            feed.name(),
            self.settings.download_dir.display()
        );
        if self.confirm(&question) {
            self.start_downloads(episodes);
        }
    }

    fn start_downloads(&mut self, episodes: Vec<Episode>) {
        let Some(feed) = self.library.feeds.get(self.selected) else { return };
        let feed_url = feed.url.clone();
        let podcast_title = feed.name().to_owned();
        let Some(podcast) = self.podcasts.get(&feed_url) else { return };
        let mut names = crate::naming::file_names(&podcast.episodes, self.settings.naming);
        let items: Vec<Download> = episodes
            .into_iter()
            .map(|episode| Download {
                feed_url: feed_url.clone(),
                podcast_title: podcast_title.clone(),
                file_name: names
                    .remove(&episode.id)
                    .unwrap_or_else(|| crate::naming::file_name(&episode, self.settings.naming)),
                episode,
            })
            .collect();
        let count = items.len();
        let target = DownloadTarget::from(&self.settings);
        let cancel = self.start_job(JobKind::Downloading, count);
        self.announce(format!("Downloading {count} episode{}…", if count == 1 { "" } else { "s" }));
        self.rep.spawn(move |rep| worker::download(rep, items, target, cancel));
    }

    pub(super) fn episodes_tab(&mut self, ui: &mut Ui) {
        heading(ui, "Episodes");
        if self.library.feeds.is_empty() {
            ui.label("There are no feeds yet. Import an OPML file on the Feeds tab first.");
            self.drop_episode_focus();
            return;
        }
        let names: Vec<String> = self.library.feeds.iter().map(|f| f.name().to_owned()).collect();
        let mut chosen = self.selected.min(names.len() - 1);
        let label = format!("Podcast{}", self.keymap.hint(Action::PodcastPicker));
        let (resp, changed) = dropdown(ui, "podcast", &label, &names, &mut chosen, true);
        self.focus_if_wanted(Target::PodcastPicker, &resp);
        if changed {
            self.selected = chosen;
            self.episode_focus = None;
            self.fetch_if_needed();
        }

        let busy = self.is_busy();
        let Some((feed, podcast)) = self.current_podcast() else { return };
        let fetched = feed.fetched_text();
        let downloaded = feed.downloaded.clone();
        let error = self.fetch_errors.get(&feed.url).cloned();
        let Some(podcast) = podcast else {
            let text = match error {
                Some(e) => format!("Could not fetch this podcast: {e}"),
                None if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Fetching) => "Fetching…".to_owned(),
                None => "This podcast hasn't been fetched yet.".to_owned(),
            };
            ui.label(text);
            if full_button(ui, &format!("Fetch this podcast{}", self.keymap.hint(Action::RefreshFeed)), !busy).clicked() {
                self.fetch_selected();
            }
            self.drop_episode_focus();
            return;
        };
        let total = podcast.episodes.len();
        let done_here = podcast.episodes.iter().filter(|e| downloaded.contains(&e.id)).count();
        ui.label(format!("{total} episodes, {done_here} downloaded. This feed was {fetched}."));

        let label = format!("Filter by title{}", self.keymap.hint(Action::Filter));
        let resp = text_field(ui, field_id(Target::Filter), &label, &mut self.filter, true);
        self.focus_if_wanted(Target::Filter, &resp);

        let shown: Vec<Episode> = self.shown_episodes().into_iter().cloned().collect();
        let ticked_count = self.ticked_here().map_or(0, |t| t.len());
        let any = !shown.is_empty();
        if full_button(ui, &format!("Tick every episode shown{}", self.keymap.hint(Action::SelectAll)), any).clicked() {
            self.tick_shown();
        }
        if full_button(ui, &format!("Untick every episode{}", self.keymap.hint(Action::SelectNone)), ticked_count > 0)
            .clicked()
        {
            self.untick_all();
        }
        if full_button(ui, &format!("Tick the episodes not yet downloaded{}", self.keymap.hint(Action::SelectNew)), any)
            .clicked()
        {
            self.tick_new();
        }
        let label = match ticked_count {
            1 => format!("Download the ticked episode{}", self.keymap.hint(Action::DownloadSelected)),
            n => format!("Download the {n} ticked episodes{}", self.keymap.hint(Action::DownloadSelected)),
        };
        if full_button(ui, &label, ticked_count > 0 && !busy).clicked() {
            self.download_ticked();
        }
        let label = format!("Download all {total} episodes{}", self.keymap.hint(Action::DownloadAll));
        if full_button(ui, &label, total > 0 && !busy).clicked() {
            self.download_all();
        }

        ui.add_space(6.0);
        heading(ui, &format!("Episode list{}", self.keymap.hint(Action::EpisodeList)));
        if shown.is_empty() {
            ui.label("No episode titles match the filter.");
            self.drop_episode_focus();
            return;
        }
        if self.shown_count_changed(shown.len()) && !self.filter.trim().is_empty() {
            self.announce(format!("{} episodes match.", shown.len()));
        }
        ui.label("Space ticks or unticks an episode. Arrow keys move through the list.");
        // Where focus goes for the list's shortcut: the row last used, or
        // the first.
        let focus_row = self
            .episode_focus
            .as_ref()
            .and_then(|id| shown.iter().position(|e| &e.id == id))
            .unwrap_or(0);
        let url = self.library.feeds[self.selected].url.clone();
        for (i, episode) in shown.iter().enumerate() {
            let mut on = self.ticked.get(&url).is_some_and(|t| t.contains(&episode.id));
            let text = episode_line(episode, downloaded.contains(&episode.id));
            let resp = ui.push_id(("episode", &episode.id), |ui| ui.checkbox(&mut on, text)).inner;
            if i == focus_row {
                self.focus_if_wanted(Target::EpisodeList, &resp);
            }
            if resp.gained_focus() {
                self.episode_focus = Some(episode.id.clone());
            }
            if resp.changed() {
                let ticked = self.ticked.entry(url.clone()).or_default();
                if on {
                    ticked.insert(episode.id.clone());
                } else {
                    ticked.remove(&episode.id);
                }
            }
        }
    }

    /// The list's shortcut was pressed but there is no list to go to.
    fn drop_episode_focus(&mut self) {
        if self.focus == Some(Target::EpisodeList) {
            self.focus = None;
            self.announce("There are no episodes to list.");
        }
    }

    /// Whether the number of episodes matching the filter differs from last
    /// frame's, so the new number can be announced as the filter is typed.
    fn shown_count_changed(&mut self, count: usize) -> bool {
        let changed = self.last_shown != Some((self.selected, count));
        self.last_shown = Some((self.selected, count));
        changed
    }
}

/// An episode's line in the list: title, when it came out in local time,
/// how long it is, and whether it has been downloaded.
fn episode_line(episode: &Episode, downloaded: bool) -> String {
    let mut line = episode.title.clone();
    if let Some(date) = episode.published {
        line.push_str(&format!(", {}", date.with_timezone(&chrono::Local).format("%-d %b %Y %H:%M")));
    }
    if !episode.duration.is_empty() {
        line.push_str(&format!(", {}", spoken_duration(&episode.duration)));
    }
    if downloaded {
        line.push_str(", downloaded");
    }
    line
}

/// A feed's duration ("3723", "62:03" or "1:02:03") as "1 h 2 min".
fn spoken_duration(text: &str) -> String {
    let parts: Option<Vec<u64>> = text.split(':').map(|p| p.trim().parse::<f64>().ok().map(|n| n as u64)).collect();
    let seconds = match parts.as_deref() {
        Some([s]) => *s,
        Some([m, s]) => m.saturating_mul(60).saturating_add(*s),
        Some([h, m, s]) => h.saturating_mul(3600).saturating_add(m.saturating_mul(60)).saturating_add(*s),
        _ => return text.to_owned(),
    };
    let (h, m) = (seconds / 3600, seconds / 60 % 60);
    match (h, m) {
        (0, 0) => format!("{seconds} s"),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(spoken_duration("3723"), "1 h 2 min");
        assert_eq!(spoken_duration("62:03"), "1 h 2 min");
        assert_eq!(spoken_duration("00:45:10"), "45 min");
        assert_eq!(spoken_duration("40"), "40 s");
        assert_eq!(spoken_duration("about an hour"), "about an hour");
        // Nonsense from a feed must not overflow.
        assert!(spoken_duration("1e30:1e30:1e30").ends_with(" h"));
    }
}
