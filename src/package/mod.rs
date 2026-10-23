//! Release packaging: an installer package (.pkg) on macOS, and a .deb on
//! Linux. It is kept apart from the code that runs when the app is used, and
//! only compiled on the platform it packages for, as in the Speech Output
//! Engine.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::build;

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(all(unix, not(target_os = "macos")))]
pub use linux::build;
