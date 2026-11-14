// The sign-in script (core/src/sign_in.rs) run by zsh, as the Swift
// SignInTests run it: the login's name comes out as the words, with nothing
// in it run. Here rather than beside the code, as core/src is scanned for
// anything that starts a program (open_gate_source.rs).
use threadbear_core::sign_in::{script, SignInAccount};

#[cfg(unix)]
#[test]
fn zsh_prints_the_name_as_data() {
    let zsh = std::path::Path::new("/bin/zsh");
    if !zsh.exists() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("tb-sign-in-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let title = "claude$(touch injected) `touch injected` \"q\" 'q' \\ $HOME\nsecond line; touch injected";
    // The main login needs no folder. Secondary logins without one now fail
    // closed before a script can run, so use a valid login to test quoting.
    let account = SignInAccount { id: "claude-main".into(), provider: "claude".into(), title: title.into(), config_dir: String::new() };
    let s = script(&account, "/usr/bin/true");
    let out = std::process::Command::new(zsh).args(["-f", "-c", &s]).current_dir(&dir).output().unwrap();
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(!dir.join("injected").exists(), "nothing in the name ran");
    assert!(printed.contains("claude$(touch injected) `touch injected`"), "{printed}");
    assert!(printed.contains("$HOME second line"), "{printed}");
    let _ = std::fs::remove_dir_all(&dir);
}
