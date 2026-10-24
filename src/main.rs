//! Podcast Batch Downloader (`audioseek`): downloads podcast episodes from the
//! feeds in an OPML file, all at once or a few at a time.

// Release builds on Windows are GUI programs without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod feed;
mod library;
mod logging;
mod naming;
mod net;
mod opml;
#[cfg(not(target_os = "windows"))]
mod package;
mod paths;
mod settings;
mod shortcuts;
mod sound;
mod text;
mod update;
mod worker;

use std::process::ExitCode;

/// The app's name, as its windows and dialogs give it.
const APP_TITLE: &str = "Podcast Batch Downloader";

const USAGE: &str = "Podcast Batch Downloader

Usage:
  audioseek                     Start the application
  audioseek --version           Print the version
  audioseek --help              Print this help
  audioseek --package-macos DIR Build an installer package (.pkg) of the app in DIR (macOS)
  audioseek --package-deb DIR   Build a .deb package in DIR (Linux)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => run_gui(),
        Some("--version" | "-V") => {
            println!("audioseek {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        #[cfg(target_os = "macos")]
        Some("--package-macos") => package(args.get(1)),
        #[cfg(all(unix, not(target_os = "macos")))]
        Some("--package-deb") => package(args.get(1)),
        Some(other) => {
            eprintln!("Unknown option: {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn package(dir: Option<&String>) -> ExitCode {
    let dir = std::path::PathBuf::from(dir.map(String::as_str).unwrap_or("dist"));
    match package::build(&dir) {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Packaging failed: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_gui() -> ExitCode {
    logging::init();
    let settings = settings::Settings::load();
    let library = library::Library::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_app_id("audioseek")
            .with_inner_size([720.0, 820.0])
            .with_min_inner_size([400.0, 480.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/audioseek.png"))
                    .expect("the bundled icon is a valid PNG"),
            ),
        ..Default::default()
    };
    let result = eframe::run_native(
        APP_TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::PodApp::new(cc, settings, library)))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("could not start the window: {e}");
            eprintln!("Could not start the window: {e}");
            ExitCode::FAILURE
        }
    }
}
