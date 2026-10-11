//! Playing a file. An audio file rodio can read is decoded by rodio (`Decoded`). A video, or
//! a sound rodio cannot read, is decoded by ffmpeg (`Streamed`): one process sends the pictures,
//! scaled down, and another the sound, whose position is the clock the pictures follow.

use super::ffmpeg::{Probe, Tools, quiet};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use rodio::{ChannelCount, Player, SampleRate, Source, mixer::Mixer};
use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    num::NonZero,
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

/// Largest picture asked of ffmpeg: bigger videos are scaled down to fit.
pub const MAX_PICTURE: [u32; 2] = [1280, 720];
/// Fastest frame rate shown.
const MAX_FPS: f64 = 60.0;
/// The sound of a video, as ffmpeg sends it.
const SOUND_RATE: u32 = 48_000;
const SOUND_CHANNELS: u16 = 2;
/// Frames of sound per chunk read from ffmpeg (about 85 ms).
const SOUND_CHUNK: usize = 4096;

/// A file being played.
pub enum Media {
    Decoded(Decoded),
    Streamed(Box<Streamed>),
}

impl Media {
    pub fn play(&mut self) {
        match self {
            Self::Decoded(m) => m.play(),
            Self::Streamed(m) => m.play(),
        }
    }
    pub fn pause(&mut self) {
        match self {
            Self::Decoded(m) => m.player.pause(),
            Self::Streamed(m) => m.pause(),
        }
    }
    pub fn playing(&self) -> bool {
        match self {
            Self::Decoded(m) => !m.player.is_paused() && !m.player.empty(),
            Self::Streamed(m) => m.playing,
        }
    }
    /// In seconds from the start.
    pub fn position(&self) -> f64 {
        match self {
            Self::Decoded(m) => m.position(),
            Self::Streamed(m) => m.clock.now().min(m.duration().unwrap_or(f64::MAX)),
        }
    }
    pub fn duration(&self) -> Option<f64> {
        match self {
            Self::Decoded(m) => m.duration,
            Self::Streamed(m) => m.duration(),
        }
    }
    pub fn seek(&mut self, to: f64) -> Result<(), String> {
        let to = to.max(0.0);
        match self {
            Self::Decoded(m) => m.seek(to),
            Self::Streamed(m) => {
                m.start(to);
                Ok(())
            }
        }
    }
    pub fn set_volume(&mut self, volume: f32) {
        match self {
            Self::Decoded(m) => m.player.set_volume(volume),
            Self::Streamed(m) => m.set_volume(volume),
        }
    }
    /// The picture to show, for a video.
    pub fn picture(&self) -> Option<&TextureHandle> {
        match self {
            Self::Decoded(_) => None,
            Self::Streamed(m) => m.texture.as_ref(),
        }
    }
    pub fn is_video(&self) -> bool {
        matches!(self, Self::Streamed(m) if m.probe.video.is_some())
    }
    /// Why the file stopped playing, when it failed.
    pub fn error(&self) -> Option<String> {
        match self {
            Self::Decoded(_) => None,
            Self::Streamed(m) => m.error(),
        }
    }
    /// Takes the pictures and the sound that came in; returns when to look again.
    pub fn tick(&mut self, ctx: &egui::Context) -> Option<Duration> {
        match self {
            Self::Decoded(m) => {
                (!m.player.is_paused() && !m.player.empty()).then_some(Duration::from_millis(200))
            }
            Self::Streamed(m) => m.tick(ctx),
        }
    }
}

/// An audio file decoded by rodio.
pub struct Decoded {
    path: PathBuf,
    mixer: Mixer,
    player: Player,
    duration: Option<f64>,
    volume: f32,
}

/// Reads an audio file with rodio. Its length is known for most files; `fallback` is used
/// when it is not (an MP3 without a header saying so).
pub fn decode(path: &Path, mixer: &Mixer, fallback: Option<f64>) -> Result<Decoded, String> {
    let (player, duration) = load(path, mixer)?;
    Ok(Decoded {
        path: path.to_path_buf(),
        mixer: mixer.clone(),
        player,
        duration: duration.or(fallback),
        volume: 1.0,
    })
}

/// A paused player of `path`, and the length of the sound.
fn load(path: &Path, mixer: &Mixer) -> Result<(Player, Option<f64>), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let decoder = rodio::Decoder::try_from(file).map_err(|e| e.to_string())?;
    let duration = decoder.total_duration().map(|d| d.as_secs_f64());
    let player = Player::connect_new(mixer);
    player.pause();
    player.append(decoder);
    Ok((player, duration))
}

impl Decoded {
    fn play(&mut self) {
        if self.player.empty() {
            // Played to the end: from the start again.
            if let Ok((player, _)) = load(&self.path, &self.mixer) {
                player.set_volume(self.volume);
                self.player = player;
            }
        }
        self.player.play();
    }

    fn position(&self) -> f64 {
        if self.player.empty() {
            return self.duration.unwrap_or(0.0);
        }
        self.player.get_pos().as_secs_f64()
    }

    fn seek(&mut self, to: f64) -> Result<(), String> {
        let paused = self.player.is_paused();
        if self.player.empty() {
            let (player, _) = load(&self.path, &self.mixer)?;
            player.set_volume(self.volume);
            self.player = player;
        }
        self.player
            .try_seek(Duration::from_secs_f64(to))
            .map_err(|e| e.to_string())?;
        if paused {
            self.player.pause();
        }
        Ok(())
    }
}

/// Time that runs while playing.
#[derive(Default)]
struct Clock {
    base: f64,
    since: Option<Instant>,
}

impl Clock {
    fn now(&self) -> f64 {
        self.base + self.since.map_or(0.0, |s| s.elapsed().as_secs_f64())
    }
    fn set(&mut self, time: f64, running: bool) {
        self.base = time;
        self.since = running.then(Instant::now);
    }
}

/// The ffmpeg processes of one run, from a position until the end or the next seek.
struct Run {
    children: Vec<Child>,
    /// Start of the run, in seconds.
    start: f64,
    frames: Option<Receiver<Vec<u8>>>,
    /// Pictures received so far.
    received: u64,
    /// A picture received, not yet due: its time and its pixels.
    next: Option<(f64, Vec<u8>)>,
    sound: Option<Player>,
    /// Silence played while the sound was late: it does not move the clock.
    silence: Arc<AtomicU64>,
    /// What ffmpeg wrote on its error output.
    errors: Arc<Mutex<String>>,
}

impl Drop for Run {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A file decoded by ffmpeg: a video, or a sound rodio cannot read.
pub struct Streamed {
    path: PathBuf,
    tools: Tools,
    pub probe: Probe,
    mixer: Option<Mixer>,
    /// Size of the pictures asked of ffmpeg.
    size: [u32; 2],
    fps: f64,
    volume: f32,
    playing: bool,
    clock: Clock,
    run: Option<Run>,
    texture: Option<TextureHandle>,
    shown: bool,
    finished: bool,
}

impl Streamed {
    /// Ready to play from the start, paused. `mixer` is None without a sound device: the video
    /// then plays silently.
    pub fn new(path: &Path, tools: &Tools, probe: Probe, mixer: Option<&Mixer>) -> Self {
        let (size, fps) = probe.video.map_or(([0, 0], 0.0), |v| {
            (
                picture_size(v.width, v.height, MAX_PICTURE),
                v.fps.min(MAX_FPS),
            )
        });
        let mut streamed = Self {
            path: path.to_path_buf(),
            tools: tools.clone(),
            probe,
            mixer: mixer.cloned(),
            size,
            fps,
            volume: 1.0,
            playing: false,
            clock: Clock::default(),
            run: None,
            texture: None,
            shown: false,
            finished: false,
        };
        streamed.start(0.0);
        streamed
    }

    fn duration(&self) -> Option<f64> {
        self.probe.duration
    }

    /// Starts the processes at `at`, keeping playing or paused.
    fn start(&mut self, at: f64) {
        let at = match self.duration() {
            Some(duration) => at.min((duration - 0.05).max(0.0)),
            None => at,
        };
        self.run = None;
        self.finished = false;
        self.shown = false;
        let errors = Arc::new(Mutex::new(String::new()));
        let mut run = Run {
            children: Vec::new(),
            start: at,
            frames: None,
            received: 0,
            next: None,
            sound: None,
            silence: Arc::new(AtomicU64::new(0)),
            errors: errors.clone(),
        };
        if self.probe.video.is_some() {
            match self.spawn(video_args(&self.path, at, self.size, self.fps), &errors) {
                Ok(mut child) => {
                    let frame = self.size[0] as usize * self.size[1] as usize * 4;
                    let (sender, receiver) = sync_channel(4);
                    if let Some(out) = child.stdout.take() {
                        thread::spawn(move || read_frames(out, frame, sender));
                    }
                    run.frames = Some(receiver);
                    run.children.push(child);
                }
                Err(e) => errors.lock().unwrap().push_str(&e),
            }
        }
        if self.probe.audio
            && let Some(mixer) = &self.mixer
            && let Ok(mut child) = self.spawn(sound_args(&self.path, at), &errors)
        {
            let (sender, receiver) = sync_channel(16);
            if let Some(out) = child.stdout.take() {
                thread::spawn(move || read_sound(out, sender));
            }
            let player = Player::connect_new(mixer);
            player.pause();
            player.set_volume(self.volume);
            player.append(SoundStream::new(receiver, run.silence.clone()));
            if self.playing {
                player.play();
            }
            run.sound = Some(player);
            run.children.push(child);
        }
        self.run = Some(run);
        // Once the processes are started, which takes a moment.
        self.clock.set(at, self.playing);
    }

    fn spawn(&self, args: Vec<OsString>, errors: &Arc<Mutex<String>>) -> Result<Child, String> {
        let mut child = quiet(&self.tools.ffmpeg)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        if let Some(mut err) = child.stderr.take() {
            let errors = errors.clone();
            thread::spawn(move || {
                let mut text = String::new();
                let _ = err.read_to_string(&mut text);
                errors.lock().unwrap().push_str(&text);
            });
        }
        Ok(child)
    }

    fn play(&mut self) {
        if self.finished || self.ended() {
            self.playing = true;
            self.start(0.0);
            return;
        }
        self.playing = true;
        self.clock.set(self.clock.now(), true);
        if let Some(sound) = self.run.as_ref().and_then(|r| r.sound.as_ref()) {
            sound.play();
        }
    }

    fn pause(&mut self) {
        self.playing = false;
        self.clock.set(self.clock.now(), false);
        if let Some(sound) = self.run.as_ref().and_then(|r| r.sound.as_ref()) {
            sound.pause();
        }
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        if let Some(sound) = self.run.as_ref().and_then(|r| r.sound.as_ref()) {
            sound.set_volume(volume);
        }
    }

    fn sound_playing(&self) -> bool {
        self.run
            .as_ref()
            .and_then(|r| r.sound.as_ref())
            .is_some_and(|s| !s.empty())
    }

    fn ended(&self) -> bool {
        let Some(run) = &self.run else {
            return true;
        };
        run.frames.is_none() && run.next.is_none() && !self.sound_playing()
    }

    fn error(&self) -> Option<String> {
        let run = self.run.as_ref()?;
        // Only when nothing came: a warning in the middle of a file does not stop it.
        let nothing = run.received == 0 && run.frames.is_none() && !self.sound_playing();
        let errors = run.errors.lock().unwrap();
        (nothing && !errors.trim().is_empty()).then(|| errors.trim().to_string())
    }

    fn tick(&mut self, ctx: &egui::Context) -> Option<Duration> {
        let fps = self.fps;
        let playing = self.playing;
        let run = self.run.as_mut()?;
        // The sound sets the time while it plays; without it, the clock runs on.
        if let Some(sound) = &run.sound
            && !sound.empty()
            && playing
        {
            let silence = run.silence.load(Ordering::Relaxed) as f64 / SOUND_RATE as f64;
            let heard = run.start + (sound.get_pos().as_secs_f64() - silence).max(0.0);
            self.clock.set(heard, true);
        }
        // Without sound, the clock waits for the first picture: ffmpeg takes a moment to start.
        if run.sound.is_none() && !self.shown {
            self.clock.set(run.start, playing);
        }
        let now = self.clock.now();
        let mut latest = None;
        loop {
            if run.next.is_none() {
                let Some(frames) = &run.frames else { break };
                match frames.try_recv() {
                    Ok(pixels) => {
                        let time = run.start + run.received as f64 / fps;
                        run.received += 1;
                        run.next = Some((time, pixels));
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        run.frames = None;
                        break;
                    }
                }
            }
            let due = run
                .next
                .as_ref()
                .is_some_and(|(time, _)| *time <= now + 0.005);
            // Paused, the first picture after a seek is shown at once.
            let first = !self.shown && latest.is_none();
            if due || first {
                latest = run.next.take();
            } else {
                break;
            }
        }
        if let Some((_, pixels)) = latest {
            let [w, h] = self.size;
            let image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ctx.load_texture(
                        format!("media:{}", self.path.display()),
                        image,
                        TextureOptions::LINEAR,
                    ));
                }
            }
            self.shown = true;
        }
        if self.ended() && !self.finished {
            self.finished = true;
            self.playing = false;
            let end = self.duration().unwrap_or(now).max(now);
            self.clock.set(end, false);
        }
        if !self.playing {
            // Paused: only while the first picture is on its way.
            return (!self.shown && self.probe.video.is_some())
                .then_some(Duration::from_millis(15));
        }
        let run = self.run.as_ref()?;
        Some(match &run.next {
            Some((time, _)) => Duration::from_secs_f64((time - now).clamp(0.001, 0.1)),
            None => Duration::from_millis(if self.probe.video.is_some() { 5 } else { 100 }),
        })
    }
}

/// The size of the pictures asked of ffmpeg: the video's, scaled down to fit `max`, in even
/// numbers of pixels.
pub fn picture_size(width: u32, height: u32, max: [u32; 2]) -> [u32; 2] {
    let scale = (max[0] as f64 / width.max(1) as f64)
        .min(max[1] as f64 / height.max(1) as f64)
        .min(1.0);
    let even = |side: u32| ((side as f64 * scale).round() as u32 / 2 * 2).max(2);
    [even(width), even(height)]
}

fn seconds(at: f64) -> String {
    format!("{at:.3}")
}

/// ffmpeg sending the pictures from `at`, in RGBA, `size` pixels, `fps` per second.
pub fn video_args(path: &Path, at: f64, size: [u32; 2], fps: f64) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-ss"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(seconds(at).into());
    args.push("-i".into());
    args.push(path.into());
    for arg in ["-map", "0:v:0", "-an", "-sn", "-vf"] {
        args.push(arg.into());
    }
    args.push(format!("fps={fps},scale={}:{}:flags=bilinear", size[0], size[1]).into());
    for arg in ["-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"] {
        args.push(arg.into());
    }
    args
}

/// ffmpeg sending the sound from `at`: 32-bit floats, stereo, 48 kHz.
pub fn sound_args(path: &Path, at: f64) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-ss"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(seconds(at).into());
    args.push("-i".into());
    args.push(path.into());
    let channels = SOUND_CHANNELS.to_string();
    let rate = SOUND_RATE.to_string();
    for arg in [
        "-map", "0:a:0", "-vn", "-sn", "-ac", &channels, "-ar", &rate, "-f", "f32le", "pipe:1",
    ] {
        args.push(arg.into());
    }
    args
}

/// Reads `buffer.len()` bytes, or fewer at the end; returns how many.
fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> usize {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) | Err(_) => break,
            Ok(read) => filled += read,
        }
    }
    filled
}

fn read_frames(mut out: impl Read, frame: usize, sender: SyncSender<Vec<u8>>) {
    loop {
        let mut pixels = vec![0u8; frame];
        if fill(&mut out, &mut pixels) < frame || sender.send(pixels).is_err() {
            return;
        }
    }
}

fn read_sound(mut out: impl Read, sender: SyncSender<Vec<f32>>) {
    let bytes_per_frame = 4 * SOUND_CHANNELS as usize;
    let mut buffer = vec![0u8; SOUND_CHUNK * bytes_per_frame];
    loop {
        let read = fill(&mut out, &mut buffer);
        let whole = read / bytes_per_frame * bytes_per_frame;
        if whole > 0 {
            let samples = buffer[..whole]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            if sender.send(samples).is_err() {
                return;
            }
        }
        if read < buffer.len() {
            return;
        }
    }
}

/// The sound ffmpeg sends, as a rodio source. When it is late, silence is played, a whole
/// frame at a time, and counted in `silence` (frames) so that the clock leaves it out.
pub struct SoundStream {
    chunks: Receiver<Vec<f32>>,
    buffer: Vec<f32>,
    at: usize,
    silence: Arc<AtomicU64>,
}

impl SoundStream {
    pub fn new(chunks: Receiver<Vec<f32>>, silence: Arc<AtomicU64>) -> Self {
        Self {
            chunks,
            buffer: Vec::new(),
            at: 0,
            silence,
        }
    }
}

impl Iterator for SoundStream {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if let Some(&sample) = self.buffer.get(self.at) {
                self.at += 1;
                return Some(sample);
            }
            self.at = 0;
            match self.chunks.try_recv() {
                Ok(chunk) => self.buffer = chunk,
                Err(TryRecvError::Empty) => {
                    self.buffer = vec![0.0; SOUND_CHANNELS as usize];
                    self.silence.fetch_add(1, Ordering::Relaxed);
                }
                Err(TryRecvError::Disconnected) => return None,
            }
        }
    }
}

impl Source for SoundStream {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        NonZero::new(SOUND_CHANNELS).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SOUND_RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/media/playback/tests.rs"]
mod tests;
