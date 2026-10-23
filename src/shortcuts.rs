//! Keyboard shortcuts: which keys do what, checks that keep them clear of
//! screen readers and the operating system, and matching of key presses.
//! Every part of the window has its own shortcut, and each works from
//! anywhere: one for a control on another tab goes to that tab first.
//!
//! As in the Speech Output Engine, screen readers see every key first and
//! keep their own commands, and the shortcuts offered here are also kept
//! clear of:
//! * single letters, digits and punctuation, which people type and screen
//!   readers use for quick navigation (WCAG 2.1.4);
//! * Alt and Option, because Ctrl+Alt is AltGr on Windows and Option types
//!   characters on a Mac;
//! * Tab, Enter, Space, the arrow keys and editing shortcuts such as Ctrl+C;
//! * keys the operating system or its accessibility features use.
//!
//! Modifiers must match exactly, so typing "ś" with AltGr+S never downloads
//! anything. Each shortcut can be changed or turned off on the Shortcuts
//! tab, and all of them can be turned off at once.

use egui::{Event, Key, Modifiers};
use std::collections::BTreeMap;

/// The value saved in settings for a shortcut that has been turned off.
pub const OFF: &str = "off";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    Windows,
    Mac,
    Linux,
}

impl Platform {
    #[cfg(test)]
    pub const ALL: [Platform; 3] = [Platform::Windows, Platform::Mac, Platform::Linux];

    pub const CURRENT: Platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    };
}

/// Modifier keys. `command` is Cmd on a Mac and Ctrl elsewhere. `ctrl` is
/// the Control key on a Mac, and is never set elsewhere.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mods {
    pub command: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    /// The modifiers held for a key press.
    fn pressed(m: Modifiers, platform: Platform) -> Self {
        match platform {
            Platform::Mac => Mods { command: m.mac_cmd, ctrl: m.ctrl, shift: m.shift, alt: m.alt },
            _ => Mods { command: m.ctrl || m.command, ctrl: false, shift: m.shift, alt: m.alt },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shortcut {
    pub mods: Mods,
    pub key: Key,
}

impl Shortcut {
    const fn key(key: Key) -> Self {
        Shortcut { mods: Mods { command: false, ctrl: false, shift: false, alt: false }, key }
    }

    /// Cmd on a Mac, Ctrl elsewhere.
    const fn command(key: Key) -> Self {
        Shortcut { mods: Mods { command: true, ctrl: false, shift: false, alt: false }, key }
    }

    /// The Control key on every platform, as in Ctrl+Tab.
    fn ctrl(key: Key, shift: bool, platform: Platform) -> Self {
        let mac = platform == Platform::Mac;
        Shortcut { mods: Mods { command: !mac, ctrl: mac, shift, alt: false }, key }
    }

    /// Reads a shortcut as saved in settings, such as "Ctrl+Shift+Tab" or
    /// "F5". "Cmd" means the Command key on a Mac and Ctrl elsewhere.
    pub fn parse(text: &str, platform: Platform) -> Option<Self> {
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = Key::from_name(parts.pop()?)?;
        let mut mods = Mods::default();
        for part in parts {
            let flag = match part {
                "Cmd" => &mut mods.command,
                "Ctrl" if platform == Platform::Mac => &mut mods.ctrl,
                "Ctrl" => &mut mods.command,
                "Shift" => &mut mods.shift,
                "Alt" | "Option" => &mut mods.alt,
                _ => return None,
            };
            if *flag {
                return None;
            }
            *flag = true;
        }
        Some(Shortcut { mods, key })
    }

    /// The text saved in settings, which `parse` reads back.
    pub fn to_text(self, platform: Platform) -> String {
        let mut parts = Vec::new();
        if self.mods.ctrl {
            parts.push("Ctrl");
        }
        if self.mods.command {
            parts.push(if platform == Platform::Mac { "Cmd" } else { "Ctrl" });
        }
        if self.mods.alt {
            parts.push("Alt");
        }
        if self.mods.shift {
            parts.push("Shift");
        }
        parts.push(self.key.name());
        parts.join("+")
    }

    /// As shown on buttons and lists, such as "Ctrl+O" or "Esc".
    pub fn short(self, platform: Platform) -> String {
        let mac = platform == Platform::Mac;
        let mut parts = Vec::new();
        if self.mods.ctrl {
            parts.push("Ctrl");
        }
        if self.mods.command {
            parts.push(if mac { "Cmd" } else { "Ctrl" });
        }
        if self.mods.alt {
            parts.push(if mac { "Option" } else { "Alt" });
        }
        if self.mods.shift {
            parts.push("Shift");
        }
        parts.push(match self.key {
            Key::Escape => "Esc",
            Key::Period => ".",
            Key::Enter if mac => "Return",
            Key::PageUp => "Page Up",
            Key::PageDown => "Page Down",
            key => key.name(),
        });
        parts.join("+")
    }
}

/// Why a shortcut is not offered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conflict {
    /// A letter, digit or symbol on its own: typed as text, and used by
    /// screen readers to move around a window (WCAG 2.1.4).
    CharacterKey,
    /// Moves focus, scrolls, edits text or presses the focused control.
    NavigationKey,
    /// Alt or Option: menus, AltGr characters and VoiceOver commands.
    AltKey,
    /// Insert is the NVDA, JAWS and Narrator key.
    ScreenReaderKey,
    /// Used by the operating system, its accessibility features, input
    /// methods or familiar editing commands.
    Reserved,
}

/// Checks a shortcut against the keys that screen readers, the operating
/// system and text editing rely on. The same rules as the Speech Output
/// Engine's, except that Ctrl+F (which filters the episodes here, much as it
/// finds elsewhere), Ctrl+G and Ctrl+H are allowed outside the Mac: Windows
/// and Linux desktops don't use them, and this app has no find-next or
/// replace for them to clash with.
pub fn check(shortcut: Shortcut, platform: Platform) -> Result<(), Conflict> {
    use Key::*;
    let Shortcut { mods, key } = shortcut;
    let function_key = function_key_number(key);
    let navigation = matches!(
        key,
        Tab | Enter | Space | Backspace | Delete | ArrowUp | ArrowDown | ArrowLeft | ArrowRight | Home | End | PageUp
            | PageDown
    );
    if key == Insert {
        return Err(Conflict::ScreenReaderKey);
    }
    if mods.alt {
        return Err(Conflict::AltKey);
    }

    if !mods.command && !mods.ctrl {
        return match function_key {
            // Shift+F10 opens context menus; other Shift+function keys are
            // left alone too.
            Some(_) if mods.shift => Err(Conflict::Reserved),
            Some(1..=9) => Ok(()),
            // F10 opens menus, F11 is full screen or Show Desktop, F12 opens
            // developer tools and dashboards.
            Some(_) => Err(Conflict::Reserved),
            None if key == Escape && !mods.shift => Ok(()),
            None if key == Escape => Err(Conflict::Reserved),
            None if navigation => Err(Conflict::NavigationKey),
            None => Err(Conflict::CharacterKey),
        };
    }

    // Ctrl or Cmd with a function key: Cmd+F5 turns VoiceOver on and off,
    // Control+F1 to F8 are Full Keyboard Access on a Mac, Ctrl+F4 closes.
    if function_key.is_some() {
        return Err(Conflict::Reserved);
    }
    // Word and line movement, and deleting words.
    if matches!(key, ArrowUp | ArrowDown | ArrowLeft | ArrowRight | Home | End | Backspace | Delete) {
        return Err(Conflict::NavigationKey);
    }
    let tab_keys = matches!(key, Tab | PageUp | PageDown);
    if platform == Platform::Mac {
        if mods.ctrl {
            // Control on a Mac is otherwise Mission Control, Spaces, input
            // sources, and Command+Control system commands.
            return if !mods.command && tab_keys { Ok(()) } else { Err(Conflict::Reserved) };
        }
        if tab_keys {
            // Cmd+Tab switches apps.
            return Err(Conflict::Reserved);
        }
    }
    if mods.shift {
        // Only Ctrl+Shift+Tab, the previous tab. Shift combinations take
        // screenshots on a Mac, switch keyboard layouts on Windows and are
        // used by input methods and other apps.
        return if key == Tab { Ok(()) } else { Err(Conflict::Reserved) };
    }
    let reserved: &[Key] = match platform {
        // Editing, hide, minimise, new, print, quit, new tab, close,
        // settings, switching windows and Spotlight.
        Platform::Mac => &[A, C, V, X, Z, Y, H, M, N, P, Q, T, W, Comma, Backtick, Space, Escape],
        // Editing, new, print, quit, new tab and close, the Start menu, and
        // input method switching (Ctrl+Space, Ctrl+.).
        _ => &[A, C, V, X, Z, Y, N, P, Q, T, W, Space, Escape, Period, Comma],
    };
    if reserved.contains(&key) { Err(Conflict::Reserved) } else { Ok(()) }
}

fn function_key_number(key: Key) -> Option<u8> {
    let name = key.name();
    name.strip_prefix('F').and_then(|n| n.parse().ok())
}

/// The digit on a key, for Ctrl+1 to Ctrl+4.
fn digit(key: Key) -> Option<usize> {
    let name = key.name();
    (name.len() == 1).then(|| name.parse().ok()).flatten()
}

/// Where an action's control is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Anywhere,
    Feeds,
    Episodes,
    Settings,
}

impl Place {
    pub const ALL: [Place; 4] = [Place::Anywhere, Place::Feeds, Place::Episodes, Place::Settings];

    /// The heading the place's shortcuts are listed under.
    pub fn heading(self) -> &'static str {
        match self {
            Place::Anywhere => "Anywhere",
            Place::Feeds => "Feeds tab",
            Place::Episodes => "Episodes tab",
            Place::Settings => "Settings tab",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    // Anywhere
    /// Ctrl+1 goes to the first tab, Ctrl+2 to the second and so on.
    GoToTab,
    NextTab,
    PreviousTab,
    Help,
    Progress,
    RepeatStatus,
    Cancel,
    // Feeds tab
    ImportOpml,
    ExportOpml,
    FeedAddress,
    FeedList,
    RemoveFeed,
    RefreshFeed,
    RefreshAll,
    // Episodes tab
    PodcastPicker,
    Filter,
    EpisodeList,
    SelectAll,
    SelectNone,
    SelectNew,
    DownloadSelected,
    DownloadAll,
    // Settings tab
    FolderField,
    Browse,
    OpenFolder,
    Naming,
    FolderPerPodcast,
    DataFolder,
}

impl Action {
    pub const ALL: [Action; 28] = [
        Action::GoToTab,
        Action::NextTab,
        Action::PreviousTab,
        Action::Help,
        Action::Progress,
        Action::RepeatStatus,
        Action::Cancel,
        Action::ImportOpml,
        Action::ExportOpml,
        Action::FeedAddress,
        Action::FeedList,
        Action::RemoveFeed,
        Action::RefreshFeed,
        Action::RefreshAll,
        Action::PodcastPicker,
        Action::Filter,
        Action::EpisodeList,
        Action::SelectAll,
        Action::SelectNone,
        Action::SelectNew,
        Action::DownloadSelected,
        Action::DownloadAll,
        Action::FolderField,
        Action::Browse,
        Action::OpenFolder,
        Action::Naming,
        Action::FolderPerPodcast,
        Action::DataFolder,
    ];

    /// The name saved in settings.
    pub fn id(self) -> &'static str {
        match self {
            Action::GoToTab => "go_to_tab",
            Action::NextTab => "next_tab",
            Action::PreviousTab => "previous_tab",
            Action::Help => "help",
            Action::Progress => "progress",
            Action::RepeatStatus => "repeat_status",
            Action::Cancel => "cancel",
            Action::ImportOpml => "import_opml",
            Action::ExportOpml => "export_opml",
            Action::FeedAddress => "feed_address",
            Action::FeedList => "feed_list",
            Action::RemoveFeed => "remove_feed",
            Action::RefreshFeed => "refresh_feed",
            Action::RefreshAll => "refresh_all",
            Action::PodcastPicker => "podcast_picker",
            Action::Filter => "filter",
            Action::EpisodeList => "episode_list",
            Action::SelectAll => "select_all",
            Action::SelectNone => "select_none",
            Action::SelectNew => "select_new",
            Action::DownloadSelected => "download_selected",
            Action::DownloadAll => "download_all",
            Action::FolderField => "folder_field",
            Action::Browse => "browse",
            Action::OpenFolder => "open_folder",
            Action::Naming => "naming",
            Action::FolderPerPodcast => "folder_per_podcast",
            Action::DataFolder => "data_folder",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::GoToTab => "Go to a tab",
            Action::NextTab => "Next tab",
            Action::PreviousTab => "Previous tab",
            Action::Help => "List the keyboard shortcuts",
            Action::Progress => "Say how far along the current job is",
            Action::RepeatStatus => "Repeat the last message",
            Action::Cancel => "Cancel fetching or downloading",
            Action::ImportOpml => "Import an OPML file",
            Action::ExportOpml => "Export the feeds as an OPML file",
            Action::FeedAddress => "Feed address box (Enter adds the feed)",
            Action::FeedList => "Feed list",
            Action::RemoveFeed => "Remove the selected feed",
            Action::RefreshFeed => "Fetch the selected feed",
            Action::RefreshAll => "Fetch every feed",
            Action::PodcastPicker => "Podcast list on the Episodes tab",
            Action::Filter => "Filter episodes by title",
            Action::EpisodeList => "Episode list",
            Action::SelectAll => "Tick every episode shown",
            Action::SelectNone => "Untick every episode",
            Action::SelectNew => "Tick the episodes not yet downloaded",
            Action::DownloadSelected => "Download the ticked episodes",
            Action::DownloadAll => "Download every episode of the podcast",
            Action::FolderField => "Download folder box",
            Action::Browse => "Choose the download folder",
            Action::OpenFolder => "Open the download folder",
            Action::Naming => "File names list",
            Action::FolderPerPodcast => "Turn a folder for each podcast on or off",
            Action::DataFolder => "Open the folder holding feeds.json",
        }
    }

    pub fn place(self) -> Place {
        use Action::*;
        match self {
            GoToTab | NextTab | PreviousTab | Help | Progress | RepeatStatus | Cancel => Place::Anywhere,
            ImportOpml | ExportOpml | FeedAddress | FeedList | RemoveFeed | RefreshFeed | RefreshAll => Place::Feeds,
            PodcastPicker | Filter | EpisodeList | SelectAll | SelectNone | SelectNew | DownloadSelected
            | DownloadAll => Place::Episodes,
            FolderField | Browse | OpenFolder | Naming | FolderPerPodcast | DataFolder => Place::Settings,
        }
    }

    pub fn default_shortcut(self, platform: Platform) -> Option<Shortcut> {
        use Shortcut as S;
        let mac = platform == Platform::Mac;
        Some(match self {
            Action::GoToTab => S::command(Key::Num1),
            Action::NextTab => S::ctrl(Key::Tab, false, platform),
            Action::PreviousTab => S::ctrl(Key::Tab, true, platform),
            Action::Help => S::key(Key::F1),
            Action::Progress => S::key(Key::F7),
            Action::RepeatStatus => S::key(Key::F8),
            Action::Cancel => S::key(Key::Escape),
            Action::ImportOpml => S::command(Key::O),
            Action::ExportOpml => S::command(Key::E),
            Action::FeedAddress => S::command(Key::U),
            Action::FeedList => S::command(Key::L),
            Action::RemoveFeed => S::command(Key::R),
            Action::RefreshFeed => S::key(Key::F5),
            Action::RefreshAll => S::key(Key::F6),
            Action::PodcastPicker => S::command(Key::K),
            Action::Filter => S::command(Key::F),
            Action::EpisodeList => S::command(Key::I),
            Action::SelectAll => S::key(Key::F2),
            Action::SelectNone => S::key(Key::F3),
            Action::SelectNew => S::key(Key::F4),
            Action::DownloadSelected => S::command(Key::S),
            Action::DownloadAll => S::command(Key::D),
            Action::FolderField => S::command(Key::G),
            Action::Browse => S::command(Key::B),
            Action::OpenFolder => S::key(Key::F9),
            // Cmd+M minimises and Cmd+H hides the app on a Mac.
            Action::Naming if mac => S::command(Key::Num8),
            Action::Naming => S::command(Key::M),
            Action::FolderPerPodcast => S::command(Key::J),
            Action::DataFolder if mac => S::command(Key::Num9),
            Action::DataFolder => S::command(Key::H),
        })
    }

    /// The shortcuts that can be chosen for this action, apart from Off.
    pub fn choices(self, platform: Platform) -> Vec<Shortcut> {
        use Key::*;
        let mut choices = match self {
            Action::GoToTab => return vec![Shortcut::command(Num1)],
            Action::NextTab => {
                return vec![Shortcut::ctrl(Tab, false, platform), Shortcut::ctrl(PageDown, false, platform)];
            }
            Action::PreviousTab => {
                return vec![Shortcut::ctrl(Tab, true, platform), Shortcut::ctrl(PageUp, false, platform)];
            }
            Action::Help => vec![Shortcut::key(F1)],
            Action::Cancel => vec![Shortcut::key(Escape)],
            _ => Vec::new(),
        };
        choices.extend([F2, F3, F4, F5, F6, F7, F8, F9].map(Shortcut::key));
        choices.extend([B, D, E, F, G, H, I, J, K, L, M, O, R, S, U].map(Shortcut::command));
        // Ctrl+1 to Ctrl+4 are the tabs.
        choices.extend([Num5, Num6, Num7, Num8, Num9, Enter].map(Shortcut::command));
        if platform == Platform::Mac {
            choices.push(Shortcut::command(Period));
        }
        choices.retain(|s| check(*s, platform).is_ok());
        choices
    }
}

/// What a key press asks the app to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Run(Action),
    /// Go to the tab with this index, counting from 0.
    Tab(usize),
    /// Close the open dropdown list, keeping focus on its dropdown.
    CloseList,
}

/// What the window looks like when a key is pressed.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyContext {
    /// A dropdown list is open, so keys belong to it.
    pub popup_open: bool,
    /// The index of the tab button that has focus, if one has.
    pub focused_tab: Option<usize>,
    pub tab_count: usize,
    /// Something is running that Cancel can stop.
    pub busy: bool,
}

/// The shortcut chosen for each action.
#[derive(Clone, Debug)]
pub struct Keymap {
    platform: Platform,
    enabled: bool,
    bindings: Vec<(Action, Option<Shortcut>)>,
}

impl Keymap {
    /// The defaults, with the changes saved in settings. A saved shortcut
    /// that is not one of the action's choices, or is already used by an
    /// earlier action, is ignored, so a damaged or hand-edited settings file
    /// cannot bring back a conflicting key.
    pub fn new(enabled: bool, saved: &BTreeMap<String, String>, platform: Platform) -> Self {
        let mut bindings: Vec<(Action, Option<Shortcut>)> = Vec::new();
        for action in Action::ALL {
            let chosen = match saved.get(action.id()) {
                None => action.default_shortcut(platform),
                Some(text) if text == OFF => None,
                Some(text) => match Shortcut::parse(text, platform).filter(|s| action.choices(platform).contains(s)) {
                    Some(shortcut) => Some(shortcut),
                    None => {
                        log::warn!("ignoring the saved shortcut {text:?} for {}", action.id());
                        action.default_shortcut(platform)
                    }
                },
            };
            let taken = chosen.is_some() && bindings.iter().any(|(_, b)| *b == chosen);
            if taken {
                log::warn!("turning off the shortcut for {}: another action already uses it", action.id());
            }
            bindings.push((action, chosen.filter(|_| !taken)));
        }
        Keymap { platform, enabled, bindings }
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// The shortcut chosen for `action`, even while shortcuts are off.
    pub fn binding(&self, action: Action) -> Option<Shortcut> {
        self.bindings.iter().find(|(a, _)| *a == action).and_then(|(_, b)| *b)
    }

    /// The shortcut that works for `action` now.
    pub fn active(&self, action: Action) -> Option<Shortcut> {
        self.binding(action).filter(|_| self.enabled)
    }

    /// The action other than `except` that uses `shortcut`.
    pub fn used_by(&self, shortcut: Shortcut, except: Action) -> Option<Action> {
        self.bindings.iter().find(|(a, b)| *a != except && *b == Some(shortcut)).map(|(a, _)| *a)
    }

    /// How a shortcut for `action` is shown, such as "Ctrl+1 – Ctrl+4" for
    /// going to a tab.
    pub fn describe(&self, action: Action, shortcut: Shortcut, tab_count: usize) -> String {
        let p = self.platform;
        if action == Action::GoToTab {
            let last = Key::from_name(&tab_count.to_string()).unwrap_or(Key::Num4);
            return format!("{} – {}", shortcut.short(p), Shortcut { key: last, ..shortcut }.short(p));
        }
        shortcut.short(p)
    }

    /// " (Ctrl+O)" to follow a button label, or nothing if `action` has no
    /// shortcut at the moment.
    pub fn hint(&self, action: Action) -> String {
        self.active(action).map(|s| format!(" ({})", s.short(self.platform))).unwrap_or_default()
    }

    /// The shortcut for the tab at `index`, such as " (Ctrl+2)", while going
    /// to tabs has one.
    pub fn tab_hint(&self, index: usize) -> String {
        let Some(first) = self.active(Action::GoToTab) else { return String::new() };
        let Some(key) = Key::from_name(&(index + 1).to_string()) else { return String::new() };
        format!(" ({})", Shortcut { key, ..first }.short(self.platform))
    }

    /// Takes the key presses that are shortcuts out of `events`, so the
    /// controls never see them, and returns what they ask for. Cancel only
    /// runs while something is running; otherwise its key is left for the
    /// controls.
    ///
    /// While a tab has focus, Left, Right, Home and End move between tabs,
    /// as in the ARIA tabs pattern. Escape on its own is always taken: egui
    /// would otherwise take focus away from the control, leaving screen
    /// reader users nowhere. While a list is open it becomes `CloseList`.
    /// Typed text is never touched, and held keys repeat only for moving
    /// between tabs.
    pub fn take(&self, events: &mut Vec<Event>, cx: KeyContext) -> Vec<Command> {
        let mut commands = Vec::new();
        events.retain(|event| {
            let Event::Key { key, pressed: true, repeat, modifiers, .. } = event else { return true };
            match self.command_for(*key, *modifiers, cx) {
                Some(Command::Run(action)) if *repeat && !matches!(action, Action::NextTab | Action::PreviousTab) => {
                    false
                }
                Some(command) => {
                    commands.push(command);
                    false
                }
                None if *key == Key::Escape && modifiers.is_none() => {
                    if cx.popup_open {
                        commands.push(Command::CloseList);
                    }
                    false
                }
                None => true,
            }
        });
        commands
    }

    fn command_for(&self, key: Key, modifiers: Modifiers, cx: KeyContext) -> Option<Command> {
        if let Some(i) = cx.focused_tab
            && modifiers.is_none()
            && cx.tab_count > 0
        {
            let last = cx.tab_count - 1;
            let to = match key {
                Key::ArrowLeft => Some(if i == 0 { last } else { i - 1 }),
                Key::ArrowRight => Some(if i >= last { 0 } else { i + 1 }),
                Key::Home => Some(0),
                Key::End => Some(last),
                _ => None,
            };
            if let Some(to) = to {
                return Some(Command::Tab(to));
            }
        }
        if cx.popup_open || !self.enabled {
            return None;
        }
        let pressed = Mods::pressed(modifiers, self.platform);
        for (action, binding) in &self.bindings {
            let Some(binding) = binding else { continue };
            if binding.mods != pressed {
                continue;
            }
            if *action == Action::GoToTab {
                if let Some(n) = digit(key)
                    && (1..=cx.tab_count).contains(&n)
                {
                    return Some(Command::Tab(n - 1));
                }
            } else if binding.key == key && (*action != Action::Cancel || cx.busy) {
                return Some(Command::Run(*action));
            }
        }
        None
    }

    /// The list shown by F1, grouped by where each control is.
    pub fn reference(&self, tab_names: &[&str]) -> String {
        let mut out = String::new();
        if !self.enabled {
            out.push_str("Keyboard shortcuts are turned off. Turn them on again on the Shortcuts tab.\n\n");
        }
        for place in Place::ALL {
            out.push_str(place.heading());
            out.push('\n');
            for action in Action::ALL.into_iter().filter(|a| a.place() == place) {
                let keys = match self.active(action) {
                    Some(s) if action == Action::GoToTab => {
                        format!("{} ({})", self.describe(action, s, tab_names.len()), tab_names.join(", "))
                    }
                    Some(s) => self.describe(action, s, tab_names.len()),
                    None => "none".to_owned(),
                };
                out.push_str(&format!("  {}: {keys}\n", action.label()));
            }
            out.push('\n');
        }
        let alt = if self.platform == Platform::Mac { "Option" } else { "Alt" };
        out.push_str("Shortcuts for another tab's controls go to that tab first.\n");
        out.push_str("In the episode list, Space ticks or unticks the episode with focus.\n");
        out.push_str(&format!(
            "In a list, the arrow keys change the choice, and {alt}+Down opens it.\n\n\
             Shortcuts can be changed or turned off on the Shortcuts tab."
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn keymap(platform: Platform) -> Keymap {
        Keymap::new(true, &BTreeMap::new(), platform)
    }

    fn cx() -> KeyContext {
        KeyContext { tab_count: 4, ..KeyContext::default() }
    }

    #[test]
    fn defaults_are_safe_unique_and_offered() {
        for p in Platform::ALL {
            let defaults: Vec<Shortcut> = Action::ALL.iter().filter_map(|a| a.default_shortcut(p)).collect();
            assert_eq!(defaults.len(), Action::ALL.len(), "every action has a shortcut on {p:?}");
            for s in &defaults {
                assert_eq!(check(*s, p), Ok(()), "{} on {p:?}", s.short(p));
            }
            for (i, s) in defaults.iter().enumerate() {
                assert!(!defaults[i + 1..].contains(s), "{} is used twice on {p:?}", s.short(p));
            }
            for action in Action::ALL {
                let default = action.default_shortcut(p).unwrap();
                assert!(action.choices(p).contains(&default), "{action:?} on {p:?} doesn't offer its default");
            }
        }
    }

    #[test]
    fn single_character_keys_are_never_choices() {
        for p in Platform::ALL {
            for action in Action::ALL {
                for s in action.choices(p) {
                    let typed = !s.mods.command && !s.mods.ctrl && !s.mods.alt;
                    assert!(!(typed && s.key.name().chars().count() == 1), "{} on {p:?}", s.short(p));
                    assert!(!s.mods.alt);
                }
            }
        }
    }

    #[test]
    fn mac_keeps_hide_and_minimise() {
        let p = Platform::Mac;
        assert_eq!(check(Shortcut::command(Key::H), p), Err(Conflict::Reserved));
        assert_eq!(check(Shortcut::command(Key::M), p), Err(Conflict::Reserved));
        assert_eq!(check(Shortcut::command(Key::H), Platform::Windows), Ok(()));
        assert_eq!(check(Shortcut::command(Key::C), Platform::Windows), Err(Conflict::Reserved));
    }

    #[test]
    fn saved_text_round_trips() {
        for p in Platform::ALL {
            for action in Action::ALL {
                for s in action.choices(p) {
                    assert_eq!(Shortcut::parse(&s.to_text(p), p), Some(s), "{} on {p:?}", s.to_text(p));
                }
            }
        }
    }

    #[test]
    fn saved_changes_apply_and_bad_ones_are_ignored() {
        let p = Platform::Windows;
        let mut saved = BTreeMap::new();
        saved.insert("download_selected".to_owned(), "F9".to_owned());
        saved.insert("open_folder".to_owned(), OFF.to_owned());
        // Not offered for anything.
        saved.insert("import_opml".to_owned(), "Ctrl+C".to_owned());
        // Already taken by Download selected, which comes first.
        saved.insert("browse".to_owned(), "F9".to_owned());
        let map = Keymap::new(true, &saved, p);
        assert_eq!(map.binding(Action::DownloadSelected), Some(Shortcut::key(Key::F9)));
        assert_eq!(map.binding(Action::OpenFolder), None);
        assert_eq!(map.binding(Action::ImportOpml), Action::ImportOpml.default_shortcut(p));
        assert_eq!(map.binding(Action::Browse), None);
    }

    #[test]
    fn keys_are_matched_exactly() {
        let altgr = Modifiers { alt: true, ctrl: true, command: true, ..Modifiers::NONE };
        let mut events = vec![
            press(Key::S, altgr),
            Event::Text("ś".into()),
            press(Key::S, Modifiers::COMMAND),
            press(Key::Num4, Modifiers::COMMAND),
            press(Key::F5, Modifiers::NONE),
        ];
        let commands = keymap(Platform::Windows).take(&mut events, cx());
        assert_eq!(
            commands,
            vec![Command::Run(Action::DownloadSelected), Command::Tab(3), Command::Run(Action::RefreshFeed)]
        );
        assert_eq!(events.len(), 2, "AltGr+S and its text are left for the controls");
    }

    #[test]
    fn turned_off_shortcuts_do_nothing() {
        let map = Keymap::new(false, &BTreeMap::new(), Platform::Windows);
        let mut events = vec![press(Key::S, Modifiers::COMMAND)];
        assert!(map.take(&mut events, cx()).is_empty());
        assert_eq!(events.len(), 1);
        assert_eq!(map.hint(Action::DownloadSelected), "");
    }

    #[test]
    fn escape_cancels_only_while_busy() {
        let map = keymap(Platform::Windows);
        let mut events = vec![press(Key::Escape, Modifiers::NONE)];
        assert!(map.take(&mut events, cx()).is_empty());
        assert!(events.is_empty(), "Escape is still kept from egui");
        let busy = KeyContext { busy: true, ..cx() };
        let mut events = vec![press(Key::Escape, Modifiers::NONE)];
        assert_eq!(map.take(&mut events, busy), vec![Command::Run(Action::Cancel)]);
        let open = KeyContext { popup_open: true, ..busy };
        let mut events = vec![press(Key::Escape, Modifiers::NONE)];
        assert_eq!(map.take(&mut events, open), vec![Command::CloseList]);
    }

    #[test]
    fn every_action_is_listed() {
        let text = keymap(Platform::Windows).reference(&["Feeds", "Episodes", "Settings", "Shortcuts"]);
        for action in Action::ALL {
            assert!(text.contains(action.label()), "{action:?}");
        }
    }
}
