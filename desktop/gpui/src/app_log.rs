// Where the app's own log goes (threadbear_client::log, the Swift app's
// AppLog): the installed app's ~/Library/Logs/Threadbear/desktop.log, or a
// scripted app's app.log beside its automation file, so a spec or an agent
// driving it can read why something didn't happen (`dev-app.sh log` reads
// the Swift app's). Any other launch (a debug build run by hand) writes to
// stderr only, and never into the installed app's file.
use crate::installed;
use std::path::{Path, PathBuf};

pub(crate) fn path() -> Option<PathBuf> {
    let launch = installed::launch();
    if launch.scripted {
        let file = std::env::var_os("THREADBEAR_AUTOMATION")?;
        return Some(Path::new(&file).parent()?.join("app.log"));
    }
    launch.installed.then(|| installed::logs_dir().join("desktop.log"))
}

/// From the app's start, before the connection's first line.
pub(crate) fn start() {
    threadbear_core::wire::set_logger(|message| threadbear_client::log::write("decode", message));
    let Some(path) = path() else { return };
    // Owner-only, as every file the app writes (private_file_source.rs).
    let opened = path.parent().map_or(Ok(()), threadbear_core::private_file::dir).and_then(|()| threadbear_client::log::open(&path, threadbear_core::private_file::append));
    match opened {
        Ok(()) => threadbear_client::log::write("app", &format!("started {}", installed::launch().build.as_ref().map_or("(no build info)", |b| b.short.as_str()))),
        Err(e) => eprintln!("threadbear: couldn't open the app log at {}: {e}", path.display()),
    }
}
