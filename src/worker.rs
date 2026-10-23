//! Work done away from the window: fetching feeds and downloading episodes.
//! Results come back to the window as messages.

use crate::feed::{Episode, Podcast};
use crate::settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

pub enum Msg {
    /// A feed has been fetched and read, or couldn't be.
    FeedFetched(String, Result<Podcast, String>),
    /// Every feed asked for has been tried.
    RefreshDone { fetched: usize, failed: usize, cancelled: bool },
    /// An episode is starting to download: its number, counting from 1, and
    /// its title.
    Downloading(usize, String),
    /// An episode has been saved, was already there, or failed.
    EpisodeDone { feed_url: String, episode_id: String, outcome: Outcome },
    DownloadsDone { cancelled: bool },
    Progress(f32),
    /// The update check has finished: the newer version, if there is one.
    /// `asked` says whether the user asked, rather than it being the check
    /// made at startup.
    UpdateChecked { result: Result<Option<String>, String>, asked: bool },
    /// A job stopped part way through with a crash (logged with the
    /// details), so it will send nothing more.
    Crashed(Job),
}

/// The kinds of work done away from the window, which are also the names
/// of their threads, as the log gives them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Job {
    Fetch,
    Download,
    UpdateCheck,
}

impl Job {
    fn thread_name(self) -> &'static str {
        match self {
            Job::Fetch => "fetch",
            Job::Download => "download",
            Job::UpdateCheck => "update check",
        }
    }
}

pub enum Outcome {
    Saved(PathBuf),
    /// A file with its name is already in the folder.
    AlreadyThere(PathBuf),
    Failed(String),
}

#[derive(Clone)]
pub struct Reporter {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Reporter {
    pub fn new(tx: Sender<Msg>, ctx: egui::Context) -> Self {
        Self { tx, ctx }
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
        self.ctx.request_repaint();
    }

    /// Runs `work` on a thread of its own. If it crashes, the window is
    /// sent `Msg::Crashed`, so it doesn't wait for the job for ever.
    pub fn spawn(&self, job: Job, work: impl FnOnce(Reporter) + Send + 'static) {
        let rep = self.clone();
        let spawned = std::thread::Builder::new().name(job.thread_name().to_owned()).spawn(move || {
            let worker = rep.clone();
            // The panic hook in `logging.rs` has already logged what happened.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(worker))).is_err() {
                rep.send(Msg::Crashed(job));
            }
        });
        if let Err(e) = spawned {
            log::error!("could not start the {} thread: {e}", job.thread_name());
            self.send(Msg::Crashed(job));
        }
    }
}

/// Fetches each feed in turn.
pub fn refresh(rep: Reporter, urls: Vec<String>, cancel: Arc<AtomicBool>) {
    let (mut fetched, mut failed) = (0, 0);
    let count = urls.len().max(1);
    for (i, url) in urls.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            rep.send(Msg::RefreshDone { fetched, failed, cancelled: true });
            return;
        }
        let result = crate::net::fetch_text(&url)
            .and_then(|text| crate::feed::parse(&text))
            .map_err(|e| {
                log::warn!("{url}: {e:#}");
                format!("{e:#}")
            });
        if result.is_ok() { fetched += 1 } else { failed += 1 }
        rep.send(Msg::FeedFetched(url, result));
        rep.send(Msg::Progress((i + 1) as f32 / count as f32));
    }
    rep.send(Msg::RefreshDone { fetched, failed, cancelled: false });
}

/// Asks GitHub whether there is a newer version.
pub fn check_for_update(rep: Reporter, asked: bool) {
    let result = crate::update::check().map_err(|e| {
        log::warn!("update check failed: {e:#}");
        format!("{e:#}")
    });
    if let Ok(Some(version)) = &result {
        log::info!("version {version} is available");
    }
    rep.send(Msg::UpdateChecked { result, asked });
}

/// One episode to download, and where it goes.
pub struct Download {
    pub feed_url: String,
    pub podcast_title: String,
    pub episode: Episode,
    /// From `naming::file_names`, so it is unique among the podcast's
    /// episodes.
    pub file_name: String,
}

/// Where downloads go, fixed when the batch starts.
#[derive(Clone)]
pub struct Target {
    pub dir: PathBuf,
    pub folder_per_podcast: bool,
}

impl From<&Settings> for Target {
    fn from(s: &Settings) -> Self {
        Target { dir: s.download_dir.clone(), folder_per_podcast: s.folder_per_podcast }
    }
}

impl Target {
    fn path(&self, item: &Download) -> PathBuf {
        let mut dir = self.dir.clone();
        if self.folder_per_podcast {
            dir.push(crate::naming::folder_name(&item.podcast_title));
        }
        dir.join(&item.file_name)
    }
}

/// Downloads each episode in turn. One that is already in the folder under
/// the same name is left alone.
pub fn download(rep: Reporter, items: Vec<Download>, target: Target, cancel: Arc<AtomicBool>) {
    let count = items.len().max(1);
    for (i, item) in items.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            rep.send(Msg::DownloadsDone { cancelled: true });
            return;
        }
        rep.send(Msg::Downloading(i + 1, item.episode.title.clone()));
        rep.send(Msg::Progress(i as f32 / count as f32));
        let path = target.path(&item);
        let outcome = if path.exists() {
            Outcome::AlreadyThere(path)
        } else {
            let mut last_percent = -1i32;
            let result = path
                .parent()
                .map(std::fs::create_dir_all)
                .transpose()
                .map_err(anyhow::Error::from)
                .and_then(|_| {
                    crate::net::download(&item.episode.url, &path, &cancel, |got, total| {
                        let Some(total) = total.filter(|t| *t > 0) else { return };
                        let part = (got as f64 / total as f64).min(1.0);
                        let overall = (i as f64 + part) / count as f64;
                        // Only as often as the bar visibly moves.
                        let percent = (overall * 1000.0) as i32;
                        if percent != last_percent {
                            last_percent = percent;
                            rep.send(Msg::Progress(overall as f32));
                        }
                    })
                });
            match result {
                Ok(()) => Outcome::Saved(path),
                Err(_) if cancel.load(Ordering::Relaxed) => {
                    rep.send(Msg::DownloadsDone { cancelled: true });
                    return;
                }
                Err(e) => {
                    log::warn!("{}: {e:#}", item.episode.url);
                    Outcome::Failed(format!("{e:#}"))
                }
            }
        };
        rep.send(Msg::EpisodeDone { feed_url: item.feed_url, episode_id: item.episode.id, outcome });
    }
    rep.send(Msg::Progress(1.0));
    rep.send(Msg::DownloadsDone { cancelled: false });
}
