# Podcast Batch Downloader

Downloads podcast episodes from the feeds in an OPML file, all at once or a
few at a time. It is built like the Speech Output Engine: an egui window with
tabs, every control on its own full-width line, a thick focus ring, a status
line that screen readers announce, and a keyboard shortcut for every part of
the window.


## Using it

1. **Feeds tab:** press Ctrl+O to import an OPML file. The new feeds are
   fetched straight away. You can also type a feed's address (Ctrl+U) and
   press Enter.
2. **Episodes tab:** choose a podcast (Ctrl+K), then tick episodes (Space)
   and press Ctrl+S to download them, or press Ctrl+D to download every
   episode of it.
3. **Settings tab:** choose the download folder and how files are named.
4. **Shortcuts tab:** change any keyboard shortcut, or turn them all off.

By default each episode is saved as the date and time it was published, in
this computer's time zone, such as `2026-10-23-14-05-00.mp3`, in a folder
named after the podcast. Episodes already in the folder are skipped.
Downloads are written to a `.part` file first, so a cancelled download never
looks finished.

## Saved feeds

Every feed is kept in `feeds.json`, with when it was last fetched and which
episodes have been downloaded, so the list survives losing the OPML file
(Ctrl+E exports it again as OPML). It is in `%APPDATA%\AudioSeek` on Windows,
`~/Library/Application Support/AudioSeek` on macOS and `~/.config/audioseek`
on Linux, beside `settings.json`.

## Log file and updates

Warnings, errors and crashes are written to `audioseek.log` in the same
folder, which is worth attaching when reporting a problem. When it grows past
1 MB it is moved to `audioseek.old.log` the next time the app starts.

When the app starts it asks GitHub whether there is a newer version, and if
there is, says so in the status line and on the Settings tab, which has a
button to open the download page. Nothing is downloaded or installed by
itself. The check can be turned off, or run straight away, on the Settings
tab.

## Keyboard shortcuts

F1 lists them all. On a Mac, Cmd takes the place of Ctrl. A shortcut for a
control on another tab goes to that tab first. These are the defaults; each
can be changed or turned off on the Shortcuts tab, and they are saved in
`settings.json`.

| Where    | Shortcut          | What it does                              |
|----------|-------------------|-------------------------------------------|
| Anywhere | Ctrl+1 to Ctrl+4  | Feeds, Episodes, Settings, Shortcuts tabs |
|          | Ctrl+Tab, Ctrl+Shift+Tab | Next and previous tab              |
|          | F1                | List the shortcuts                        |
|          | F7                | Say how far along the current job is      |
|          | F8                | Repeat the last message                   |
|          | Esc               | Cancel fetching or downloading            |
| Feeds    | Ctrl+O            | Import an OPML file                       |
|          | Ctrl+E            | Export the feeds as OPML                  |
|          | Ctrl+U            | Feed address box                          |
|          | Ctrl+L            | Feed list                                 |
|          | Ctrl+R            | Remove the selected feed                  |
|          | F5                | Fetch the selected feed                   |
|          | F6                | Fetch every feed                          |
| Episodes | Ctrl+K            | Podcast list                              |
|          | Ctrl+F            | Filter by title                           |
|          | Ctrl+I            | Episode list                              |
|          | F2 / F3 / F4      | Tick all shown / untick all / tick new    |
|          | Ctrl+S            | Download the ticked episodes              |
|          | Ctrl+D            | Download every episode                    |
| Settings | Ctrl+G            | Download folder box                       |
|          | Ctrl+B            | Choose the download folder                |
|          | F9                | Open the download folder                  |
|          | Ctrl+M (Mac: Cmd+8) | File names list                         |
|          | Ctrl+J            | Folder per podcast on or off              |
|          | Ctrl+H (Mac: Cmd+9) | Open the folder holding feeds.json      |

As in the Speech Output Engine, the shortcuts (and every choice offered on
the Shortcuts tab) avoid single letters, Alt and Option, Insert, and editing
keys such as Ctrl+C, so they don't clash with screen readers or typing. The
one difference: Ctrl+F, Ctrl+G and Ctrl+H are allowed on Windows and Linux,
where nothing else uses them. On a Mac, Cmd+H hides apps and Cmd+M minimises
them, so the Mac gets Cmd+8 and Cmd+9 instead.

## Tests

`cargo test` runs the offline tests. `cargo test -- --ignored` also fetches
some real feeds and downloads one episode.

## Licence

audioseek is free software under the GNU General Public License, version 3
or (at your option) any later version; see [LICENSE](LICENSE). The icon is
from the Papirus icon theme (GPL-3.0, see `assets/icon/CREDITS.txt`), and the
Google Sans fonts are under the SIL Open Font License (`assets/fonts/OFL.txt`).
