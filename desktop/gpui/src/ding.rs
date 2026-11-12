// The soft ding when a thread finishes working (Settings > General >
// Notifications), Swift's DingPlayer. It plays wherever Mike is, even in
// another app, at most once in a second and a half, so several threads
// finishing together ding once. The sound itself is made in
// threadbear_core::completion_sound, the same as the Swift app's.
use crate::settings::Settings;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use threadbear_core::completion_sound::{self, Ding};

thread_local! {
    static LAST_PLAYED: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Each ding's WAV, made the first time it plays.
    static MADE: RefCell<HashMap<&'static str, Vec<u8>>> = RefCell::new(HashMap::new());
}

/// After a thread's summary changes: the ding, if it just finished.
pub(crate) fn thread_changed(previous: Option<&str>, now: &str, settings: &Settings) {
    if !settings.ding_on() || !completion_sound::finished(previous, now) {
        return;
    }
    if LAST_PLAYED.get().is_some_and(|at| at.elapsed() < Duration::from_millis(1500)) {
        return;
    }
    play(settings.ding(), settings.ding_volume());
}

/// Plays `ding` now (Settings' Play button, or picking one there).
pub(crate) fn play(ding: Ding, volume: f64) {
    LAST_PLAYED.set(Some(Instant::now()));
    MADE.with_borrow_mut(|made| {
        let wav = made.entry(ding.name()).or_insert_with(|| completion_sound::wav(ding));
        platform::play(wav, volume);
    });
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::Retained;
    use objc2::AnyThread as _;
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSData;
    use std::cell::RefCell;

    thread_local! {
        /// The one playing, kept so the next stops it rather than both
        /// sounding at once, and so it isn't freed partway through.
        static PLAYING: RefCell<Option<Retained<NSSound>>> = const { RefCell::new(None) };
    }

    pub(super) fn play(wav: &[u8], volume: f64) {
        let data = NSData::with_bytes(wav);
        let Some(sound) = NSSound::initWithData(NSSound::alloc(), &data) else { return };
        sound.setVolume(volume.clamp(0., 1.) as f32);
        PLAYING.with_borrow_mut(|playing| {
            if let Some(old) = playing.take() {
                old.stop();
            }
            sound.play();
            *playing = Some(sound);
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    // No sound yet on Windows and Linux: the switch is there, and quiet.
    pub(super) fn play(_wav: &[u8], _volume: f64) {}
}
