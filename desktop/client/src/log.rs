// The app's own log, as the Swift app's AppLog.swift: the connection coming
// and going, requests that failed, runner notices, held sends and, in a
// scripted run, the automation commands. So an agent can see why something
// didn't happen, rather than guess from where it stopped.
//
// Lines go to stderr always, and to a file once `open` has named one: the
// installed app's `~/Library/Logs/Threadbear/desktop.log`, a scripted app's
// `app.log` beside its automation file (desktop/gpui/src/app_log.rs picks).
// The file is kept under 2 MB: past that it's moved to `.1` and started again.
//
// Never the words of a message, a draft or a reply: thread ids, method
// names, counts, and the runner's and the connection's own error text.
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Past this the file starts again, the old one kept as `.1`.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Opens the file to add to its end. The app hands over core's
/// `private_file::append` (owner-only from the first byte, never through a
/// symlink), which this crate can't reach itself.
pub type Opener = fn(&Path) -> std::io::Result<File>;

struct LogFile {
    path: PathBuf,
    file: File,
    bytes: u64,
    opener: Opener,
}

static FILE: Mutex<Option<LogFile>> = Mutex::new(None);

/// Starts copying lines to `path`, adding to what's there. Its folder must
/// be there already.
pub fn open(path: &Path, opener: Opener) -> std::io::Result<()> {
    let file = opener(path)?;
    let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(LogFile { path: path.to_owned(), file, bytes, opener });
    Ok(())
}

/// Where lines are going, if anywhere but stderr.
pub fn path() -> Option<PathBuf> {
    FILE.lock().ok()?.as_ref().map(|f| f.path.clone())
}

/// One line: the time, what it's about (`connection`, `rpc`, `notice`,
/// `restart`, `send`, `automation`) and what happened.
pub fn write(category: &str, message: &str) {
    let line = format!("{} {category} {message}\n", clock());
    eprint!("threadbear: {category} {message}\n");
    let Ok(mut guard) = FILE.lock() else { return };
    let Some(log) = guard.as_mut() else { return };
    if log.bytes + line.len() as u64 > MAX_BYTES {
        let old = log.path.with_extension("log.1");
        let _ = std::fs::rename(&log.path, &old);
        match (log.opener)(&log.path) {
            Ok(file) => {
                log.file = file;
                log.bytes = 0;
            }
            Err(_) => return,
        }
    }
    if log.file.write_all(line.as_bytes()).is_ok() {
        log.bytes += line.len() as u64;
    }
}

/// The time of day in UTC, to the millisecond ("10:31:07.042"): the log is
/// for reading beside the runner's, which is in UTC too.
fn clock() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let day_ms = ms % 86_400_000;
    let (h, m, s, milli) = (day_ms / 3_600_000, day_ms / 60_000 % 60, day_ms / 1000 % 60, day_ms % 1000);
    format!("{h:02}:{m:02}:{s:02}.{milli:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_lines_and_starts_again_past_the_limit() {
        let dir = std::env::temp_dir().join(format!("tb-app-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("desktop.log");
        std::fs::create_dir_all(&dir).unwrap();
        open(&path, |p| std::fs::OpenOptions::new().create(true).append(true).open(p)).unwrap();
        write("connection", "connected to runner 1.2.3");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with(" connection connected to runner 1.2.3\n"), "{text}");
        assert_eq!(text.split(' ').next().map(str::len), Some("00:00:00.000".len()));
        // Past the limit the file is moved aside and begun again.
        let big = "x".repeat(1024);
        for _ in 0..(MAX_BYTES / 1024 + 2) {
            write("rpc", &big);
        }
        assert!(std::fs::metadata(&path).unwrap().len() < MAX_BYTES);
        assert!(dir.join("desktop.log.1").exists());
        *FILE.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
