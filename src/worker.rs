//! Work done away from the window: fetching feeds and downloading episodes.
//! Results come back to the window as messages.

use crate::feed::{Episode, Podcast};
use crate::library::{Feed, Library};
use crate::settings::{Naming, Settings};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// The least time between progress messages while an episode downloads.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

pub enum Msg {
    /// A feed has been fetched and read, or couldn't be.
    FeedFetched(String, Result<Podcast, String>),
    /// Every feed asked for has been tried.
    RefreshDone {
        fetched: usize,
        failed: usize,
        cancelled: bool,
    },
    /// An episode is starting to download: its number, counting from 1, and
    /// its title.
    Downloading(usize, String),
    /// An episode has been saved, was already there, or failed.
    EpisodeDone {
        feed_url: String,
        episode_id: String,
        outcome: Outcome,
    },
    DownloadsDone {
        cancelled: bool,
    },
    Progress(f32),
    /// The update check has finished: the newer version, if there is one.
    /// `asked` says whether the user asked, rather than it being the check
    /// made at startup.
    UpdateChecked {
        result: Result<Option<String>, String>,
        asked: bool,
    },
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

/// How many feeds are fetched at once. Enough that one slow server doesn't
/// hold up the rest, and few enough to be polite to hosts with many feeds.
const FETCHERS: usize = 6;

/// Fetches the feeds, a few at once, in the order given. Each is reported
/// as it finishes, so not necessarily in that order.
pub fn refresh(rep: Reporter, urls: Vec<String>, cancel: Arc<AtomicBool>) {
    let count = urls.len();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let fetched = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..FETCHERS.min(count) {
            let (rep, urls, cancel, next, done, fetched) = (rep.clone(), &urls, &cancel, &next, &done, &fetched);
            let fetcher = move || {
                while !cancel.load(Ordering::Relaxed) {
                    let Some(url) = urls.get(next.fetch_add(1, Ordering::Relaxed)) else { break };
                    let result = crate::net::fetch_text(url).and_then(|text| crate::feed::parse(&text)).map_err(|e| {
                        log::warn!("{url}: {e:#}");
                        format!("{e:#}")
                    });
                    if result.is_ok() {
                        fetched.fetch_add(1, Ordering::Relaxed);
                    }
                    let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                    rep.send(Msg::FeedFetched(url.clone(), result));
                    rep.send(Msg::Progress(finished as f32 / count as f32));
                }
            };
            // A thread that can't start crashes the job, which says so.
            std::thread::Builder::new()
                .name(Job::Fetch.thread_name().to_owned())
                .spawn_scoped(scope, fetcher)
                .expect("could not start a thread to fetch feeds");
        }
    });
    let (done, fetched) = (done.into_inner(), fetched.into_inner());
    rep.send(Msg::RefreshDone { fetched, failed: done - fetched, cancelled: done < count });
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
    /// From `naming::file_names`, so it is no other episode's, in this
    /// podcast or another sharing the folder.
    pub file_name: String,
}

/// The downloads for `episodes` of `feed`, whose fetched episodes are
/// `podcast`'s, each with a file name no other episode in the library has
/// in the folder it goes to. See `naming::file_names`.
pub fn downloads(
    library: &Library,
    feed: &Feed,
    podcast: &Podcast,
    episodes: Vec<Episode>,
    naming: Naming,
    target: &Target,
) -> Vec<Download> {
    let podcast_title = feed.name().to_owned();
    let reserved = library.reserved_names(&feed.url, &target.dir_for(&podcast_title));
    let mut names = crate::naming::file_names(&podcast.episodes, naming, &reserved);
    episodes
        .into_iter()
        .map(|episode| Download {
            feed_url: feed.url.clone(),
            podcast_title: podcast_title.clone(),
            file_name: names.remove(&episode.id).unwrap_or_else(|| crate::naming::file_name(&episode, naming)),
            episode,
        })
        .collect()
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
    /// The folder a podcast's episodes go in.
    pub fn dir_for(&self, podcast_title: &str) -> PathBuf {
        let mut dir = self.dir.clone();
        if self.folder_per_podcast {
            dir.push(crate::naming::folder_name(podcast_title));
        }
        dir
    }

    fn path(&self, item: &Download) -> PathBuf {
        self.dir_for(&item.podcast_title).join(&item.file_name)
    }
}

/// Downloads each episode in turn. One that is already in the folder under
/// its name is left alone: the name is no other episode's (see `Download`),
/// so the file is taken to be this one's.
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
            let mut last_sent = Instant::now();
            let result =
                path.parent().map(std::fs::create_dir_all).transpose().map_err(anyhow::Error::from).and_then(|_| {
                    crate::net::download(&item.episode.url, &path, &cancel, |got, total| {
                        let Some(total) = total.filter(|t| *t > 0) else { return };
                        let part = (got as f64 / total as f64).min(1.0);
                        let overall = (i as f64 + part) / count as f64;
                        // Only as often as the bar visibly moves, and at most
                        // ten times a second, as each redraws the window.
                        let percent = (overall * 1000.0) as i32;
                        if percent != last_percent && last_sent.elapsed() >= PROGRESS_INTERVAL {
                            last_percent = percent;
                            last_sent = Instant::now();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::test_server::{ok, response, serve, serve_paths};
    use std::sync::mpsc::channel;

    fn episode(id: &str, title: &str, date: &str, url: &str) -> Episode {
        Episode {
            id: id.to_owned(),
            title: title.to_owned(),
            published: chrono::DateTime::parse_from_rfc3339(date).ok(),
            url: url.to_owned(),
            mime: String::new(),
            duration: String::new(),
        }
    }

    fn podcast(title: &str, episodes: Vec<Episode>) -> Podcast {
        Podcast { title: title.to_owned(), website: String::new(), episodes }
    }

    #[test]
    fn podcasts_sharing_a_folder_get_names_of_their_own() {
        let dir = tempfile::tempdir().unwrap();
        let target = Target { dir: dir.path().to_owned(), folder_per_podcast: false };
        let mut a = Feed::new("https://a.example/feed".into(), "News A".into(), String::new());
        let b = Feed::new("https://b.example/feed".into(), "News B".into(), String::new());
        // Both bulletins come out on the hour.
        let hour = "2026-10-23T09:00:00+00:00";
        a.mark_downloaded(
            "a1",
            &dir.path().join(crate::naming::file_name(&episode("a1", "", hour, "https://a/1.mp3"), Naming::Timestamp)),
        );
        let library = Library { feeds: vec![a, b.clone()], ..Library::default() };
        let b1 = episode("b1", "", hour, "https://b/1.mp3");
        let items = downloads(&library, &b, &podcast("News B", vec![b1.clone()]), vec![b1], Naming::Timestamp, &target);
        assert_eq!(items.len(), 1);
        assert!(items[0].file_name.ends_with(" (2).mp3"), "{}", items[0].file_name);
        assert_eq!(items[0].podcast_title, "News B");
    }

    #[test]
    fn downloads_report_each_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let target = Target { dir: dir.path().to_owned(), folder_per_podcast: true };
        let server = serve(vec![ok(b"new episode"), response("404 Not Found", &[], b"")]);
        let feed = Feed::new("https://show.example/feed".into(), "Show".into(), String::new());
        let date = "2026-10-23T09:00:00+00:00";
        let there = episode("1", "There already", date, &format!("{server}/never-asked.mp3"));
        let new = episode("2", "New", "2026-10-24T09:00:00+00:00", &format!("{server}/new.mp3"));
        let gone = episode("3", "Gone", "2026-10-25T09:00:00+00:00", &format!("{server}/gone.mp3"));
        let show = podcast("Show", vec![gone.clone(), new.clone(), there.clone()]);
        let library = Library { feeds: vec![feed.clone()], ..Library::default() };
        let items = downloads(&library, &feed, &show, vec![there, new, gone], Naming::TimestampTitle, &target);
        let folder = dir.path().join("Show");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(&items[0].file_name), b"old").unwrap();

        let (tx, rx) = channel();
        download(Reporter::new(tx, egui::Context::default()), items, target, Arc::new(AtomicBool::new(false)));
        let mut outcomes = Vec::new();
        let mut finished = None;
        for msg in rx.try_iter() {
            match msg {
                Msg::EpisodeDone { episode_id, outcome, .. } => outcomes.push((episode_id, outcome)),
                Msg::DownloadsDone { cancelled } => finished = Some(cancelled),
                _ => {}
            }
        }
        assert_eq!(finished, Some(false));
        assert_eq!(outcomes.len(), 3);
        assert!(matches!(&outcomes[0], (id, Outcome::AlreadyThere(_)) if id == "1"));
        match &outcomes[1] {
            (id, Outcome::Saved(path)) if id == "2" => assert_eq!(std::fs::read(path).unwrap(), b"new episode"),
            _ => panic!("the new episode wasn't saved"),
        }
        assert!(matches!(&outcomes[2], (id, Outcome::Failed(_)) if id == "3"));
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2, "nothing saved for the failed one");
    }

    /// Every message `work` sends, once it has finished.
    fn messages(work: impl FnOnce(Reporter)) -> Vec<Msg> {
        let (tx, rx) = channel();
        work(Reporter::new(tx, egui::Context::default()));
        rx.try_iter().collect()
    }

    #[test]
    fn refreshing_fetches_every_feed() {
        let rss = |title: &str| {
            let xml = format!(
                r#"<rss><channel><title>{title}</title><item><title>One</title><enclosure url="https://x/1.mp3"/></item></channel></rss>"#
            );
            ok(xml.as_bytes())
        };
        // More feeds than fetchers, two of them missing.
        let routes: Vec<(String, Vec<u8>)> =
            (0..10).filter(|i| i % 5 != 4).map(|i| (format!("/{i}"), rss(&format!("Show {i}")))).collect();
        let server = serve_paths(routes.iter().map(|(p, r)| (p.as_str(), r.clone())).collect());
        let urls: Vec<String> = (0..10).map(|i| format!("{server}/{i}")).collect();
        let msgs = messages(|rep| refresh(rep, urls.clone(), Arc::new(AtomicBool::new(false))));

        let mut results: Vec<(String, bool)> = msgs
            .iter()
            .filter_map(|m| match m {
                Msg::FeedFetched(url, result) => Some((url.clone(), result.is_ok())),
                _ => None,
            })
            .collect();
        results.sort();
        let mut expected: Vec<(String, bool)> = (0..10).map(|i| (format!("{server}/{i}"), i % 5 != 4)).collect();
        expected.sort();
        assert_eq!(results, expected, "each feed once, the missing ones failed");
        assert!(matches!(msgs.last(), Some(Msg::RefreshDone { fetched: 8, failed: 2, cancelled: false })));
        let progress: Vec<f32> =
            msgs.iter().filter_map(|m| if let Msg::Progress(p) = m { Some(*p) } else { None }).collect();
        assert_eq!(progress.len(), 10);
        assert_eq!(progress.iter().cloned().fold(0.0, f32::max), 1.0);
    }

    #[test]
    fn a_cancelled_refresh_says_so() {
        let msgs =
            messages(|rep| refresh(rep, vec!["http://127.0.0.1:9/feed".into()], Arc::new(AtomicBool::new(true))));
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], Msg::RefreshDone { fetched: 0, failed: 0, cancelled: true }));
    }

    #[test]
    fn refreshing_nothing_finishes() {
        let msgs = messages(|rep| refresh(rep, Vec::new(), Arc::new(AtomicBool::new(false))));
        assert!(matches!(msgs[..], [Msg::RefreshDone { fetched: 0, failed: 0, cancelled: false }]));
    }

    #[test]
    fn a_crashing_job_says_so() {
        let (tx, rx) = channel();
        let rep = Reporter::new(tx, egui::Context::default());
        rep.spawn(Job::Fetch, |_| panic!("on purpose"));
        let msg = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(matches!(msg, Msg::Crashed(Job::Fetch)));
    }
}
