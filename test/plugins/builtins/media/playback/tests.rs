use super::*;
use crate::plugins::builtins::media::ffmpeg;

/// A sound device of the tests: a mixer read at the pace a sound card would.
fn speakers() -> Mixer {
    let (mixer, mut source) = rodio::mixer::mixer(
        NonZero::new(SOUND_CHANNELS).unwrap(),
        NonZero::new(SOUND_RATE).unwrap(),
    );
    thread::spawn(move || {
        loop {
            for _ in 0..960 {
                source.next();
            }
            thread::sleep(Duration::from_millis(10));
        }
    });
    mixer
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-media-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// One second of a 440 Hz sine, 16-bit mono.
fn write_wav(path: &Path) {
    let rate = 8000u32;
    let samples: Vec<i16> = (0..rate)
        .map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16)
        .collect();
    let data = samples.len() as u32 * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

/// Calls `tick` until `done`, or fails after `seconds`.
fn run_until(media: &mut Media, ctx: &egui::Context, seconds: f64, done: impl Fn(&Media) -> bool) {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    while !done(media) {
        assert!(
            Instant::now() < deadline,
            "too long, at {}",
            media.position()
        );
        media.tick(ctx);
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn pictures_fit_in_the_largest_size_in_even_pixels() {
    assert_eq!(picture_size(1920, 1080, MAX_PICTURE), [1280, 720]);
    assert_eq!(picture_size(640, 480, MAX_PICTURE), [640, 480]);
    assert_eq!(picture_size(1080, 1920, MAX_PICTURE), [404, 720]);
    assert_eq!(picture_size(321, 241, MAX_PICTURE), [320, 240]);
    assert_eq!(picture_size(1, 1, MAX_PICTURE), [2, 2]);
}

#[test]
fn ffmpeg_is_asked_for_raw_pictures_and_sound_from_a_position() {
    let args: Vec<String> = video_args(Path::new("in.mkv"), 1.5, [320, 240], 25.0)
        .into_iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let at = args.iter().position(|a| a == "-ss").unwrap();
    assert_eq!(args[at + 1], "1.500");
    assert!(
        at < args.iter().position(|a| a == "-i").unwrap(),
        "a fast seek"
    );
    assert!(args.contains(&"fps=25,scale=320:240:flags=bilinear".to_string()));
    assert_eq!(args.last().unwrap(), "pipe:1");
    let sound: Vec<String> = sound_args(Path::new("in.mkv"), 0.0)
        .into_iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(sound.windows(2).any(|w| w == ["-f", "f32le"]));
    assert!(sound.windows(2).any(|w| w == ["-ar", "48000"]));
}

#[test]
fn late_sound_is_filled_with_whole_frames_of_silence_and_counted() {
    let (sender, receiver) = sync_channel(4);
    let silence = Arc::new(AtomicU64::new(0));
    let mut stream = SoundStream::new(receiver, silence.clone());
    sender.send(vec![0.1, 0.2, 0.3, 0.4]).unwrap();
    let first: Vec<f32> = (0..4).map(|_| stream.next().unwrap()).collect();
    assert_eq!(first, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!([stream.next(), stream.next()], [Some(0.0), Some(0.0)]);
    assert_eq!(silence.load(Ordering::Relaxed), 1);
    sender.send(vec![0.5, 0.6]).unwrap();
    assert_eq!([stream.next(), stream.next()], [Some(0.5), Some(0.6)]);
    drop(sender);
    assert_eq!(stream.next(), None);
}

#[test]
fn rodio_plays_and_seeks_in_a_wav_file() {
    let path = temp("tone.wav");
    write_wav(&path);
    let ctx = egui::Context::default();
    let mut media = Media::Decoded(decode(&path, &speakers(), None).unwrap());
    let duration = media.duration().unwrap();
    assert!((duration - 1.0).abs() < 0.01, "{duration}");
    assert!(!media.playing(), "opens paused");
    media.play();
    run_until(&mut media, &ctx, 5.0, |m| m.position() > 0.2);
    media.seek(0.6).unwrap();
    run_until(&mut media, &ctx, 5.0, |m| m.position() >= 0.6);
    run_until(&mut media, &ctx, 5.0, |m| !m.playing());
    assert!(media.position() > 0.99);
    // Played to the end, it starts again.
    media.play();
    run_until(&mut media, &ctx, 5.0, |m| m.playing() && m.position() < 0.5);
}

/// Needs ffmpeg on the PATH (or installed by the IDE); skipped without it.
#[test]
fn ffmpeg_plays_a_video_with_its_sound_seeks_and_ends() {
    let Some(tools) = ffmpeg::locate() else {
        return;
    };
    let path = temp("clip.mkv");
    let made = ffmpeg::quiet(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=size=320x240:rate=25:duration=2")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
        .args(["-c:v", "mpeg4", "-c:a", "aac", "-shortest"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success());
    let probe = ffmpeg::probe(&tools, &path).unwrap();
    let video = probe.video.unwrap();
    assert_eq!((video.width, video.height, video.fps), (320, 240, 25.0));
    assert!(probe.audio);
    assert!((probe.duration.unwrap() - 2.0).abs() < 0.2);

    let ctx = egui::Context::default();
    let mut media = Media::Streamed(Box::new(Streamed::new(
        &path,
        &tools,
        probe,
        Some(&speakers()),
    )));
    assert!(media.is_video());
    // Paused, the first picture is shown.
    run_until(&mut media, &ctx, 10.0, |m| m.picture().is_some());
    assert_eq!(media.picture().unwrap().size(), [320, 240]);
    assert_eq!(media.position(), 0.0);
    media.play();
    run_until(&mut media, &ctx, 10.0, |m| m.position() > 0.4);
    media.seek(1.5).unwrap();
    assert!((media.position() - 1.5).abs() < 0.1, "{}", media.position());
    run_until(&mut media, &ctx, 10.0, |m| !m.playing());
    assert!(media.position() > 1.9, "{}", media.position());
    assert_eq!(media.error(), None);
    // From the start again.
    media.play();
    run_until(&mut media, &ctx, 10.0, |m| {
        m.playing() && m.position() < 1.0
    });
}

#[test]
fn without_a_sound_device_a_video_plays_silently() {
    let Some(tools) = ffmpeg::locate() else {
        return;
    };
    let path = temp("silent.avi");
    let made = ffmpeg::quiet(&tools.ffmpeg)
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=size=160x120:rate=10:duration=1")
        .args(["-c:v", "mjpeg"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success());
    let probe = ffmpeg::probe(&tools, &path).unwrap();
    assert!(!probe.audio);
    let ctx = egui::Context::default();
    let mut media = Media::Streamed(Box::new(Streamed::new(&path, &tools, probe, None)));
    media.play();
    let started = Instant::now();
    run_until(&mut media, &ctx, 10.0, |m| !m.playing());
    // In real time, not as fast as ffmpeg decodes.
    assert!(started.elapsed() > Duration::from_millis(800));
    assert!(media.position() > 0.9);
}
