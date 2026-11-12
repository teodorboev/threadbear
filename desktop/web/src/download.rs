// Fetching the runtime: one GET of the pinned archive from Spotify's CDN,
// handed to threadbear_core::web_runtime, which checks it as it comes in and
// again before a file comes out of it. Blocking, so the app runs it on a
// thread of its own.
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use threadbear_core::web_runtime::{self as core, Archive, InstallError, Layout};

/// How far an install has got, for the app to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Downloading { received: u64, total: u64 },
    Unpacking,
}

/// Downloads `archive` into the layout's download folder and installs it.
/// `local` is a copy already on disk to use in its place, which only the
/// scripted control passes (the specs, so each run doesn't fetch 135 MB): it's
/// held to the same pin as a download, so a copy that isn't the build is
/// refused the same way.
pub fn install(archive: &Archive, layout: &Layout, local: Option<&std::path::Path>, progress: &mut dyn FnMut(Progress), cancel: &AtomicBool) -> Result<(), InstallError> {
    let pin = archive.pin();
    if let Some(local) = local {
        progress(Progress::Unpacking);
        return core::install_from_file(local, &pin, layout);
    }
    let part = layout.download_file(archive.file);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        // A stall this long means the connection's gone; a slow one keeps going.
        .timeout_per_read(Some(Duration::from_secs(60)))
        .user_agent("Threadbear (web runtime)")
        .build()
        .into();
    let response = agent.get(&archive.url()).call().map_err(|e| InstallError::Failed(format!("couldn't reach Spotify's CEF builds ({e})")))?;
    let mut body = response.into_body().into_reader();
    let total = pin.bytes;
    let mut last = 0u64;
    core::receive(
        &mut body,
        &part,
        &pin,
        &mut |received| {
            // About every megabyte: more is just redrawing.
            if received - last >= 1_000_000 || received == total {
                last = received;
                progress(Progress::Downloading { received, total });
            }
        },
        cancel,
    )?;
    progress(Progress::Unpacking);
    let installed = core::install_from_file(&part, &pin, layout);
    let _ = std::fs::remove_file(&part);
    // The download folder too, once it's empty.
    if let Some(dir) = part.parent() {
        let _ = std::fs::remove_dir(dir);
    }
    installed
}
