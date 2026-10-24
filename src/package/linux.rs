//! The Debian and Ubuntu package: `audioseek --package-deb <dir>` writes a
//! `.deb` containing the running binary, assembled in Rust (ar + tar + xz) so
//! that a release workflow needs no shell script or extra packaging tools.

use anyhow::Context;
use std::path::{Path, PathBuf};

/// What the window needs: OpenGL, X11 or Wayland keyboard handling, ALSA for
/// the sounds, and zenity, which shows its message boxes and questions
/// (without it, Remove and Download all can't ask first, so do nothing).
/// Feeds and episodes are fetched with rustls, so there is no OpenSSL.
const DEB_DEPENDS: &str = "libc6 (>= 2.35), libgcc-s1, libasound2t64 | libasound2, libgl1, libxkbcommon0, \
libxkbcommon-x11-0, libwayland-client0, zenity";
/// File dialogs go through the desktop portal; AT-SPI and Orca are how the
/// window reaches screen readers.
const DEB_RECOMMENDS: &str = "xdg-desktop-portal, at-spi2-core, orca";

const DESKTOP_ENTRY: &str = "[Desktop Entry]
Type=Application
Name=Podcast Batch Downloader
GenericName=Podcast downloader
Comment=Download podcast episodes from the feeds in an OPML file
Exec=audioseek
Icon=audioseek
Terminal=false
Categories=AudioVideo;Audio;Network;Accessibility;
Keywords=podcast;rss;opml;download;episodes;accessibility;
";

const COPYRIGHT: &str = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: audioseek

Files: *
Copyright: 2026 The audioseek contributors
License: GPL-3.0-or-later
 On Debian systems, the full text of the GNU General Public License
 version 3 can be found in /usr/share/common-licenses/GPL-3.

Files: usr/share/icons/hicolor/*/apps/audioseek.*
Copyright: Papirus Development Team
License: GPL-3.0
 From the Papirus icon theme,
 https://github.com/PapirusDevelopmentTeam/papirus-icon-theme

Files: embedded fonts (Google Sans Medium and Bold)
Copyright: Google LLC
License: OFL-1.1
 See /usr/share/doc/audioseek/fonts-OFL.txt.
";

fn deb_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "i386",
        "arm" => "armhf",
        other => other,
    }
}

/// One file to place in the package: (path inside the package, mode, bytes).
type DebFile = (&'static str, u32, Vec<u8>);

/// xz rather than gzip makes the package about a third smaller. dpkg has read
/// xz since 1.17, long before Ubuntu 22.04.
fn tar_xz(dirs: &[&str], files: &[DebFile], mtime: u64) -> anyhow::Result<Vec<u8>> {
    let xz = xz2::write::XzEncoder::new(Vec::new(), 9);
    let mut tar = tar::Builder::new(xz);
    for dir in dirs {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Directory);
        h.set_mode(0o755);
        h.set_uid(0);
        h.set_gid(0);
        h.set_mtime(mtime);
        h.set_size(0);
        tar.append_data(&mut h, dir, std::io::empty())?;
    }
    for (path, mode, data) in files {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Regular);
        h.set_mode(*mode);
        h.set_uid(0);
        h.set_gid(0);
        h.set_mtime(mtime);
        h.set_size(data.len() as u64);
        tar.append_data(&mut h, path, data.as_slice())?;
    }
    Ok(tar.into_inner()?.finish()?)
}

fn ar_member(out: &mut Vec<u8>, name: &str, data: &[u8], mtime: u64) {
    out.extend_from_slice(
        format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", name, mtime, 0, 0, "100644", data.len()).as_bytes(),
    );
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(b'\n');
    }
}

/// Builds `audioseek_<version>_<arch>.deb` in `out_dir` from the running binary.
pub fn build(out_dir: &Path) -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("could not locate the running executable")?;
    let binary = std::fs::read(&exe)?;
    let version = env!("CARGO_PKG_VERSION");
    let mtime = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    });

    let files: Vec<DebFile> = vec![
        ("./usr/bin/audioseek", 0o755, binary),
        ("./usr/share/applications/audioseek.desktop", 0o644, DESKTOP_ENTRY.as_bytes().to_vec()),
        (
            "./usr/share/icons/hicolor/256x256/apps/audioseek.png",
            0o644,
            include_bytes!("../../assets/icon/audioseek.png").to_vec(),
        ),
        (
            "./usr/share/icons/hicolor/scalable/apps/audioseek.svg",
            0o644,
            include_bytes!("../../assets/icon/audioseek.svg").to_vec(),
        ),
        ("./usr/share/doc/audioseek/copyright", 0o644, COPYRIGHT.as_bytes().to_vec()),
        ("./usr/share/doc/audioseek/fonts-OFL.txt", 0o644, include_bytes!("../../assets/fonts/OFL.txt").to_vec()),
    ];
    let dirs = [
        "./",
        "./usr/",
        "./usr/bin/",
        "./usr/share/",
        "./usr/share/applications/",
        "./usr/share/icons/",
        "./usr/share/icons/hicolor/",
        "./usr/share/icons/hicolor/256x256/",
        "./usr/share/icons/hicolor/256x256/apps/",
        "./usr/share/icons/hicolor/scalable/",
        "./usr/share/icons/hicolor/scalable/apps/",
        "./usr/share/doc/",
        "./usr/share/doc/audioseek/",
    ];
    let installed_kib = files.iter().map(|f| f.2.len() as u64).sum::<u64>().div_ceil(1024);

    let control = format!(
        "Package: audioseek\n\
         Version: {version}\n\
         Architecture: {arch}\n\
         Maintainer: wrichards <mediaswing@outlook.com>\n\
         Installed-Size: {installed_kib}\n\
         Depends: {DEB_DEPENDS}\n\
         Recommends: {DEB_RECOMMENDS}\n\
         Section: sound\n\
         Priority: optional\n\
         Description: Podcast Batch Downloader, an accessible podcast downloader\n \
         Imports the feeds in an OPML file and downloads all of a podcast's\n \
         episodes, or the ones you tick, named by when they were published.\n \
         Keeps its own list of feeds, with when each was last fetched.\n \
         Fully usable by keyboard and screen reader.\n",
        arch = deb_arch()
    );

    let control_tar = tar_xz(&["./"], &[("./control", 0o644, control.into_bytes())], mtime)?;
    let data_tar = tar_xz(&dirs, &files, mtime)?;

    let mut deb = b"!<arch>\n".to_vec();
    ar_member(&mut deb, "debian-binary", b"2.0\n", mtime);
    ar_member(&mut deb, "control.tar.xz", &control_tar, mtime);
    ar_member(&mut deb, "data.tar.xz", &data_tar, mtime);

    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join(format!("audioseek_{version}_{}.deb", deb_arch()));
    std::fs::write(&path, deb)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Packages the test binary and, where dpkg is installed (as on Ubuntu),
    /// checks that dpkg accepts the result.
    #[test]
    fn builds_valid_deb() {
        let dir = tempfile::tempdir().unwrap();
        let deb = build(dir.path()).unwrap();
        let bytes = std::fs::read(&deb).unwrap();
        assert!(bytes.starts_with(b"!<arch>\ndebian-binary   "));
        let dpkg = Path::new("/usr/bin/dpkg-deb");
        if dpkg.exists() {
            let info = Command::new(dpkg).arg("--info").arg(&deb).output().unwrap();
            assert!(info.status.success(), "{}", String::from_utf8_lossy(&info.stderr));
            let contents = Command::new(dpkg).arg("--contents").arg(&deb).output().unwrap();
            assert!(String::from_utf8_lossy(&contents.stdout).contains("usr/bin/audioseek"));
        }
    }
}
