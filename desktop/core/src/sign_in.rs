//! Sign in… on the signed-out line: the Terminal script that runs a CLI's own
//! sign-in for one login, as the Swift app's `SignIn` writes it. The same way
//! the runner starts that CLI: claude-main with no CLAUDE_CONFIG_DIR (so it's
//! the login in the Keychain the runner reads), the other Claude logins with
//! their folder, and Codex with CODEX_HOME.
//!
//! Everything that comes from outside the app (a login's name and folder, the
//! CLI's path) is quoted or printed as data, never run: a login's provider
//! once went into the script as its name, shell and all (Daybreak, 7 Nov).

/// The thread's login to sign in again, or None: never for another runner's
/// thread (that Mac's login is signed in on that Mac, and nothing its runner
/// says may pick what runs here), and an unknown login falls back to the
/// main one of a provider there is, named in fixed words (`signInAccount`).
pub fn account_for(provider: &str, remote: bool, known: Option<SignInAccount>) -> Option<SignInAccount> {
    if remote {
        return None;
    }
    if let Some(known) = known {
        return Some(known);
    }
    match provider {
        "claude" => Some(SignInAccount { id: "claude-main".into(), provider: "claude".into(), title: "Claude".into(), config_dir: String::new() }),
        "codex" => Some(SignInAccount { id: "codex-main".into(), provider: "codex".into(), title: "Codex".into(), config_dir: String::new() }),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInAccount {
    pub id: String,
    pub provider: String,
    pub title: String,
    pub config_dir: String,
}

/// A provider's own sign-in, as `ProviderInfo.loginCommand` has it.
pub fn login_command(provider: &str) -> &'static str {
    match provider {
        "codex" => "codex login",
        _ => "claude auth login",
    }
}

/// `s` in single quotes for zsh, with any quote in it closed and reopened.
pub fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A login's name on one line, for the script's first line of output: data
/// printed by printf, never shell source.
pub fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(200).collect()
}

/// An account id as part of a file name: letters, digits, - and _.
pub fn file_safe(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(64).collect()
}

/// The script. `cli_path` is the binary the runner starts for new sessions
/// (Codex is often Threadbear's own copy, not on the PATH); empty uses the
/// PATH's.
pub fn script(account: &SignInAccount, cli_path: &str) -> String {
    let codex = account.provider == "codex";
    let mut env = String::new();
    if codex && !account.config_dir.is_empty() {
        env = format!("CODEX_HOME={} ", quoted(&account.config_dir));
    }
    if (account.provider == "claude" || account.provider.is_empty()) && account.id != "claude-main" && !account.config_dir.is_empty() {
        env = format!("CLAUDE_CONFIG_DIR={} ", quoted(&account.config_dir));
    }
    let name = crate::logins::provider_title(&account.provider);
    // "claude auth login": the program, then what it's asked to do.
    let mut words = login_command(&account.provider).split(' ');
    let program = words.next().unwrap_or("claude");
    let command = words.collect::<Vec<_>>().join(" ");
    let binary = if cli_path.is_empty() { program.to_owned() } else { quoted(cli_path) };
    let first = quoted(&format!("Signing in to {name} for Threadbear ({}).", one_line(&account.title)));
    format!(
        "#!/bin/zsh -l\nexport PATH=\"$HOME/.local/bin:$PATH\"\nprintf '%s\\n' {first}\necho\n{env}{binary} {command}\necho\necho \"Done. Go back to Threadbear and press Send again. You can close this window.\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, provider: &str, title: &str, config_dir: &str) -> SignInAccount {
        SignInAccount { id: id.into(), provider: provider.into(), title: title.into(), config_dir: config_dir.into() }
    }

    #[test]
    fn the_main_claude_login_sets_no_config_folder() {
        let s = script(&account("claude-main", "claude", "me@example.com", "/Users/x/.claude"), "/Users/x/.local/bin/claude");
        assert!(s.contains("\n'/Users/x/.local/bin/claude' auth login\n"), "{s}");
        assert!(!s.contains("CLAUDE_CONFIG_DIR"));
    }

    #[test]
    fn another_claude_login_uses_its_folder() {
        let s = script(&account("claude-1", "claude", "two", "/Users/x/Library/Application Support/Threadbear/accounts/claude-1"), "");
        assert!(s.contains("CLAUDE_CONFIG_DIR='/Users/x/Library/Application Support/Threadbear/accounts/claude-1' claude auth login"), "{s}");
    }

    #[test]
    fn codex_uses_codex_home_and_quotes_it() {
        let s = script(&account("codex-1", "codex", "c", "/tmp/it's here"), "/opt/codex");
        assert!(s.contains("CODEX_HOME='/tmp/it'\\''s here' '/opt/codex' login"), "{s}");
        assert!(s.starts_with("#!/bin/zsh -l\n"));
    }

    #[test]
    fn another_runners_thread_never_picks_a_sign_in_here() {
        assert_eq!(account_for("claude", true, Some(account("claude-main", "claude", "me", ""))), None, "even with a login of that id here");
        assert_eq!(account_for("claude$(touch /tmp/x)", false, None), None, "no such provider: no sign-in");
        assert_eq!(account_for("codex", false, None).map(|a| a.id), Some("codex-main".into()));
    }

    #[test]
    fn a_logins_name_is_printed_never_run() {
        let title = "claude$(touch injected) `touch injected` \"q\" 'q' \\ $HOME\nsecond line; touch injected";
        let s = script(&account("../../evil id", "claude", title, ""), "/usr/bin/true");
        let printed = s.lines().find(|l| l.starts_with("printf")).unwrap();
        // The whole name is inside one pair of single quotes, its own quote
        // closed and reopened, and the newline gone.
        assert_eq!(printed, format!("printf '%s\\n' {}", quoted(&format!("Signing in to Claude for Threadbear ({}).", one_line(title)))));
        assert!(!printed.contains('\n'));
        assert_eq!(file_safe("../../evil id"), "______evil_id");
    }
}
