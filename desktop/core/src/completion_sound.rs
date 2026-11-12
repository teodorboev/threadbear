//! The soft ding that says a thread is done (Settings > General >
//! Notifications), off until it's turned on: Swift's `CompletionSound`. It
//! plays when a thread stops working (its turn finished, it's waiting for an
//! answer, or it failed), not when it was stopped, as whoever stopped it
//! knows. The three dings are made here as WAV data, so there's no sound file
//! to ship, and they come out the same as the Swift app's.

pub const DEFAULT_VOLUME: f64 = 0.6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ding {
    #[default]
    Chime,
    Bell,
    Droplet,
}

impl Ding {
    pub const ALL: [Ding; 3] = [Ding::Chime, Ding::Bell, Ding::Droplet];

    /// The name saved for it (`completionSoundDing`), Swift's raw value.
    pub fn name(self) -> &'static str {
        match self {
            Ding::Chime => "chime",
            Ding::Bell => "bell",
            Ding::Droplet => "droplet",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Ding::Chime => "Chime",
            Ding::Bell => "Bell",
            Ding::Droplet => "Droplet",
        }
    }

    /// A saved name, or the chime for one it doesn't know.
    pub fn from_name(name: &str) -> Ding {
        Ding::ALL.into_iter().find(|d| d.name() == name).unwrap_or_default()
    }
}

/// Whether a thread going from `previous` to `now` (`ThreadStatus` raw
/// values) has just finished working.
pub fn finished(previous: Option<&str>, now: &str) -> bool {
    matches!(previous, Some("running" | "starting")) && matches!(now, "idle" | "waiting" | "error")
}

const SAMPLE_RATE: usize = 44_100;

/// A sine at `ratio` times the note, `level` loud, dying away over `decay` seconds.
struct Partial {
    ratio: f64,
    level: f64,
    decay: f64,
}

/// A note starting `at` seconds in, gliding from `from` Hz to `to` over `glide` seconds.
struct Note {
    at: f64,
    from: f64,
    to: f64,
    glide: f64,
    partials: Vec<Partial>,
}

fn notes(ding: Ding) -> (f64, Vec<Note>) {
    let partial = |ratio, level, decay| Partial { ratio, level, decay };
    match ding {
        // Two notes going up, G5 then C6, like a gentle doorbell.
        Ding::Chime => {
            let p = || vec![partial(1., 1., 0.32), partial(2., 0.18, 0.18)];
            (0.95, vec![Note { at: 0., from: 783.99, to: 783.99, glide: 0., partials: p() }, Note { at: 0.13, from: 1046.5, to: 1046.5, glide: 0., partials: p() }])
        }
        // One small bell, A5, with a bell's uneven overtones.
        Ding::Bell => (
            1.5,
            vec![Note { at: 0., from: 880., to: 880., glide: 0., partials: vec![partial(1., 1., 0.55), partial(2.0, 0.28, 0.35), partial(2.76, 0.14, 0.22), partial(5.4, 0.05, 0.1)] }],
        ),
        // A short round blip that falls in pitch, like a drop of water.
        Ding::Droplet => (0.45, vec![Note { at: 0., from: 1250., to: 720., glide: 0.07, partials: vec![partial(1., 1., 0.11)] }]),
    }
}

/// The ding as a 16-bit mono WAV file, peaking at half of full scale so it stays soft.
pub fn wav(ding: Ding) -> Vec<u8> {
    let (length, notes) = notes(ding);
    let rate = SAMPLE_RATE as f64;
    let count = (length * rate) as usize;
    let mut samples = vec![0f64; count];
    let attack = 0.006;
    for note in &notes {
        let start = (note.at * rate) as usize;
        for partial in &note.partials {
            let mut phase = 0.;
            for (i, sample) in samples.iter_mut().enumerate().skip(start) {
                let t = (i - start) as f64 / rate;
                let glide = if note.glide > 0. { (t / note.glide).min(1.) } else { 1. };
                let freq = (note.from + (note.to - note.from) * glide) * partial.ratio;
                phase += 2. * std::f64::consts::PI * freq / rate;
                let envelope = (t / attack).min(1.) * (-t / partial.decay).exp();
                *sample += phase.sin() * partial.level * envelope;
            }
        }
    }
    // A short fade at the very end, so it never stops with a click.
    let fade = (0.03 * rate) as usize;
    for (i, sample) in samples.iter_mut().enumerate().skip(count.saturating_sub(fade)) {
        *sample *= (count - i) as f64 / fade as f64;
    }
    let peak = samples.iter().fold(0f64, |m, s| m.max(s.abs()));
    let scale = if peak > 0. { 0.5 / peak } else { 0. };
    let mut pcm = Vec::with_capacity(count * 2);
    for s in samples {
        let v = ((s * scale).clamp(-1., 1.) * i16::MAX as f64) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = header(pcm.len());
    out.extend(pcm);
    out
}

fn header(data_bytes: usize) -> Vec<u8> {
    let mut d = Vec::with_capacity(44);
    let u32le = |d: &mut Vec<u8>, v: usize| d.extend_from_slice(&(v as u32).to_le_bytes());
    let u16le = |d: &mut Vec<u8>, v: u16| d.extend_from_slice(&v.to_le_bytes());
    d.extend_from_slice(b"RIFF");
    u32le(&mut d, 36 + data_bytes);
    d.extend_from_slice(b"WAVE");
    d.extend_from_slice(b"fmt ");
    u32le(&mut d, 16);
    u16le(&mut d, 1);
    u16le(&mut d, 1);
    u32le(&mut d, SAMPLE_RATE);
    u32le(&mut d, SAMPLE_RATE * 2);
    u16le(&mut d, 2);
    u16le(&mut d, 16);
    d.extend_from_slice(b"data");
    u32le(&mut d, data_bytes);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_dings_when_a_turn_ends_but_not_when_it_was_never_working() {
        assert!(finished(Some("running"), "idle"));
        assert!(finished(Some("starting"), "waiting"));
        assert!(finished(Some("running"), "error"));
        assert!(!finished(Some("idle"), "idle"), "nothing was working");
        assert!(!finished(None, "idle"), "a thread first heard of");
        assert!(!finished(Some("running"), "running"));
    }

    #[test]
    fn each_ding_is_a_soft_mono_wav_of_its_length() {
        for (ding, seconds) in [(Ding::Chime, 0.95), (Ding::Bell, 1.5), (Ding::Droplet, 0.45)] {
            let wav = wav(ding);
            let samples = (seconds * SAMPLE_RATE as f64) as usize;
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..16], b"WAVEfmt ");
            assert_eq!(wav.len(), 44 + samples * 2, "{ding:?}");
            let loudest = wav[44..].chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs()).max().unwrap();
            assert!(loudest <= i16::MAX as u16 / 2 + 1 && loudest > i16::MAX as u16 / 3, "{ding:?} peaks at half: {loudest}");
            let last = i16::from_le_bytes([wav[wav.len() - 2], wav[wav.len() - 1]]);
            assert!(last.unsigned_abs() < 200, "{ding:?} fades out at the end");
        }
        assert_eq!(Ding::from_name("bell"), Ding::Bell);
        assert_eq!(Ding::from_name("klaxon"), Ding::Chime);
    }
}
