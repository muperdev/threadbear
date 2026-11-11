// The slash command palette's rules: a port of ThreadbearCore/Threads/SlashCommands.swift
// (`SlashPalette`) and the key handling of SlashPaletteView.swift
// (`SlashPaletteView.handle`). The view that draws them is desktop/gpui/src/slash_palette.rs.
use serde::Deserialize;

/// A command the palette offers, as `threads.commands` lists it
/// (docs/protocol.md, "Slash commands").
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SlashCommand {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    /// "threadbear", "cli" or "skill"; anything newer counts as the CLI's.
    pub source: String,
    /// Other names that run it (/usage is /cost and /stats too).
    pub aliases: Option<Vec<String>>,
    /// Where it was defined: "user", "project", "plugin" (with `plugin`),
    /// "claude.ai", "system", or one this app doesn't know.
    pub scope: Option<String>,
    pub plugin: Option<String>,
    /// Its file on the runner's machine, when the runner found it.
    pub path: Option<String>,
}

impl SlashCommand {
    /// What to type, with its arguments: "/code-review [low|medium|high] [target]".
    pub fn usage(&self) -> String {
        match self.argument_hint.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
            Some(hint) => format!("/{} {hint}", self.name),
            None => format!("/{}", self.name),
        }
    }

    pub fn is_skill(&self) -> bool {
        self.source == "skill"
    }

    pub fn is_threadbear(&self) -> bool {
        self.source == "threadbear"
    }
}

/// `threads.commands`' answer, read leniently: a malformed entry is left out.
pub fn decode_commands(value: &serde_json::Value) -> Vec<SlashCommand> {
    value
        .get("commands")
        .and_then(|c| c.as_array())
        .map(|list| list.iter().filter_map(|c| serde_json::from_value::<SlashCommand>(c.clone()).ok()).filter(|c| !c.name.is_empty()).collect())
        .unwrap_or_default()
}

/// The palette's categories (`SlashPalette.Category`), in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    All,
    Threadbear,
    Cli,
    Skills,
    Recent,
}

impl Category {
    pub const ALL: [Category; 5] = [Category::All, Category::Threadbear, Category::Cli, Category::Skills, Category::Recent];

    /// The CLI's own are named after it ("Claude Code", "Codex").
    pub fn title(self, provider: &str) -> String {
        match self {
            Category::All => "All".into(),
            Category::Threadbear => "Threadbear".into(),
            Category::Cli => cli_name(provider),
            Category::Skills => "Skills".into(),
            Category::Recent => "Recent".into(),
        }
    }

    /// The category's tooltip (`SlashPaletteView.categoryTip`).
    pub fn tip(self, provider: &str) -> String {
        match self {
            Category::All => "Every command this thread can run".into(),
            Category::Threadbear => "Threadbear's own, which change this thread's settings".into(),
            Category::Cli => format!("{}'s own commands", cli_name(provider)),
            Category::Skills => "Your skills and this folder's".into(),
            Category::Recent => "The ones you picked lately".into(),
        }
    }
}

/// `Providers.info(provider).cliName`.
pub fn cli_name(provider: &str) -> String {
    match provider {
        "claude" | "" => "Claude Code".to_owned(),
        "codex" => "Codex".to_owned(),
        other => {
            let mut chars = other.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        }
    }
}

/// The command being typed, without its slash, while the message is only
/// that: "/", "/co". None once there's a space (its arguments), a second
/// line, or a second slash (a path, "/Users/").
pub fn query(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('/')?;
    (!rest.chars().any(|c| c.is_whitespace() || c == '/')).then_some(rest)
}

/// Where a command came from, for the detail pane.
pub fn origin(command: &SlashCommand, provider: &str) -> String {
    if command.is_threadbear() {
        return "Threadbear's own: it changes this thread's settings".to_owned();
    }
    let kind = match command.path.as_deref() {
        Some(path) => {
            if path.ends_with("/SKILL.md") {
                "skill"
            } else {
                "command"
            }
        }
        None => {
            if command.is_skill() {
                "skill"
            } else {
                "command"
            }
        }
    };
    match command.scope.as_deref() {
        Some("user") => format!("Your {kind}"),
        Some("project") | Some("repo") => format!("This folder's {kind}"),
        Some("plugin") => match command.plugin.as_deref() {
            Some(plugin) => format!("From the {plugin} plugin"),
            None => "From a plugin".to_owned(),
        },
        Some("claude.ai") => "Synced from claude.ai".to_owned(),
        Some("admin") => "Set up by your organisation".to_owned(),
        Some("system") => format!("{}'s own", cli_name(provider)),
        _ => {
            if command.is_skill() {
                "A skill".to_owned()
            } else {
                format!("{}'s own", cli_name(provider))
            }
        }
    }
}

/// A command's description as it should read, on the one paragraph it came
/// as: skills write theirs with inline markdown (`SlashPalette.description`),
/// so its marks come off. Links keep their words and stop being links, as a
/// description comes from whoever wrote the skill (a `file:` link in a
/// palette once ran a script); anything that isn't markdown stays as written.
pub fn plain_description(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '`' | '*' => {}
            // Emphasis, but not the middle of a word (snake_case).
            '_' if text[..i].chars().next_back().is_none_or(|p| !p.is_alphanumeric()) || text[i + 1..].chars().next().is_none_or(|n| !n.is_alphanumeric()) => {}
            '[' => {
                // [words](url): keep the words if the brackets close and a
                // link follows.
                let rest = &text[i + 1..];
                if let Some(close) = rest.find(']') {
                    if rest[close + 1..].starts_with('(') && rest[close + 1..].contains(')') {
                        out.push_str(&rest[..close]);
                        let after = close + 1 + rest[close + 1..].find(')').unwrap_or(0);
                        for _ in 0..rest[..=after].chars().count() {
                            chars.next();
                        }
                        continue;
                    }
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// A path to show: the home folder as ~.
pub fn short_path(path: &str, home: &str) -> String {
    if !home.is_empty() && (path == home || path.starts_with(&format!("{home}/"))) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_owned()
    }
}

pub fn in_category(command: &SlashCommand, category: Category, recent: &[String]) -> bool {
    match category {
        Category::All => true,
        Category::Threadbear => command.is_threadbear(),
        Category::Cli => !command.is_threadbear() && !command.is_skill(),
        Category::Skills => command.is_skill(),
        Category::Recent => recent.iter().any(|r| r == &command.name),
    }
}

/// The matches for what's typed: names that start with it first, then names
/// that contain it, then ones whose description does, each in the runner's
/// order. Recent is in the order they were used.
pub fn matches(commands: &[SlashCommand], query: &str, category: Category, recent: &[String]) -> Vec<SlashCommand> {
    let q = query.to_lowercase();
    let rank = |c: &SlashCommand| -> Option<u8> {
        let name = c.name.to_lowercase();
        if q.is_empty() || name.starts_with(&q) {
            Some(0)
        } else if name.contains(&q) {
            Some(1)
        } else if c.description.as_deref().is_some_and(|d| d.to_lowercase().contains(&q)) {
            Some(2)
        } else {
            None
        }
    };
    let mut ranked: Vec<(u8, usize, &SlashCommand)> = commands
        .iter()
        .filter(|c| in_category(c, category, recent))
        .enumerate()
        .filter_map(|(i, c)| rank(c).map(|r| (r, i, c)))
        .map(|(r, i, c)| if category == Category::Recent { (r, recent.iter().position(|n| n == &c.name).unwrap_or(i), c) } else { (r, i, c) })
        .collect();
    ranked.sort_by_key(|(r, i, _)| (*r, *i));
    ranked.into_iter().map(|(_, _, c)| c.clone()).collect()
}

/// How many each category has for what's typed. Recent is left out when
/// nothing's been used yet: a thread with none has no Recent to click.
pub fn counts(commands: &[SlashCommand], query: &str, recent: &[String]) -> Vec<(Category, usize)> {
    Category::ALL
        .iter()
        .filter(|cat| !(**cat == Category::Recent && recent.is_empty()))
        .map(|cat| (*cat, matches(commands, query, *cat, recent).len()))
        .collect()
}

/// The text a pick leaves in the box: the command, and a space when its
/// arguments are to be typed next.
pub fn fill(command: &SlashCommand, for_arguments: bool) -> String {
    if for_arguments {
        format!("/{} ", command.name)
    } else {
        format!("/{}", command.name)
    }
}

/// The recently used list after using `name`: newest first, eight kept.
pub fn used(name: &str, recent: &[String]) -> Vec<String> {
    let mut list = vec![name.to_owned()];
    list.extend(recent.iter().filter(|r| r.as_str() != name).cloned());
    list.truncate(8);
    list
}

/// The categories' and the list's widths for a palette `total` wide
/// (`SlashPalette.columnWidths`), from what each needs to show its longest
/// entry whole. The detail gets the rest, never less than `detail_min`.
/// `chosen`: Mike dragged the list to that width, so it isn't held to half of
/// what's left (only to leaving the detail its least).
pub fn column_widths(total: f64, categories: f64, list: f64, chosen: bool, detail_min: f64, list_min: f64, categories_min: f64) -> (f64, f64) {
    let mut cats = categories;
    let mut lst = if chosen { list.max(list_min) } else { list.min(list_min.max((total - cats) / 2.)) };
    let over = cats + lst + detail_min - total;
    if over > 0. {
        lst = list_min.max(lst - over);
    }
    let still = cats + lst + detail_min - total;
    if still > 0. {
        cats = categories_min.max(cats - still);
    }
    (cats, lst)
}

/// The palette's height for a window `window` tall: about half of it, at
/// least `least` and at most `most`.
pub fn palette_height(window: f64, least: f64, most: f64) -> f64 {
    most.min(least.max(window * 0.5))
}

/// The keys the palette takes from the composer while it's open
/// (`PaletteKey`); the cursor stays in the box, so typing keeps narrowing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKey {
    Up,
    Down,
    Tab,
    Enter,
    Escape,
}

/// What the composer keeps for its palette (`PaletteState`): which category
/// and command are picked, and the text it was closed at with Esc (it stays
/// closed until the text changes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaletteState {
    pub category: Category,
    pub selected: Option<String>,
    pub closed_at: Option<String>,
}

/// What a palette key does (`SlashPaletteView.handle`): moves the pick, or
/// gives the command to fill in (Tab) or run (Return). `None`: nothing more
/// to do, the state changed or the key wasn't one that picks.
pub fn handle_key(key: PaletteKey, matches: &[SlashCommand], state: &mut PaletteState, text: &str) -> Option<(SlashCommand, bool)> {
    let index = matches.iter().position(|c| Some(c.name.as_str()) == state.selected.as_deref()).unwrap_or(0);
    match key {
        PaletteKey::Up => {
            if !matches.is_empty() {
                state.selected = Some(matches[index.saturating_sub(1)].name.clone());
            }
            None
        }
        PaletteKey::Down => {
            if !matches.is_empty() {
                state.selected = Some(matches[(index + 1).min(matches.len() - 1)].name.clone());
            }
            None
        }
        PaletteKey::Escape => {
            state.closed_at = Some(text.to_owned());
            None
        }
        PaletteKey::Tab | PaletteKey::Enter => matches.get(index).map(|c| (c.clone(), key == PaletteKey::Enter)),
    }
}

/// The picked command: the one selected if it's still listed, else the first.
pub fn picked<'a>(matches: &'a [SlashCommand], state: &PaletteState) -> Option<&'a SlashCommand> {
    matches.iter().find(|c| Some(c.name.as_str()) == state.selected.as_deref()).or_else(|| matches.first())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn commands() -> Vec<SlashCommand> {
        decode_commands(&json!({ "commands": [
            { "name": "model", "description": "Switch this thread's model", "argumentHint": "[model]", "source": "threadbear" },
            { "name": "clear", "description": "Clear conversation history", "source": "cli" },
            { "name": "compact", "description": "Clear but keep a summary", "argumentHint": "<optional instructions>", "source": "cli" },
            { "name": "context", "description": "Show current context usage", "source": "cli" },
            { "name": "cost", "description": "Show the total cost", "source": "cli", "scope": "system", "aliases": ["usage", "stats"] },
            { "name": "code-review", "description": "Review the current diff", "argumentHint": "[low|medium|high] [target]", "source": "skill", "scope": "user", "path": "/home/mike/.claude/skills/code-review/SKILL.md" },
            { "name": "deslop", "description": "A second-pass cleanup", "source": "skill", "scope": "project", "path": "/x/.claude/skills/deslop/SKILL.md" },
            { "name": "ship", "description": "Drive a branch to merge-ready", "source": "skill" },
            { "bad": true }
        ]}))
    }

    #[test]
    fn decoding_is_lenient_about_bad_entries() {
        let list = commands();
        assert_eq!(list.len(), 8, "the entry with no name is left out");
        assert_eq!(list[0].usage(), "/model [model]");
        assert_eq!(list[1].usage(), "/clear");
    }

    #[test]
    fn the_query_is_only_while_the_message_is_just_the_command() {
        assert_eq!(query("/"), Some(""));
        assert_eq!(query("/co"), Some("co"));
        assert_eq!(query("/code-review big"), None);
        assert_eq!(query("hello"), None);
        assert_eq!(query("/Users/mike"), None);
        assert_eq!(query("/a\nb"), None);
    }

    #[test]
    fn names_that_start_with_it_come_first() {
        let rows = matches(&commands(), "co", Category::All, &[]);
        let names: Vec<&str> = rows.iter().map(|c| c.name.as_str()).collect();
        // Clear's and deslop's descriptions have "co" in them, so they follow.
        assert_eq!(names, vec!["compact", "context", "cost", "code-review", "clear", "deslop"]);
    }

    #[test]
    fn then_names_that_contain_it_then_descriptions() {
        let rows = matches(&commands(), "de", Category::All, &[]);
        let names: Vec<&str> = rows.iter().map(|c| c.name.as_str()).collect();
        // deslop starts with it; model and code-review have it inside their
        // names, in the runner's order.
        assert_eq!(names, vec!["deslop", "model", "code-review"]);
    }

    #[test]
    fn categories_filter_and_count() {
        let all = commands();
        let recent = vec!["ship".to_owned()];
        let counts = counts(&all, "", &recent);
        let by_title: Vec<(String, usize)> = counts.iter().map(|(c, n)| (c.title("claude"), *n)).collect();
        assert_eq!(by_title, vec![("All".into(), 8), ("Threadbear".into(), 1), ("Claude Code".into(), 4), ("Skills".into(), 3), ("Recent".into(), 1)]);
    }

    #[test]
    fn recent_only_appears_once_something_was_used() {
        let counts_none = counts(&commands(), "", &[]);
        assert!(counts_none.iter().all(|(c, _)| *c != Category::Recent));
    }

    #[test]
    fn recent_is_in_the_order_they_were_used() {
        let recent = vec!["ship".to_owned(), "model".to_owned()];
        let rows = matches(&commands(), "", Category::Recent, &recent);
        let names: Vec<&str> = rows.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["ship", "model"]);
    }

    #[test]
    fn a_pick_fills_with_a_space_for_arguments() {
        let c = &commands()[1];
        assert_eq!(fill(c, true), "/clear ");
        assert_eq!(fill(c, false), "/clear");
    }

    #[test]
    fn used_puts_the_newest_first_and_keeps_eight() {
        let recent: Vec<String> = (0..9).map(|i| format!("c{i}")).collect();
        let list = used("c3", &recent);
        assert_eq!(list[0], "c3");
        assert_eq!(list.len(), 8);
        assert_eq!(list.iter().filter(|c| c.as_str() == "c3").count(), 1);
    }

    #[test]
    fn origins_say_where_a_command_is_from() {
        let all = commands();
        assert_eq!(origin(&all[0], "claude"), "Threadbear's own: it changes this thread's settings");
        assert_eq!(origin(&all[1], "claude"), "Claude Code's own");
        assert_eq!(origin(&all[1], "codex"), "Codex's own");
        assert_eq!(origin(&all[5], "claude"), "Your skill");
        assert_eq!(origin(&all[6], "claude"), "This folder's skill");
        assert_eq!(origin(&all[7], "claude"), "A skill");
    }

    #[test]
    fn descriptions_lose_their_marks_and_links() {
        assert_eq!(plain_description("Drive Mac apps with the `computercub` command."), "Drive Mac apps with the computercub command.");
        assert_eq!(plain_description("**Bold** and *italic* and _emphasis_."), "Bold and italic and emphasis.");
        assert_eq!(plain_description("See [the docs](https://example.com/x) for more."), "See the docs for more.");
        assert_eq!(plain_description("A [link](file:///bin/sh) and snake_case stays"), "A link and snake_case stays");
        // Not links, as written.
        assert_eq!(plain_description("[not a link] and a stray ] bracket"), "[not a link] and a stray ] bracket");
    }

    #[test]
    fn short_paths_use_a_tilde() {
        assert_eq!(short_path("/home/mike/.claude/x", "/home/mike"), "~/.claude/x");
        assert_eq!(short_path("/opt/x", "/home/mike"), "/opt/x");
    }

    #[test]
    fn the_columns_keep_the_detail_readable() {
        // Wide: each column as it wants.
        let (cats, list) = column_widths(780., 160., 220., false, 200., 120., 110.);
        assert_eq!((cats, list), (160., 220.));
        // Narrow: the list gives way first, then the categories.
        let (cats, list) = column_widths(420., 160., 300., false, 200., 120., 110.);
        assert_eq!(list, 120.);
        assert_eq!(cats, 110.);
    }

    #[test]
    fn the_palette_is_about_half_the_window() {
        assert_eq!(palette_height(800., 300., 560.), 400.);
        assert_eq!(palette_height(300., 300., 560.), 300.);
        assert_eq!(palette_height(2000., 300., 560.), 560.);
    }

    #[test]
    fn keys_move_the_pick_and_stop_at_the_ends() {
        let all = commands();
        let list = matches(&all, "", Category::All, &[]);
        let mut state = PaletteState::default();
        assert_eq!(handle_key(PaletteKey::Down, &list, &mut state, "/"), None);
        assert_eq!(state.selected.as_deref(), Some("clear"));
        assert_eq!(handle_key(PaletteKey::Up, &list, &mut state, "/"), None);
        assert_eq!(state.selected.as_deref(), Some("model"));
        // Already at the top.
        assert_eq!(handle_key(PaletteKey::Up, &list, &mut state, "/"), None);
        assert_eq!(state.selected.as_deref(), Some("model"));
    }

    #[test]
    fn tab_fills_and_return_runs() {
        let all = commands();
        let list = matches(&all, "cl", Category::All, &[]);
        let mut state = PaletteState::default();
        let (cmd, run) = handle_key(PaletteKey::Tab, &list, &mut state, "/cl").unwrap();
        assert_eq!((cmd.name.as_str(), run), ("clear", false));
        let (cmd, run) = handle_key(PaletteKey::Enter, &list, &mut state, "/cl").unwrap();
        assert_eq!((cmd.name.as_str(), run), ("clear", true));
    }

    #[test]
    fn escape_closes_it_at_this_text() {
        let list = matches(&commands(), "", Category::All, &[]);
        let mut state = PaletteState::default();
        assert_eq!(handle_key(PaletteKey::Escape, &list, &mut state, "/mo"), None);
        assert_eq!(state.closed_at.as_deref(), Some("/mo"));
    }

    #[test]
    fn the_pick_falls_back_to_the_first_row() {
        let list = matches(&commands(), "", Category::All, &[]);
        let mut state = PaletteState { selected: Some("nope".to_owned()), ..Default::default() };
        assert_eq!(picked(&list, &state).unwrap().name, "model");
        state.selected = Some("ship".to_owned());
        assert_eq!(picked(&list, &state).unwrap().name, "ship");
    }
}
