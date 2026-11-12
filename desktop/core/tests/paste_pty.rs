//! A paste into a real shell in a real pty, as the Terminal sends it
//! (`terminal::paste_bytes`), never runs what's in it by itself.
//!
//! Daybreak's finding 84 (11 Nov): a paste kept every control character but
//! ESC inside the brackets, and the pty's line discipline takes ^\, ^C and ^Z
//! as signals before bash's bracketed-paste parser sees them. The signal
//! flushes the brackets' opening, so the line break after it is Return and the
//! command runs. Each case here is first played with the bytes the old code
//! sent, to show this machine's bash and pty really do run them (a bash too
//! old for bracketed paste, or a pty that doesn't, skips the test rather than
//! passing it), then with today's `paste_bytes`, which must leave it unrun.
//!
//! The pty is Python's (`pty.fork`), as the core crate has no pty of its own;
//! no Python 3 or bash on the PATH skips it too.

use std::process::Command;
use std::time::{Duration, Instant};
use threadbear_core::terminal::paste_bytes;

/// Starts an interactive bash in a pty, waits for it to turn bracketed paste
/// on, sends the bytes (hex in argv[1]) in one write, and says RAN if the
/// marker came out of a command, HELD if it didn't and bash still runs what's
/// typed after it, DEAD if it doesn't, NOBRACKETS if bash never asked for
/// brackets.
const HARNESS: &str = r#"
import os, pty, sys, time, select
payload = bytes.fromhex(sys.argv[1])
pid, fd = pty.fork()
if pid == 0:
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "TERM": "xterm-256color", "HOME": os.environ.get("TMPDIR", "/tmp"), "PS1": "$ "}
    os.execvpe("bash", ["bash", "--norc", "--noprofile", "-i"], env)
out = b""
def read_for(seconds):
    global out
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                return
            if not chunk:
                return
            out += chunk
end = time.time() + 10
while b"\x1b[?2004h" not in out and time.time() < end:
    read_for(0.1)
if b"\x1b[?2004h" not in out:
    print("NOBRACKETS")
    sys.exit(0)
read_for(0.3)
os.write(fd, payload)
read_for(1.5)
if b"TB_PASTE_RAN" in out:
    print("RAN")
else:
    # Still there, and still at its prompt: ^U takes the held paste off the
    # line, and a command typed after it runs.
    os.write(fd, b"\x15printf 'TB_%s\\n' ALIVE\r")
    read_for(1.5)
    print("HELD" if b"TB_ALIVE" in out else "DEAD")
try:
    os.kill(pid, 9)
except OSError:
    pass
"#;

fn run(bytes: &[u8]) -> Option<String> {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let started = Instant::now();
    let out = Command::new("python3").args(["-I", "-c", HARNESS, &hex]).output().ok()?;
    assert!(started.elapsed() < Duration::from_secs(30));
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// What the Terminal sent before the fix: only ESC and CSI taken out.
fn old_bytes(text: &str) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    let inner: String = text.chars().filter(|c| *c != '\u{1b}' && *c != '\u{9b}').collect();
    [b"\x1b[200~".as_slice(), inner.as_bytes(), b"\x1b[201~".as_slice()].concat()
}

#[test]
fn a_pasted_signal_character_cant_run_the_paste() {
    // The marker is split so the shell echoing the paste back isn't a run.
    let command = "printf \"TB_%s_RAN\\n\" PASTE\n";
    let mut tried = 0;
    for signal in ['\u{1c}', '\u{3}', '\u{1a}'] {
        let text = format!("{signal}{command}");
        match run(&old_bytes(&text)).as_deref() {
            Some("RAN") => {}
            other => {
                eprintln!("skipping {:?}: the old bytes didn't run here ({other:?})", signal);
                continue;
            }
        }
        tried += 1;
        assert_eq!(run(&paste_bytes(&text, true)).as_deref(), Some("HELD"), "{signal:?} then a line break ran the paste");
    }
    // Linux check hosts have bash 5 and Python, so there a run that tried
    // nothing is a broken harness, not a pass. macOS's own bash (3.2) has no
    // bracketed paste, so a Mac without a newer one skips.
    if tried == 0 {
        assert!(!cfg!(target_os = "linux"), "no case could be played here, so nothing was checked");
        eprintln!("skipped: no bash with bracketed paste and a pty that runs the old bytes here");
    }
}
