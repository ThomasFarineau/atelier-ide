//! ffmpeg and ffprobe, which read the videos: the ones the IDE installed, else the ones of the
//! PATH. The IDE installs the static LGPL build of BtbN/FFmpeg-Builds, checked against its
//! SHA-256, keeping only `ffmpeg` and `ffprobe`.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const RELEASE: &str = "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/latest";
const CHECKSUMS: &str = "checksums.sha256";

/// The programs that read the videos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

/// What ffprobe says of a file.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Probe {
    /// In seconds.
    pub duration: Option<f64>,
    pub video: Option<VideoStream>,
    pub audio: bool,
}

/// The first video stream, as ffmpeg decodes it (a rotated video is turned upright).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoStream {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// Where an installation is.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Install {
    #[default]
    Idle,
    /// Bytes received, and expected when the server says so.
    Downloading {
        done: u64,
        total: Option<u64>,
    },
    Unpacking,
    Failed(String),
    Done,
}

/// The folder the IDE installs ffmpeg in (`ATELIER_FFMPEG_DIR` overrides it, for tests).
pub fn managed_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ATELIER_FFMPEG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map_or_else(std::env::temp_dir, PathBuf::from);
    home.join(".atelier").join("tools").join("ffmpeg")
}

pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// A command without a console window, reading nothing.
pub fn quiet(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn runs(program: &Path) -> bool {
    if program.is_absolute() && !program.exists() {
        return false;
    }
    quiet(program)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The ffmpeg the IDE installed, else the one of the PATH. It starts both programs: call it
/// off the interface.
pub fn locate() -> Option<Tools> {
    let managed = managed_dir().join("bin");
    [
        (managed.join(exe("ffmpeg")), managed.join(exe("ffprobe"))),
        (PathBuf::from("ffmpeg"), PathBuf::from("ffprobe")),
    ]
    .into_iter()
    .find(|(ffmpeg, ffprobe)| runs(ffmpeg) && runs(ffprobe))
    .map(|(ffmpeg, ffprobe)| Tools { ffmpeg, ffprobe })
}

/// Asks ffprobe what `path` holds.
pub fn probe(tools: &Tools, path: &Path) -> Result<Probe, String> {
    let output = quiet(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if message.is_empty() {
            tr!("media.unreadable_file").into()
        } else {
            message
        });
    }
    let json: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    Ok(parse_probe(&json))
}

/// The answer of `ffprobe -print_format json -show_format -show_streams`.
pub fn parse_probe(json: &Value) -> Probe {
    let streams = json["streams"].as_array().map_or(&[][..], Vec::as_slice);
    let number = |v: &Value| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
    };
    // The cover of a song is a video stream of one picture.
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        .and_then(|s| {
            let mut width = u32::try_from(s["width"].as_u64()?).ok()?;
            let mut height = u32::try_from(s["height"].as_u64()?).ok()?;
            // ffmpeg turns the pictures of a video filmed upright (a phone) the right way up.
            let rotation = s["side_data_list"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|d| number(&d["rotation"]))
                .or_else(|| number(&s["tags"]["rotate"]))
                .unwrap_or(0.0);
            if (rotation.round().abs() as i64) % 180 == 90 {
                std::mem::swap(&mut width, &mut height);
            }
            let fps = [&s["avg_frame_rate"], &s["r_frame_rate"]]
                .into_iter()
                .find_map(|r| rate(r.as_str()?))
                .unwrap_or(25.0);
            (width > 0 && height > 0).then_some(VideoStream { width, height, fps })
        });
    Probe {
        duration: number(&json["format"]["duration"]).filter(|d| d.is_finite() && *d > 0.0),
        video,
        audio: streams.iter().any(|s| s["codec_type"] == "audio"),
    }
}

/// A frame rate written `30000/1001` or `25`.
pub fn rate(text: &str) -> Option<f64> {
    let (numerator, denominator) = text.split_once('/').unwrap_or((text, "1"));
    let numerator: f64 = numerator.trim().parse().ok()?;
    let denominator: f64 = denominator.trim().parse().ok()?;
    let rate = numerator / denominator;
    (numerator > 0.0 && denominator > 0.0 && rate.is_finite() && rate <= 1000.0).then_some(rate)
}

/// The name BtbN gives the builds of this system, None where there is none (macOS…).
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("win64"),
        ("linux", "x86_64") => Some("linux64"),
        ("linux", "aarch64") => Some("linuxarm64"),
        _ => None,
    }
}

/// The newest static LGPL release build for `platform` in a GitHub release: its name and URL.
pub fn pick_asset(release: &Value, platform: &str) -> Option<(String, String)> {
    let marker = format!("-latest-{platform}-lgpl-");
    release["assets"]
        .as_array()?
        .iter()
        .filter_map(|a| Some((a["name"].as_str()?, a["browser_download_url"].as_str()?)))
        .filter(|(name, _)| {
            name.starts_with("ffmpeg-n")
                && name.contains(&marker)
                && !name.contains("shared")
                && (name.ends_with(".zip") || name.ends_with(".tar.xz"))
        })
        .max_by_key(|(name, _)| version(name))
        .map(|(name, url)| (name.to_string(), url.to_string()))
}

/// `ffmpeg-n9.0-latest-…` → [9, 0].
fn version(name: &str) -> Vec<u32> {
    name.trim_start_matches("ffmpeg-n")
        .split('-')
        .next()
        .unwrap_or_default()
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect()
}

/// The SHA-256 of `name` in a `checksums.sha256` file.
pub fn checksum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_ascii_lowercase())
    })
}

/// The program an entry of the archive is, when it is one of the two kept.
pub fn wanted(entry: &str) -> Option<String> {
    let entry = entry.replace('\\', "/");
    [exe("ffmpeg"), exe("ffprobe")]
        .into_iter()
        .find(|program| entry.ends_with(&format!("/bin/{program}")))
}

/// Downloads and unpacks ffmpeg in the background; `state` says how it goes.
pub fn install(state: Arc<Mutex<Install>>) {
    {
        let mut current = state.lock().unwrap();
        if matches!(*current, Install::Downloading { .. } | Install::Unpacking) {
            return;
        }
        *current = Install::Downloading {
            done: 0,
            total: None,
        };
    }
    thread::spawn(move || {
        let result = download(&state);
        *state.lock().unwrap() = match result {
            Ok(()) => Install::Done,
            Err(e) => Install::Failed(e),
        };
    });
}

fn download(state: &Mutex<Install>) -> Result<(), String> {
    let platform = platform().ok_or(tr!("media.no_build_for_system"))?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(1800)))
        .build()
        .into();
    let text = |url: &str| -> Result<String, String> {
        agent
            .get(url)
            .header("User-Agent", "atelier-ide")
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())
    };
    let release: Value = serde_json::from_str(&text(RELEASE)?).map_err(|e| e.to_string())?;
    let (name, url) = pick_asset(&release, platform).ok_or(tr!("media.no_build_for_system"))?;
    let sums_url = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == CHECKSUMS)
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or(tr!("media.no_checksum"))?;
    let expected = checksum(&text(sums_url)?, &name).ok_or(tr!("media.no_checksum"))?;

    let target = managed_dir();
    let staging = target.with_extension("download");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(staging.join("bin")).map_err(|e| e.to_string())?;
    let archive = staging.join(&name);
    let mut response = agent
        .get(&url)
        .header("User-Agent", "atelier-ide")
        .call()
        .map_err(|e| e.to_string())?;
    let total = response.body().content_length();
    let mut reader = response.body_mut().as_reader();
    let mut file = BufWriter::new(File::create(&archive).map_err(|e| e.to_string())?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
        done += read as u64;
        *state.lock().unwrap() = Install::Downloading { done, total };
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != expected {
        let _ = fs::remove_dir_all(&staging);
        return Err(tr!("media.checksum_mismatch").into());
    }

    *state.lock().unwrap() = Install::Unpacking;
    let bin = staging.join("bin");
    let unpacked = if name.ends_with(".zip") {
        unpack_zip(&archive, &bin)
    } else {
        unpack_tar_xz(&archive, &staging, &bin)
    };
    let _ = fs::remove_file(&archive);
    let found = unpacked?;
    if found < 2 {
        let _ = fs::remove_dir_all(&staging);
        return Err(tr!("media.archive_without_ffmpeg").into());
    }
    let _ = fs::remove_dir_all(&target);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(&staging, &target).map_err(|e| e.to_string())
}

/// Copies the two programs out of a zip archive; returns how many it found.
fn unpack_zip(archive: &Path, bin: &Path) -> Result<usize, String> {
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut found = 0;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let Some(program) = wanted(entry.name()) else {
            continue;
        };
        let mut out = File::create(bin.join(program)).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        found += 1;
    }
    Ok(found)
}

/// Copies the two programs out of a `.tar.xz` archive, unpacked on disk first.
fn unpack_tar_xz(archive: &Path, staging: &Path, bin: &Path) -> Result<usize, String> {
    let tar_path = staging.join("ffmpeg.tar");
    {
        let mut input = BufReader::new(File::open(archive).map_err(|e| e.to_string())?);
        let mut output = BufWriter::new(File::create(&tar_path).map_err(|e| e.to_string())?);
        lzma_rs::xz_decompress(&mut input, &mut output).map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
    }
    let mut tar = tar::Archive::new(File::open(&tar_path).map_err(|e| e.to_string())?);
    let mut found = 0;
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();
        if let Some(program) = wanted(&path.to_string_lossy()) {
            entry.unpack(bin.join(program)).map_err(|e| e.to_string())?;
            found += 1;
        }
    }
    let _ = fs::remove_file(&tar_path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for program in ["ffmpeg", "ffprobe"] {
            let _ = fs::set_permissions(bin.join(program), fs::Permissions::from_mode(0o755));
        }
    }
    Ok(found)
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/media/ffmpeg/tests.rs"]
mod tests;
