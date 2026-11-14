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

/// Why a scripted app (one with THREADBEAR_AUTOMATION set) mustn't start
/// with `config`: it would reach the live runner, the one with the real data
/// folder `real`, and every command a script sent would act on Mike's real
/// threads, write his drafts and settings, and start real turns. That's what
/// an app with no THREADBEAR_HOME gets (the parity audit, 13 Nov). Not only
/// the real folder itself: a folder whose token is the real one (`secrets`
/// linked to it), or any folder on the live runner's port (a copied token),
/// gets through to it too (Codex's review). `allowed` is
/// THREADBEAR_AUTOMATION_LIVE=1, for when the person whose runner it is says
/// so, as the Swift app's `--automation-allow-live`.
pub fn scripted_refusal(config: &RunnerConfig, real: &Path, allowed: bool) -> Option<String> {
    if allowed {
        return None;
    }
    // Compared with links resolved, so ~/x/../Library/... or a link to the
    // real folder is the real folder too.
    // A file not there yet (a token before the runner's first start) by its
    // folder, so a linked `secrets` still counts.
    let resolved = |p: &Path| {
        std::fs::canonicalize(p).unwrap_or_else(|_| match (p.parent().and_then(|d| std::fs::canonicalize(d).ok()), p.file_name()) {
            (Some(dir), Some(name)) => dir.join(name),
            _ => p.to_path_buf(),
        })
    };
    let live = RunnerConfig { home: real.to_path_buf(), port: port_from_runner_json(real).unwrap_or(DEFAULT_PORT) };
    let why = if resolved(&config.home) == resolved(real) {
        format!("won't use the real data folder ({})", real.display())
    } else if resolved(&config.token_path()) == resolved(&live.token_path()) {
        format!("won't use the live runner's token ({} is it)", config.token_path().display())
    } else if config.port == live.port {
        format!("won't connect on the live runner's port ({})", live.port)
    } else {
        return None;
    };
    Some(format!(
        "A scripted app {why}: that runner has the real threads. Set THREADBEAR_HOME to the mock's or a dev runner's (desktop/scripts/dev-app.sh does), or THREADBEAR_AUTOMATION_LIVE=1 if the person whose runner it is has said so."
    ))
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
    fn a_scripted_app_refuses_the_live_runner() {
        let base = std::env::temp_dir().join(format!("tb-desktop-refuse-{}", std::process::id()));
        let real = base.join("real");
        let other = base.join("mock");
        std::fs::create_dir_all(real.join("secrets")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(real.join("runner.json"), r#"{"port": 9555}"#).unwrap();
        let at = |home: PathBuf, port: u16| RunnerConfig { home, port };
        assert!(scripted_refusal(&at(real.clone(), 1234), &real, false).is_some());
        // The same folder by another way there.
        assert!(scripted_refusal(&at(base.join("mock/../real"), 1234), &real, false).is_some());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real, base.join("link")).unwrap();
            assert!(scripted_refusal(&at(base.join("link"), 1234), &real, false).is_some());
            // A folder of its own whose token is the real one.
            let borrowed = base.join("borrowed");
            std::fs::create_dir_all(&borrowed).unwrap();
            std::os::unix::fs::symlink(real.join("secrets"), borrowed.join("secrets")).unwrap();
            assert!(scripted_refusal(&at(borrowed, 1234), &real, false).unwrap().contains("token"));
        }
        // The real folder not made yet still counts.
        assert!(scripted_refusal(&at(base.join("none"), 1234), &base.join("none"), false).is_some());
        // The live runner's port, from its runner.json, else the default.
        assert!(scripted_refusal(&at(other.clone(), 9555), &real, false).unwrap().contains("port"));
        assert!(scripted_refusal(&at(other.clone(), DEFAULT_PORT), &base.join("none"), false).unwrap().contains("port"));
        assert_eq!(scripted_refusal(&at(other.clone(), 1234), &real, false), None);
        // Told it may.
        assert_eq!(scripted_refusal(&at(real.clone(), 9555), &real, true), None);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn expands_a_leading_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home("~/x/y"), home.join("x/y"));
        assert_eq!(expand_home("/abs"), PathBuf::from("/abs"));
    }
}
