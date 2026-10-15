use super::*;
use serde_json::json;

#[test]
fn a_video_with_sound_is_read_from_ffprobe() {
    let probe = parse_probe(&json!({
        "streams": [
            {"codec_type": "video", "width": 1920, "height": 1080, "avg_frame_rate": "30000/1001", "disposition": {"attached_pic": 0}},
            {"codec_type": "audio", "sample_rate": "48000"}
        ],
        "format": {"duration": "12.500000"}
    }));
    let video = probe.video.unwrap();
    assert_eq!((video.width, video.height), (1920, 1080));
    assert!((video.fps - 29.97).abs() < 0.01);
    assert!(probe.audio);
    assert_eq!(probe.duration, Some(12.5));
}

#[test]
fn a_phone_video_filmed_upright_is_turned() {
    let probe = parse_probe(&json!({
        "streams": [{"codec_type": "video", "width": 1920, "height": 1080, "avg_frame_rate": "0/0", "r_frame_rate": "30/1",
                     "side_data_list": [{"rotation": -90}]}],
        "format": {}
    }));
    let video = probe.video.unwrap();
    assert_eq!((video.width, video.height), (1080, 1920));
    assert_eq!(video.fps, 30.0, "0/0 is no rate");
    assert!(!probe.audio);
    assert_eq!(probe.duration, None);
}

#[test]
fn the_cover_of_a_song_is_not_a_video() {
    let probe = parse_probe(&json!({
        "streams": [
            {"codec_type": "audio"},
            {"codec_type": "video", "width": 500, "height": 500, "disposition": {"attached_pic": 1}}
        ],
        "format": {"duration": "200.1"}
    }));
    assert_eq!(probe.video, None);
    assert!(probe.audio);
}

#[test]
fn frame_rates() {
    assert_eq!(rate("25"), Some(25.0));
    assert_eq!(rate("50/2"), Some(25.0));
    assert_eq!(rate("0/0"), None);
    assert_eq!(rate("90000/1"), None, "a time base, not a rate");
    assert_eq!(rate("x"), None);
}

#[test]
fn the_newest_static_lgpl_build_of_the_system_is_chosen() {
    let asset = |name: &str| json!({"name": name, "browser_download_url": format!("https://example.com/{name}")});
    let release = json!({"assets": [
        asset("checksums.sha256"),
        asset("ffmpeg-n8.1-latest-win64-lgpl-8.1.zip"),
        asset("ffmpeg-n9.0-latest-win64-gpl-9.0.zip"),
        asset("ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip"),
        asset("ffmpeg-n9.0-latest-win64-lgpl-9.0.zip"),
        asset("ffmpeg-n9.0-latest-linux64-lgpl-9.0.tar.xz"),
        asset("ffmpeg-n10.0-latest-linuxarm64-lgpl-10.0.tar.xz"),
        asset("ffmpeg-master-latest-win64-lgpl.zip"),
    ]});
    assert_eq!(
        pick_asset(&release, "win64").unwrap().0,
        "ffmpeg-n9.0-latest-win64-lgpl-9.0.zip"
    );
    let (name, url) = pick_asset(&release, "linux64").unwrap();
    assert_eq!(name, "ffmpeg-n9.0-latest-linux64-lgpl-9.0.tar.xz");
    assert!(url.ends_with(&name));
    assert_eq!(pick_asset(&release, "macos"), None);
}

#[test]
fn checksums_and_archive_entries() {
    let sums = "ABCDEF  ffmpeg-n9.0-latest-win64-lgpl-9.0.zip\n123456 *ffmpeg-n9.0-latest-linux64-lgpl-9.0.tar.xz\n";
    assert_eq!(
        checksum(sums, "ffmpeg-n9.0-latest-win64-lgpl-9.0.zip").as_deref(),
        Some("abcdef")
    );
    assert_eq!(
        checksum(sums, "ffmpeg-n9.0-latest-linux64-lgpl-9.0.tar.xz").as_deref(),
        Some("123456")
    );
    assert_eq!(checksum(sums, "other.zip"), None);
    let program = exe("ffmpeg");
    assert_eq!(
        wanted(&format!("ffmpeg-n9.0-latest-win64-lgpl-9.0/bin/{program}")),
        Some(program.clone())
    );
    assert_eq!(
        wanted(&format!("ffmpeg-n9.0/bin/{}", exe("ffprobe"))),
        Some(exe("ffprobe"))
    );
    assert_eq!(wanted(&format!("ffmpeg-n9.0/bin/{}", exe("ffplay"))), None);
    assert_eq!(wanted(&format!("ffmpeg-n9.0/doc/{program}")), None);
}
