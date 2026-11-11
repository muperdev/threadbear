// Where the runner is: its data dir (for the token) and its port, found
// the way runner/src/config.ts and `tb` find them, so pointing
// THREADBEAR_HOME at a dev runner or the mock works the same here.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 8797;

#[derive(Clone, Debug)]
pub struct RunnerConfig {
    pub home: PathBuf,
    pub port: u16,
}

impl RunnerConfig {
    pub fn from_env() -> Self {
        let home = std::env::var("THREADBEAR_HOME")
            .or_else(|_| std::env::var("THREADBEAR_DATA_DIR"))
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| expand_home(&s))
            .unwrap_or_else(default_home);
        let port = std::env::var("THREADBEAR_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| port_from_runner_json(&home))
            .unwrap_or(DEFAULT_PORT);
        RunnerConfig { home, port }
    }

    pub fn token_path(&self) -> PathBuf {
        self.home.join("secrets").join("runner-token")
    }

    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}/rpc", self.port)
    }

    /// Read fresh on every connect: the runner makes a new one when it's
    /// reinstalled, and the mock writes its own.
    pub fn read_token(&self) -> Result<String, String> {
        std::fs::read_to_string(self.token_path())
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("Couldn't read the runner's token at {}: {e}", self.token_path().display()))
    }
}

/// The runner keeps the Mac's folder names on every platform for now
/// (INSTALL.md, "A runner on Linux"). A Windows runner may choose
/// %APPDATA%\Threadbear instead; this follows whatever it picks.
pub fn default_home() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join("Library").join("Application Support").join("Threadbear")
}

fn expand_home(p: &str) -> PathBuf {
    if p == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(rest);
    }
    PathBuf::from(p)
}

fn port_from_runner_json(home: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(home.join("runner.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("port")?.as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_port_from_runner_json() {
        let dir = std::env::temp_dir().join(format!("tb-desktop-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(port_from_runner_json(&dir), None);
        std::fs::write(dir.join("runner.json"), r#"{"port": 9123, "other": true}"#).unwrap();
        assert_eq!(port_from_runner_json(&dir), Some(9123));
        std::fs::write(dir.join("runner.json"), r#"{"port": 0}"#).unwrap();
        assert_eq!(port_from_runner_json(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn expands_a_leading_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home("~/x/y"), home.join("x/y"));
        assert_eq!(expand_home("/abs"), PathBuf::from("/abs"));
    }
}
