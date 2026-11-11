// Folded Highlights headings, remembered across launches: the Swift app keeps
// them in UserDefaults (`collapsedHighlights`); here a small file in the data
// folder, which is per user and per runner like the rest of it.
use std::collections::HashSet;
use std::path::PathBuf;
use threadbear_client::config::RunnerConfig;

fn path() -> PathBuf {
    RunnerConfig::from_env().home.join("desktop-highlights.json")
}

pub(crate) fn load_collapsed() -> HashSet<String> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .map(|list| list.into_iter().collect())
        .unwrap_or_default()
}

pub(crate) fn save_collapsed(folds: &HashSet<String>) {
    let mut list: Vec<&String> = folds.iter().collect();
    list.sort();
    if let Ok(text) = serde_json::to_string(&list) {
        // Best effort: it's a preference, not data.
        let _ = std::fs::write(path(), text);
    }
}
