use super::*;

#[test]
fn music_and_videos_open_in_the_player() {
    let plugin = MediaPlugin::default();
    for name in ["a/Song.MP3", "x.wav", "x.flac", "x.ogg", "x.m4a", "x.opus"] {
        assert!(is_audio(Path::new(name)), "{name}");
        assert!(plugin.opens_binary(Path::new(name)), "{name}");
    }
    for name in ["clip.mp4", "Film.MKV", "old.avi", "x.mov", "x.webm"] {
        assert!(is_video(Path::new(name)), "{name}");
        assert_eq!(
            plugin.editor_layout(Path::new(name)),
            Some(EditorLayout::Preview)
        );
    }
    for name in ["notes.txt", "mp3", "x.mp3.txt", "photo.png"] {
        assert!(!is_media(Path::new(name)), "{name}");
    }
    assert!(!plugin.layout_switch(Path::new("x.mp4")));
}

#[test]
fn times_read_as_a_player_shows_them() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(5.9), "0:05");
    assert_eq!(format_time(65.0), "1:05");
    assert_eq!(format_time(3723.4), "1:02:03");
    assert_eq!(format_time(-3.0), "0:00");
}
