# Media player

The built-in **Media player** plugin opens music and videos in a player instead of the text editor.

| Kind | Files | Decoded by |
| --- | --- | --- |
| Music | `.mp3`, `.wav`, `.flac`, `.ogg`, `.m4a` | the IDE itself |
| Other sounds | `.opus`, `.aac`, `.oga` (and any music the IDE cannot read) | ffmpeg |
| Videos | `.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.m4v`, `.wmv`, `.flv`, `.mpg` | ffmpeg |

## Playing

- A file opens paused. **▶** or a click on the picture plays or pauses it, and the bar under it shows the time and moves through the file.
- While the pointer is on the player: **Space** plays or pauses, **←** and **→** move by 5 seconds.
- The volume and mute button are shared by every file.
- A file pauses when its tab is no longer shown, and stops when the tab is closed.
- Videos are shown at most at 1280 × 720 and 60 frames per second, scaled to the editor. A video filmed upright on a phone is shown upright. Without a sound device, a video plays silently.

## ffmpeg

Videos need `ffmpeg` and `ffprobe`. The IDE uses the ones on the `PATH`. When there are none, the player offers to **download ffmpeg**: the static LGPL build of [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (about 170 MB), checked against its SHA-256 and installed in `~/.atelier/tools/ffmpeg`, where only `ffmpeg` and `ffprobe` are kept. The download is offered on Windows (x64) and Linux (x64, ARM64). On macOS, install it with `brew install ffmpeg`, then click **Look again**.
