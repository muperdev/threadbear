// The app's keyboard shortcuts, a port of ThreadbearCore's Shortcuts.swift: the
// keys in use are shortcuts.json's changes over the defaults, written
// "cmd+opt+down" so a person or an agent can read and change them there. The
// file is the Swift app's own: both apps read the same one.
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A key and the modifiers held with it (`ShortcutKey`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShortcutKey {
    pub key: String,
    pub command: bool,
    pub option: bool,
    pub control: bool,
    pub shift: bool,
}

/// Keys with a name, and what menus show for them.
const NAMED: [(&str, &str); 14] = [
    ("down", "↓"),
    ("up", "↑"),
    ("left", "←"),
    ("right", "→"),
    ("return", "↩"),
    ("tab", "⇥"),
    ("space", "Space"),
    ("delete", "⌫"),
    ("forwarddelete", "⌦"),
    ("escape", "Esc"),
    ("home", "Home"),
    ("end", "End"),
    ("pageup", "Page Up"),
    ("pagedown", "Page Down"),
];

fn is_named(key: &str) -> bool {
    NAMED.iter().any(|(n, _)| *n == key) || is_function_key(key)
}

fn is_function_key(key: &str) -> bool {
    key.len() > 1 && key.starts_with('f') && key[1..].chars().all(|c| c.is_ascii_digit())
}

impl ShortcutKey {
    pub fn new(key: &str) -> Self {
        Self { key: key.to_string(), command: true, option: false, control: false, shift: false }
    }

    pub fn with(key: &str, option: bool, control: bool, shift: bool) -> Self {
        Self { key: key.to_string(), command: true, option, control, shift }
    }

    /// "cmd+opt+down". None for anything that isn't a key (`init(string:)`).
    pub fn parse(string: &str) -> Option<Self> {
        let text = string.trim().to_lowercase();
        // "cmd++" is ⌘ and the plus sign, which a split alone would lose.
        let mut parts: Vec<String> = if let Some(rest) = text.strip_suffix("++") {
            let mut parts: Vec<String> = rest.split('+').map(String::from).collect();
            parts.push("+".into());
            parts
        } else {
            text.split('+').map(String::from).collect()
        };
        let last = parts.pop()?;
        if !(last.chars().count() == 1 || is_named(&last)) {
            return None;
        }
        let mut k = Self { key: last, command: false, option: false, control: false, shift: false };
        for part in parts {
            match part.as_str() {
                "cmd" | "command" => k.command = true,
                "opt" | "option" | "alt" => k.option = true,
                "ctrl" | "control" => k.control = true,
                "shift" => k.shift = true,
                _ => return None,
            }
        }
        Some(k)
    }

    /// As shortcuts.json writes it, always in the same order.
    pub fn string(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.command {
            parts.push("cmd");
        }
        if self.control {
            parts.push("ctrl");
        }
        if self.option {
            parts.push("opt");
        }
        if self.shift {
            parts.push("shift");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// As menus show it: "⌥⌘↓". ⌃ ⌥ ⇧ ⌘ is the order macOS menus show them.
    pub fn display(&self) -> String {
        let mods = format!("{}{}{}{}", if self.control { "⌃" } else { "" }, if self.option { "⌥" } else { "" }, if self.shift { "⇧" } else { "" }, if self.command { "⌘" } else { "" });
        let key = NAMED.iter().find(|(n, _)| *n == self.key).map(|(_, s)| s.to_string()).unwrap_or_else(|| self.key.to_uppercase());
        format!("{mods}{key}")
    }

    /// True for a press that's text, not a command (`typesInstead`).
    pub fn types_instead(&self) -> bool {
        !self.command && !self.control && !is_function_key(&self.key)
    }

    /// GPUI's name for the chord, as `KeyBinding::new` wants it: ⌘ on the Mac,
    /// Control elsewhere, so the shortcuts mean the same thing on every OS.
    pub fn chord(&self, macos: bool) -> String {
        let mut out = String::new();
        if self.control {
            out.push_str("ctrl-");
        }
        if self.option {
            out.push_str("alt-");
        }
        if self.shift {
            out.push_str("shift-");
        }
        if self.command {
            out.push_str(if macos { "cmd-" } else { "ctrl-" });
        }
        // GPUI's names: "enter", "backspace", "delete", "pageup"...
        let key = match self.key.as_str() {
            "return" => "enter",
            "delete" => "backspace",
            "forwarddelete" => "delete",
            "space" => "space",
            other => other,
        };
        out.push_str(key);
        out
    }
}

/// Everything in the menu bar that can have a key (`ShortcutCommand`). The raw
/// names are shortcuts.json's: leave them alone once they've shipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShortcutCommand {
    NewThread,
    Archive,
    Hibernate,
    Stop,
    CopyTranscript,
    Back,
    Forward,
    NextInboxThread,
    ShowInbox,
    ShowScheduled,
    ShowProjects,
    ShowLibrarian,
    ShowHibernating,
    ShowArchived,
    ZoomIn,
    ZoomOut,
    ActualSize,
    ToggleThreads,
    TogglePanels,
    ToggleTerminal,
    ResetPanelLayout,
    Settings,
}

impl ShortcutCommand {
    pub const ALL: [ShortcutCommand; 22] = [
        Self::NewThread,
        Self::Archive,
        Self::Hibernate,
        Self::Stop,
        Self::CopyTranscript,
        Self::Back,
        Self::Forward,
        Self::NextInboxThread,
        Self::ShowInbox,
        Self::ShowScheduled,
        Self::ShowProjects,
        Self::ShowLibrarian,
        Self::ShowHibernating,
        Self::ShowArchived,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ActualSize,
        Self::ToggleThreads,
        Self::TogglePanels,
        Self::ToggleTerminal,
        Self::ResetPanelLayout,
        Self::Settings,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::NewThread => "newThread",
            Self::Archive => "archive",
            Self::Hibernate => "hibernate",
            Self::Stop => "stop",
            Self::CopyTranscript => "copyTranscript",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::NextInboxThread => "nextInboxThread",
            Self::ShowInbox => "showInbox",
            Self::ShowScheduled => "showScheduled",
            Self::ShowProjects => "showProjects",
            Self::ShowLibrarian => "showLibrarian",
            Self::ShowHibernating => "showHibernating",
            Self::ShowArchived => "showArchived",
            Self::ZoomIn => "zoomIn",
            Self::ZoomOut => "zoomOut",
            Self::ActualSize => "actualSize",
            Self::ToggleThreads => "toggleThreads",
            Self::TogglePanels => "togglePanels",
            Self::ToggleTerminal => "toggleTerminal",
            Self::ResetPanelLayout => "resetPanelLayout",
            Self::Settings => "settings",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.name() == name)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::NewThread => "New Thread",
            Self::Archive => "Archive",
            Self::Hibernate => "Hibernate",
            Self::Stop => "Stop",
            Self::CopyTranscript => "Copy Transcript as Markdown",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::NextInboxThread => "Next Inbox Thread",
            Self::ShowInbox => "Show Inbox",
            Self::ShowScheduled => "Show Scheduled",
            Self::ShowProjects => "Show Projects",
            Self::ShowLibrarian => "Show Librarian",
            Self::ShowHibernating => "Show Hibernating",
            Self::ShowArchived => "Show Archived",
            Self::ZoomIn => "Zoom In",
            Self::ZoomOut => "Zoom Out",
            Self::ActualSize => "Actual Size",
            Self::ToggleThreads => "Show Threads",
            Self::TogglePanels => "Show Panels",
            Self::ToggleTerminal => "Show Terminal",
            Self::ResetPanelLayout => "Reset Panel Layout",
            Self::Settings => "Settings…",
        }
    }

    pub fn default_key(self) -> Option<ShortcutKey> {
        match self {
            Self::NewThread => Some(ShortcutKey::new("n")),
            Self::Settings => Some(ShortcutKey::new(",")),
            Self::Archive => Some(ShortcutKey::new("e")),
            Self::Stop => Some(ShortcutKey::new(".")),
            Self::CopyTranscript => Some(ShortcutKey::with("c", false, false, true)),
            Self::Back => Some(ShortcutKey::new("[")),
            Self::Forward => Some(ShortcutKey::new("]")),
            Self::NextInboxThread => Some(ShortcutKey::with("down", true, false, false)),
            Self::ShowInbox => Some(ShortcutKey::new("1")),
            Self::ShowScheduled => Some(ShortcutKey::new("2")),
            Self::ShowProjects => Some(ShortcutKey::new("3")),
            Self::ShowLibrarian => Some(ShortcutKey::new("4")),
            // ⌘= is the key under the + sign, so ⌘+ works without shift.
            Self::ZoomIn => Some(ShortcutKey::new("=")),
            Self::ZoomOut => Some(ShortcutKey::new("-")),
            Self::ActualSize => Some(ShortcutKey::new("0")),
            Self::ToggleThreads => Some(ShortcutKey::with("s", false, true, false)),
            Self::TogglePanels => Some(ShortcutKey::with("p", true, false, false)),
            // VS Code's and T3 Code's key for their terminal.
            Self::ToggleTerminal => Some(ShortcutKey::new("j")),
            Self::Hibernate | Self::ShowHibernating | Self::ShowArchived | Self::ResetPanelLayout => None,
        }
    }
}

/// The keys in use: the defaults, with whatever's been changed on top. Only the
/// changes are kept, so a default that changes in an update reaches everyone
/// who hasn't picked their own (`ShortcutBindings`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShortcutBindings {
    changes: BTreeMap<ShortcutCommand, Option<ShortcutKey>>,
    /// Entries for commands this build doesn't have, kept so saving from here
    /// doesn't lose them. `None` for "no key".
    others: BTreeMap<String, Option<String>>,
    /// Lines of the file that couldn't be used, to show on the page.
    pub problems: Vec<String>,
}

impl ShortcutBindings {
    /// Reads shortcuts.json. An error means it isn't a JSON object at all, so
    /// the keys in use stay as they are through an agent's half-done edit; a bad
    /// line only loses that line.
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let Value::Object(map) = value else { return Err("not an object".into()) };
        let mut out = Self::default();
        for (name, value) in map {
            let Some(command) = ShortcutCommand::from_name(&name) else {
                match value {
                    Value::Null => {
                        out.others.insert(name, None);
                    }
                    Value::String(text) => {
                        out.others.insert(name, Some(text));
                    }
                    _ => {}
                }
                continue;
            };
            match value {
                Value::Null => out.set(command, None),
                Value::String(text) => match ShortcutKey::parse(&text).filter(|k| !k.types_instead()) {
                    Some(key) => out.set(command, Some(key)),
                    None => out.problems.push(format!("{name}: {text}")),
                },
                other => out.problems.push(format!("{name}: {other}")),
            }
        }
        Ok(out)
    }

    /// What goes in shortcuts.json: only what's changed. None when nothing is,
    /// so the file can go.
    pub fn json(&self) -> Option<String> {
        if self.changes.is_empty() && self.others.is_empty() {
            return None;
        }
        let mut map = Map::new();
        for (command, key) in &self.changes {
            map.insert(command.name().to_string(), key.as_ref().map(|k| Value::String(k.string())).unwrap_or(Value::Null));
        }
        for (name, value) in &self.others {
            map.insert(name.clone(), value.clone().map(Value::String).unwrap_or(Value::Null));
        }
        serde_json::to_string_pretty(&Value::Object(map)).ok()
    }

    pub fn key(&self, command: ShortcutCommand) -> Option<ShortcutKey> {
        match self.changes.get(&command) {
            Some(change) => change.clone(),
            None => command.default_key(),
        }
    }

    pub fn is_changed(&self, command: ShortcutCommand) -> bool {
        self.changes.contains_key(&command)
    }

    pub fn set(&mut self, command: ShortcutCommand, key: Option<ShortcutKey>) {
        if key == command.default_key() {
            self.changes.remove(&command);
        } else {
            self.changes.insert(command, key);
        }
    }

    /// The command on this key, other than `except`.
    pub fn command_using(&self, key: &ShortcutKey, except: Option<ShortcutCommand>) -> Option<ShortcutCommand> {
        ShortcutCommand::ALL.iter().copied().find(|c| Some(*c) != except && self.key(*c).as_ref() == Some(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> ShortcutKey {
        ShortcutKey::parse(text).unwrap()
    }

    #[test]
    fn a_key_reads_and_writes_the_way_the_file_says_it() {
        assert_eq!(ShortcutKey::new("n"), key("cmd+n"));
        assert_eq!(key("cmd+opt+down").string(), "cmd+opt+down");
        assert_eq!(key("shift+cmd+c").string(), "cmd+shift+c", "the same order however it was written");
        assert_eq!(key("CTRL+J").string(), "ctrl+j");
        assert_eq!(key("alt+cmd+p"), key("opt+cmd+p"));
        assert_eq!(key("cmd++").key, "+", "a plus sign is a key too");
    }

    #[test]
    fn nonsense_isnt_a_key() {
        for text in ["", "cmd", "cmd+", "banana+n", "cmd+banana", "cmd+n+m", "+"] {
            assert!(ShortcutKey::parse(text).is_none(), "{text}");
        }
    }

    #[test]
    fn a_key_shows_the_way_macos_menus_do() {
        assert_eq!(key("cmd+n").display(), "⌘N");
        assert_eq!(key("cmd+opt+down").display(), "⌥⌘↓");
        assert_eq!(key("cmd+shift+c").display(), "⇧⌘C");
        assert_eq!(key("ctrl+cmd+s").display(), "⌃⌘S");
        assert_eq!(key("cmd+.").display(), "⌘.");
        assert_eq!(key("cmd+[").display(), "⌘[");
        assert_eq!(key("ctrl+return").display(), "⌃↩");
        assert_eq!(key("f5").display(), "F5");
    }

    #[test]
    fn a_key_has_to_be_more_than_something_youd_type() {
        assert!(!key("cmd+e").types_instead());
        assert!(!key("ctrl+e").types_instead());
        assert!(!key("f5").types_instead(), "function keys don't type");
        let mut typed = ShortcutKey::new("e");
        typed.command = false;
        assert!(typed.types_instead());
    }

    #[test]
    fn a_chord_means_the_same_on_every_os() {
        assert_eq!(key("cmd+e").chord(true), "cmd-e");
        assert_eq!(key("cmd+e").chord(false), "ctrl-e");
        assert_eq!(key("cmd+opt+down").chord(true), "alt-cmd-down");
        assert_eq!(key("cmd+return").chord(true), "cmd-enter");
    }

    #[test]
    fn the_defaults_are_the_twenty_two_in_the_menus() {
        let keys: Vec<ShortcutKey> = ShortcutCommand::ALL.iter().filter_map(|c| c.default_key()).collect();
        let unique: std::collections::HashSet<&ShortcutKey> = keys.iter().collect();
        assert_eq!(keys.len(), unique.len(), "two commands can't start on one key");
        assert_eq!(ShortcutCommand::ALL.len(), 22);
    }

    #[test]
    fn the_file_holds_only_whats_changed() {
        let mut b = ShortcutBindings::default();
        assert_eq!(b.json(), None);
        b.set(ShortcutCommand::Archive, Some(key("cmd+k")));
        b.set(ShortcutCommand::Stop, None);
        let text = b.json().unwrap();
        let read = ShortcutBindings::parse(&text).unwrap();
        assert_eq!(read.key(ShortcutCommand::Archive), Some(key("cmd+k")));
        assert_eq!(read.key(ShortcutCommand::Stop), None);
        assert_eq!(read.key(ShortcutCommand::NewThread), Some(key("cmd+n")), "the rest stay at their defaults");
        // Setting it back to the default is no change at all.
        let mut back = read.clone();
        back.set(ShortcutCommand::Archive, Some(key("cmd+e")));
        assert!(!back.is_changed(ShortcutCommand::Archive));
    }

    #[test]
    fn a_half_done_edit_keeps_the_keys_in_use() {
        assert!(ShortcutBindings::parse("{ not json").is_err());
        assert!(ShortcutBindings::parse("[1, 2]").is_err());
        let read = ShortcutBindings::parse("{\"archive\": \"banana\", \"newThread\": \"cmd+t\"}").unwrap();
        assert_eq!(read.problems.len(), 1, "the bad line only loses that line");
        assert_eq!(read.key(ShortcutCommand::NewThread), Some(key("cmd+t")));
        assert_eq!(read.key(ShortcutCommand::Archive), Some(key("cmd+e")));
    }

    #[test]
    fn unknown_commands_stay_in_the_file() {
        let read = ShortcutBindings::parse("{\"somethingNewer\": \"cmd+z\"}").unwrap();
        assert!(read.json().unwrap().contains("somethingNewer"));
    }
}
