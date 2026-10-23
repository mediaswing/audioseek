# Changelog

## [Unreleased]

### Added

- Warnings, errors and crashes are written to `audioseek.log`, beside `feeds.json`. The Settings tab shows where it is.
- When it starts, the app checks GitHub for a newer version and says if there is one. The Settings tab has a button to open the download page, a button to check straight away, and a setting to stop checking.

## [1.0.0] - 2026-10-23

### Added

- Import the feeds in an OPML file, or add a feed by its address, and export the list again as OPML.
- Every feed is kept in `feeds.json`, with when it was last fetched and which episodes have been downloaded, so the list survives losing the OPML file.
- Download all of a podcast's episodes, or just the ones you tick. Episodes are named by when they were published, in your own time zone, such as `2026-10-23-14-05-00.mp3`, in a folder for each podcast. Episodes already in the folder are skipped.
- Every part of the window has its own keyboard shortcut, and each can be changed or turned off on the Shortcuts tab. F1 lists them.
- Installers for Windows (x64 and Arm), Ubuntu and Apple Silicon Macs.
