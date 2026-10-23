# Changelog

## [Unreleased]

### Added

- Warnings, errors and crashes are written to `audioseek.log`, beside `feeds.json`. The Settings tab shows where it is.
- When it starts, the app checks GitHub for a newer version and says if there is one. The Settings tab has a button to open the download page, a button to check straight away, and a setting to stop checking.
- A feed can be marked `"disabled": true` in `feeds.json`. "Fetch every feed" then skips it, and it isn't fetched when shown on the Episodes tab, but it can still be fetched on its own.

### Changed

- Pressing Escape (or whichever key Cancel uses) now asks before stopping a fetch or download. The Cancel button still stops it straight away.
- While feeds are fetched, `feeds.json` is saved every few seconds and at the end, rather than after each feed, so the window no longer stutters while a long list is fetched. Closing the window part way through still saves what has been fetched.
- Feeds are fetched six at a time, so fetching a long list is several times quicker, and one slow server no longer holds up the rest.
- The Episodes tab does much less work each time the window is redrawn, and downloads update the progress bar at most ten times a second, so a podcast with thousands of episodes stays responsive while downloading. Every episode is still in the list for screen readers.
- Web addresses in the log have passwords, keys and query strings taken out, so it is safe to attach to a bug report even with private feeds.
- A download folder must be a full path; a relative one is refused with a message.
- The Ubuntu package depends on `zenity`, which shows the app's questions and messages on Linux.

### Fixed

- An episode could be skipped as "already in the folder", and marked as downloaded, when the file there was a different episode's. This happened when two podcasts shared a folder and published at the same time, when two podcasts' names made the same folder, and, with title naming, when a feed dropped an old episode and the " (2)" numbering moved along. `feeds.json` now records the file each episode is saved as (in a new `files` field, which older versions ignore), and no other episode is given that name.
- If fetching or downloading crashes, the app now says so and can be used again, instead of waiting for the job for ever.
- A damaged `feeds.json` or `settings.json` is kept under a dated name, and the app says so when it starts, rather than starting empty without a word. A `settings.json` that can't be read (locked, say) is no longer saved over.
- A failure to save the settings is now reported.
- On Linux without zenity, Remove, Download all and Escape now say that they couldn't ask first, rather than doing nothing silently.
- Messages say "1 feed", "1 episode" and "1 was already in the folder" rather than "1 feeds" and the like.

## [1.0.0] - 2026-10-23

### Added

- Import the feeds in an OPML file, or add a feed by its address, and export the list again as OPML.
- Every feed is kept in `feeds.json`, with when it was last fetched and which episodes have been downloaded, so the list survives losing the OPML file.
- Download all of a podcast's episodes, or just the ones you tick. Episodes are named by when they were published, in your own time zone, such as `2026-10-23-14-05-00.mp3`, in a folder for each podcast. Episodes already in the folder are skipped.
- Every part of the window has its own keyboard shortcut, and each can be changed or turned off on the Shortcuts tab. F1 lists them.
- Installers for Windows (x64 and Arm), Ubuntu and Apple Silicon Macs.
