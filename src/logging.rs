//! The log file, `audioseek.log` in the app's config folder. Release builds
//! on Windows have no console, so this is the only place warnings, errors and
//! crashes are kept.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

/// When the log grows past this, it is moved to `audioseek.old.log` the next
/// time the app starts, so the two together never take much room.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

struct Logger {
    file: Mutex<File>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // The app's own messages, and only problems from the libraries,
        // which are chatty.
        if metadata.target().starts_with(env!("CARGO_CRATE_NAME")) {
            metadata.level() <= log::Level::Info
        } else {
            metadata.level() <= log::Level::Warn
        }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}: {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.target(),
            record.args()
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

/// Starts writing the log, and logs any crash before it happens. If the log
/// can't be opened the app runs without one.
pub fn init() {
    let path = crate::paths::log_file();
    let opened = (|| {
        std::fs::create_dir_all(path.parent()?).ok()?;
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
            let _ = std::fs::rename(&path, crate::paths::old_log_file());
        }
        OpenOptions::new().create(true).append(true).open(&path).ok()
    })();
    let Some(file) = opened else {
        eprintln!("Could not open the log file {}", path.display());
        return;
    };
    if log::set_boxed_logger(Box::new(Logger { file: Mutex::new(file) })).is_err() {
        return;
    }
    log::set_max_level(log::LevelFilter::Info);

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        log::error!("crashed in thread {}: {info}", thread.name().unwrap_or("unnamed"));
        default_hook(info);
    }));

    log::info!("audioseek {} started on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
}
