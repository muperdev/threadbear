// What a runner's `hello` says about its logins, for the composer's chips and
// pickers: a port of RunnerLogins, Account, ModelChoice and Pool, and of what
// ThreadSettings, ModelPicker and Pools work out from them. Lenient, as the
// protocol's readers are: an unknown or missing field never fails.
use crate::composer::{effort_title, model_title, CODEX_EFFORT_LEVELS, EFFORT_LEVELS};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelChoice {
    pub id: String,
    pub model: String,
    pub title: String,
    pub description: Option<String>,
    pub effort_levels: Vec<String>,
    pub default_effort: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Account {
    pub id: String,
    pub provider: String,
    pub title: String,
    pub default_model: Option<String>,
    pub models: Vec<ModelChoice>,
    pub default_effort: Option<String>,
    pub email: Option<String>,
    pub note: Option<String>,
    /// Kept as it came (AccountUsage), for the badge a later slice draws.
    pub usage: Option<Value>,
}

impl Account {
    /// The account's provider, "claude" when none is given (ProviderInfo).
    pub fn provider_id(&self) -> &str {
        if self.provider.is_empty() {
            "claude"
        } else {
            &self.provider
        }
    }

    /// AccountName.title: the bare address when the runner sends one, else
    /// the title without Codex's "Codex · " prefix, else the id.
    pub fn name(&self) -> String {
        if let Some(email) = self.email.as_deref().filter(|e| !e.is_empty()) {
            return email.to_owned();
        }
        if let Some(rest) = self.title.strip_prefix("Codex · ") {
            return rest.to_owned();
        }
        if self.title.is_empty() {
            self.id.clone()
        } else {
            self.title.clone()
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Pool {
    pub id: String,
    pub title: String,
    pub account_ids: Vec<String>,
    pub icon: Option<String>,
    /// Folders whose threads start in this pool (`Pools.forFolder`).
    pub folders: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RunnerDefaults {
    pub cwd: String,
    pub account_id: String,
    pub pool_id: Option<String>,
    pub model: Option<String>,
    pub models: HashMap<String, String>,
    pub efforts: HashMap<String, String>,
    pub permission_mode: Option<String>,
}

impl RunnerDefaults {
    /// The model a new thread on `provider` gets when it names none.
    pub fn model_for(&self, provider: &str) -> Option<&str> {
        self.models.get(provider).map(String::as_str).or(if provider == "claude" { self.model.as_deref() } else { None })
    }
}

/// A runner's logins (RunnerLogins), filled from its `hello`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunnerLogins {
    pub accounts: Vec<Account>,
    pub pools: Vec<Pool>,
    pub defaults: RunnerDefaults,
}

impl RunnerLogins {
    pub fn from_hello(hello: &Value) -> RunnerLogins {
        let mut accounts: Vec<Account> = lossy(&hello["accounts"]);
        for account in &mut accounts {
            for model in &mut account.models {
                // `model` and `title` fall back to the id (ModelChoice.init).
                if model.model.is_empty() {
                    model.model = model.id.clone();
                }
                if model.title.is_empty() {
                    model.title = model.id.clone();
                }
            }
        }
        let mut pools: Vec<Pool> = lossy(&hello["pools"]);
        for pool in &mut pools {
            if pool.title.is_empty() {
                pool.title = pool.id.clone();
            }
        }
        RunnerLogins { accounts, pools, defaults: serde_json::from_value(hello["defaults"].clone()).unwrap_or_default() }
    }

    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn pool(&self, id: &str) -> Option<&Pool> {
        self.pools.iter().find(|p| p.id == id)
    }

    /// Whether pools are in use: the rail lists them, and the login is the
    /// runner's to pick unless the thread is pinned (ThreadSettings.usesPools).
    pub fn uses_pools(&self) -> bool {
        !self.pools.is_empty()
    }
}

fn lossy<T: serde::de::DeserializeOwned>(v: &Value) -> Vec<T> {
    v.as_array().map(|items| items.iter().filter_map(|item| serde_json::from_value(item.clone()).ok()).collect()).unwrap_or_default()
}

/// How a provider id reads (Providers.info).
pub fn provider_title(id: &str) -> String {
    match id {
        "claude" | "" => "Claude".to_owned(),
        "codex" => "Codex".to_owned(),
        other => {
            let mut chars = other.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        }
    }
}

/// Whether a provider's Auto mode is a real judge (Providers autoMode
/// "classifier"), or behaves like Ask.
pub fn has_auto_mode(provider: &str) -> bool {
    provider == "claude" || provider.is_empty()
}

/// A model's row in the picker (ModelPicker.Row).
#[derive(Clone, Debug, PartialEq)]
pub struct PickerRow {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub is_default: bool,
    pub is_selected: bool,
    pub is_older: bool,
}

/// ComposerChips.choice: the picker row an id belongs to.
pub fn choice_for<'a>(id: &str, choices: &'a [ModelChoice]) -> Option<&'a ModelChoice> {
    choices.iter().find(|c| c.id == id).or_else(|| choices.iter().find(|c| c.model == id))
}

/// ComposerChips.resolvedModel: the first id that's set.
pub fn resolved_model<'a>(ids: &[Option<&'a str>]) -> Option<&'a str> {
    ids.iter().find_map(|id| id.filter(|s| !s.is_empty()))
}

/// ComposerChips.modelTitle: the CLI's own title for an id when the list is
/// known, else as `model_title` formats it.
pub fn model_title_with(id: Option<&str>, choices: &[ModelChoice]) -> String {
    match id.filter(|s| !s.is_empty()) {
        None => "Default".to_owned(),
        Some(id) => choice_for(id, choices).map_or_else(|| model_title(Some(id)), |c| c.title.clone()),
    }
}

/// ComposerChips.chipTitle: "Fable 5.1 · High".
pub fn chip_title(model: Option<&str>, effort: Option<&str>, choices: &[ModelChoice]) -> String {
    let title = model_title_with(model, choices);
    match effort.filter(|e| !e.is_empty()) {
        Some(effort) => format!("{title} · {}", effort_title(effort)),
        None => title,
    }
}

/// ModelPicker.rows: one account's list. For Claude the main rows are the
/// aliases plus whatever the default and the chosen model are; dated and old
/// full ids are older. Codex ids are all dashed, so every model is a main row.
pub fn picker_rows(choices: &[ModelChoice], default_id: Option<&str>, selected: Option<&str>, query: &str, provider: &str) -> Vec<PickerRow> {
    let codex = provider != "claude" && !provider.is_empty();
    let fallback: Vec<ModelChoice> = if codex {
        default_id.filter(|d| !d.is_empty()).map(|d| vec![ModelChoice { id: d.into(), model: d.into(), title: model_title(Some(d)), ..Default::default() }]).unwrap_or_default()
    } else {
        ["opus", "sonnet", "haiku"].iter().map(|a| ModelChoice { id: (*a).into(), model: (*a).into(), title: model_title(Some(a)), ..Default::default() }).collect()
    };
    let known: &[ModelChoice] = if choices.is_empty() { &fallback } else { choices };
    let matches = |c: &ModelChoice, id: Option<&str>| id.filter(|s| !s.is_empty()).is_some_and(|id| c.id == id || c.model == id);
    let is_main = |c: &ModelChoice| codex || c.id.chars().all(char::is_alphabetic) || matches(c, default_id) || matches(c, selected);
    let mut rows: Vec<PickerRow> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |c: &ModelChoice, older: bool, rows: &mut Vec<PickerRow>| {
        if seen.insert(c.model.clone()) {
            rows.push(PickerRow { id: c.id.clone(), title: c.title.clone(), subtitle: c.description.clone(), is_default: matches(c, default_id), is_selected: matches(c, selected), is_older: older });
        }
    };
    for c in known.iter().filter(|c| is_main(c)) {
        add(c, false, &mut rows);
    }
    if let Some(selected) = selected.filter(|s| !s.is_empty()) {
        if !known.iter().any(|c| matches(c, Some(selected))) {
            rows.push(PickerRow { id: selected.to_owned(), title: model_title(Some(selected)), subtitle: None, is_default: false, is_selected: true, is_older: false });
        }
    }
    for c in known.iter().filter(|c| !is_main(c)) {
        add(c, true, &mut rows);
    }
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|r| r.title.to_lowercase().contains(&needle) || r.id.to_lowercase().contains(&needle) || r.subtitle.as_deref().is_some_and(|s| s.to_lowercase().contains(&needle)))
        .map(|mut r| {
            r.is_older = false;
            r
        })
        .collect()
}

/// ModelPicker.effortLevels: the levels the picker offers for a model.
pub fn effort_levels(model: Option<&str>, choices: &[ModelChoice], provider: &str) -> Vec<String> {
    let provider_levels = || -> Vec<String> {
        match provider {
            "codex" => CODEX_EFFORT_LEVELS.iter().map(|s| s.to_string()).collect(),
            "claude" | "" => EFFORT_LEVELS.iter().map(|s| s.to_string()).collect(),
            _ => Vec::new(),
        }
    };
    match model.and_then(|m| choice_for(m, choices)) {
        Some(choice) if !choice.effort_levels.is_empty() => choice.effort_levels.clone(),
        _ => provider_levels(),
    }
}

/// ModelPicker.impliedEffort: the level a thread with none chosen runs at.
pub fn implied_effort(model: Option<&str>, choices: &[ModelChoice], account: Option<&Account>, runner_default: Option<&str>) -> Option<String> {
    if let Some(d) = runner_default.filter(|d| !d.is_empty()) {
        return Some(d.to_owned());
    }
    if let Some(own) = account.and_then(|a| a.default_effort.as_deref()).filter(|d| !d.is_empty()) {
        return Some(own.to_owned());
    }
    model.and_then(|m| choice_for(m, choices)).and_then(|c| c.default_effort.clone())
}

/// What the composer's chips say about a thread, or the settings a new thread
/// starts with (ThreadSettings). All the strings the chips need, worked out
/// here so the tests can read them.
pub struct ChipSettings<'a> {
    pub logins: &'a RunnerLogins,
    pub account: String,
    pub provider: String,
    pub pool_id: Option<String>,
    pub model: Option<String>,
    pub model_in_use: Option<String>,
    pub effort: Option<String>,
    pub effort_in_use: Option<String>,
    pub permission_mode: String,
    pub cwd: String,
    /// The settings a new thread starts with, not a thread's own: its
    /// defaults come from the runner (ThreadSettings.defaultModel and
    /// effectiveEffort, which take `thread == nil` to mean this).
    pub is_new: bool,
}

impl<'a> ChipSettings<'a> {
    pub fn choices(&self) -> &'a [ModelChoice] {
        self.logins.account(&self.account).map_or(&[], |a| a.models.as_slice())
    }

    pub fn default_model(&self) -> Option<String> {
        let runner_default = if self.is_new { self.logins.defaults.model_for(&self.provider) } else { self.model_in_use.as_deref() };
        resolved_model(&[runner_default, self.logins.account(&self.account).and_then(|a| a.default_model.as_deref())]).map(str::to_owned)
    }

    pub fn effective_model(&self) -> Option<String> {
        resolved_model(&[self.model.as_deref(), self.default_model().as_deref()]).map(str::to_owned)
    }

    pub fn effective_effort(&self) -> Option<String> {
        let runner_default = if self.is_new { self.logins.defaults.efforts.get(&self.provider).map(String::as_str) } else { None };
        self.effort.clone().or_else(|| self.effort_in_use.clone()).or_else(|| implied_effort(self.effective_model().as_deref(), self.choices(), self.logins.account(&self.account), runner_default))
    }

    pub fn uses_pools(&self) -> bool {
        self.logins.uses_pools()
    }

    pub fn pool(&self) -> Option<&'a Pool> {
        if !self.uses_pools() {
            return None;
        }
        self.pool_id.as_deref().and_then(|id| self.logins.pool(id))
    }

    /// Who runs it, at the front of the model chip (ThreadSettings.owner):
    /// the pool's title, or a pinned login's name (`AccountName.login`). None
    /// without pools.
    pub fn owner(&self) -> Option<String> {
        if !self.uses_pools() {
            return None;
        }
        match self.pool() {
            Some(pool) => Some(pool.title.clone()),
            None => self.logins.account(&self.account).map(|_| crate::new_thread::login_name(&self.account, &self.logins.accounts)),
        }
    }

    /// The model chip (ThreadSettings.modelChipTitle): "Claude · Fable 5.1 · High".
    /// `ThreadSettings.modelChipBadge`: with pools, the pool's badge when
    /// every login in it is running out, or a pinned login's own.
    pub fn model_chip_badge(&self) -> Option<crate::new_thread::ChipBadge> {
        if !self.uses_pools() {
            return None;
        }
        match self.pool() {
            Some(pool) => crate::new_thread::pool_chip_badge(pool, &self.logins.accounts),
            None => crate::new_thread::chip_badge(self.logins.account(&self.account)),
        }
    }

    /// `ThreadSettings.accountBadge`: the account chip's "· 12%" or "Limit reached".
    pub fn account_badge(&self) -> Option<crate::new_thread::ChipBadge> {
        crate::new_thread::chip_badge(self.logins.account(&self.account))
    }

    pub fn model_chip_title(&self) -> String {
        let rest = chip_title(self.effective_model().as_deref(), self.effective_effort().as_deref(), self.choices());
        match self.owner().filter(|o| !o.is_empty()) {
            Some(owner) => format!("{owner} · {rest}"),
            None => rest,
        }
    }

    /// The chip's tooltip (ThreadSettings.modelChipHelp).
    pub fn model_chip_help(&self) -> String {
        let mut parts = Vec::new();
        if let Some(pool) = self.pool() {
            parts.push(format!("{} pool", pool.title));
        } else if self.uses_pools() {
            parts.push(format!("Pinned to {}", self.logins.account(&self.account).map_or_else(|| self.account.clone(), |a| a.name())));
        }
        if let Some(model) = &self.model {
            parts.push(model.clone());
        } else if let Some(default) = self.default_model() {
            parts.push(format!("{default}, the default"));
        } else {
            parts.push("The account's default model".to_owned());
        }
        if let Some(effort) = &self.effort {
            parts.push(format!("{} effort", effort_title(effort).to_lowercase()));
        } else if let Some(level) = &self.effort_in_use {
            parts.push(format!("{level} effort, the model's own level"));
        } else if let Some(level) = self.effective_effort() {
            parts.push(format!("{} effort, the default", effort_title(&level).to_lowercase()));
        } else {
            parts.push("the model's own effort level".to_owned());
        }
        parts.join(", ")
    }

    pub fn account_chip_title(&self) -> String {
        short_account_name(&self.account, &self.logins.accounts)
    }

    pub fn account_chip_help(&self) -> String {
        self.logins.account(&self.account).map_or_else(|| self.account.clone(), |a| match &a.note {
            Some(note) if !note.is_empty() => format!("{} ({note})", a.name()),
            _ => a.name(),
        })
    }

    pub fn permission_title(&self) -> &'static str {
        permission_title(&self.permission_mode)
    }

    pub fn permission_help(&self) -> &'static str {
        permission_help(&self.permission_mode)
    }

    /// The modes the permission menu offers: Auto needs a provider with its
    /// own judge of each action, or a thread already on it.
    pub fn permission_modes(&self) -> Vec<&'static str> {
        if has_auto_mode(&self.provider) || self.permission_mode == "auto" {
            vec!["full", "auto", "ask", "plan"]
        } else {
            vec!["full", "ask", "plan"]
        }
    }

    /// The model rows of a provider's column (ThreadSettings.rows).
    pub fn rows(&self, provider: &str, query: &str) -> Vec<PickerRow> {
        let own = provider == self.provider;
        let listed = if own { Some(self.account.as_str()) } else { self.logins.accounts.iter().find(|a| a.provider_id() == provider).map(|a| a.id.as_str()) };
        let Some(listed) = listed else { return picker_rows(&[], None, None, query, provider) };
        let account = self.logins.account(listed);
        let choices = account.map_or(&[][..], |a| a.models.as_slice());
        let default = if own { self.default_model() } else { self.logins.defaults.model_for(provider).map(str::to_owned).or_else(|| account.and_then(|a| a.default_model.clone())) };
        picker_rows(choices, default.as_deref(), if own { self.effective_model() } else { None }.as_deref(), query, provider)
    }

    pub fn effort_levels(&self) -> Vec<String> {
        effort_levels(self.effective_model().as_deref(), self.choices(), &self.provider)
    }
}

/// How a mode reads on the chip (PermissionMode.composerTitle).
pub fn permission_title(mode: &str) -> &'static str {
    match mode {
        "full" => "Full access",
        "ask" => "Ask before edits",
        "auto" => "Auto",
        "plan" => "Plan first",
        _ => "Unknown mode",
    }
}

pub fn permission_help(mode: &str) -> &'static str {
    match mode {
        "full" => "Edits files and runs commands without asking",
        "ask" => "Asks before editing files or running commands",
        "auto" => "A safety check approves or blocks each action, and asks only when unsure",
        "plan" => "Reads and plans, and changes nothing until you approve its plan",
        _ => "A mode from a newer runner that this app doesn't know. Update the app to see it. Anything made from this thread here asks before edits",
    }
}

/// AccountName.short: the part of a Claude email before the @, unless
/// another Claude account shares it; "Codex", or "Codex sam.lee" if there
/// are several.
pub fn short_account_name(id: &str, accounts: &[Account]) -> String {
    let Some(account) = accounts.iter().find(|a| a.id == id) else { return id.to_owned() };
    let full = account.name();
    let local = |s: &str| s.split('@').next().unwrap_or(s).to_owned();
    let siblings: Vec<&Account> = accounts.iter().filter(|a| a.id != id && a.provider_id() == account.provider_id()).collect();
    if account.provider_id() == "codex" {
        return if siblings.is_empty() { "Codex".to_owned() } else { format!("Codex {}", local(&full)) };
    }
    if !full.contains('@') {
        return full;
    }
    let mine = local(&full);
    if siblings.iter().any(|a| local(&a.name()) == mine) {
        full
    } else {
        mine
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn logins() -> RunnerLogins {
        RunnerLogins::from_hello(&json!({
            "accounts": [
                { "id": "claude-main", "provider": "claude", "email": "mike@example.com", "defaultModel": "claude-fable-5-1", "models": [
                    { "id": "opus", "model": "claude-opus-5-5", "title": "Opus 5.5", "description": "For complex work" },
                    { "id": "claude-fable-5-1", "title": "Fable 5.1" },
                    { "id": "claude-sonnet-4-5", "title": "Sonnet 4.5" }
                ]},
                { "id": "codex-main", "provider": "codex", "title": "Codex · sam@example.com", "models": [{ "id": "gpt-6.1-sol" }] }
            ],
            "pools": [{ "id": "claude", "title": "Claude", "accountIds": ["claude-main"] }],
            "defaults": { "cwd": "/x", "accountId": "claude-main", "models": { "codex": "gpt-6-luna" }, "efforts": { "codex": "low" } }
        }))
    }

    fn settings(l: &RunnerLogins) -> ChipSettings<'_> {
        ChipSettings {
            logins: l,
            account: "claude-main".into(),
            provider: "claude".into(),
            pool_id: Some("claude".into()),
            model: None,
            model_in_use: None,
            effort: Some("high".into()),
            effort_in_use: None,
            permission_mode: "full".into(),
            cwd: "/x".into(),
            is_new: false,
        }
    }

    #[test]
    fn the_chip_reads_pool_model_and_effort() {
        let l = logins();
        assert_eq!(settings(&l).model_chip_title(), "Claude · Fable 5.1 · High");
    }

    #[test]
    fn without_pools_the_chip_is_just_the_model() {
        let mut l = logins();
        l.pools.clear();
        assert_eq!(settings(&l).model_chip_title(), "Fable 5.1 · High");
    }

    #[test]
    fn rows_keep_older_ids_under_older() {
        let l = logins();
        let rows = settings(&l).rows("claude", "");
        let titles: Vec<(&str, bool, bool, bool)> = rows.iter().map(|r| (r.title.as_str(), r.is_default, r.is_selected, r.is_older)).collect();
        assert_eq!(titles, vec![("Opus 5.5", false, false, false), ("Fable 5.1", true, true, false), ("Sonnet 4.5", false, false, true)]);
    }

    #[test]
    fn the_search_finds_older_models_too() {
        let l = logins();
        let rows = settings(&l).rows("claude", "sonnet");
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].is_older);
    }

    #[test]
    fn another_providers_column_is_a_plain_list() {
        let l = logins();
        let rows = settings(&l).rows("codex", "");
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].is_selected);
    }

    #[test]
    fn permission_modes_offer_auto_only_where_it_exists() {
        let l = logins();
        assert_eq!(settings(&l).permission_modes(), vec!["full", "auto", "ask", "plan"]);
        let mut s = settings(&l);
        s.provider = "codex".into();
        assert_eq!(s.permission_modes(), vec!["full", "ask", "plan"]);
    }

    #[test]
    fn account_names() {
        let l = logins();
        assert_eq!(short_account_name("claude-main", &l.accounts), "mike");
        assert_eq!(short_account_name("codex-main", &l.accounts), "Codex");
        assert_eq!(l.account("claude-main").unwrap().name(), "mike@example.com");
        assert_eq!(l.account("codex-main").unwrap().name(), "sam@example.com");
    }
}
