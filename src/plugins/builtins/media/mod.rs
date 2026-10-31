//! Media player built-in plugin: music (MP3, WAV, FLAC, OGG, M4A…) and videos (MP4, MKV, AVI,
//! MOV, WebM…) open in a player instead of the text editor. Music rodio can read plays without
//! anything else; videos need ffmpeg (`ffmpeg`), found on the PATH or installed by the plugin.
//! A file opens paused, and pauses when its tab is no longer shown.

pub mod ffmpeg;
pub mod playback;

use crate::{
    plugins::builtin::{BuiltinPlugin, EditorLayout, Host},
    ui,
};
use eframe::egui::{
    self, Align, Color32, Key, Layout, Modifiers, Rect, RichText, Sense, Stroke, UiBuilder, pos2,
    vec2,
};
use ffmpeg::{Install, Probe, Tools};
use playback::{Media, Streamed};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::SystemTime,
};

const AUDIO: [&str; 8] = ["mp3", "wav", "flac", "ogg", "oga", "m4a", "aac", "opus"];
const VIDEO: [&str; 10] = [
    "mp4", "m4v", "mkv", "avi", "mov", "webm", "wmv", "flv", "mpg", "mpeg",
];
/// Height of the bar of controls.
const BAR: f32 = 44.0;
/// Seconds an arrow key moves.
const STEP: f64 = 5.0;

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

pub fn is_audio(path: &Path) -> bool {
    extension(path).is_some_and(|e| AUDIO.contains(&e.as_str()))
}

pub fn is_video(path: &Path) -> bool {
    extension(path).is_some_and(|e| VIDEO.contains(&e.as_str()))
}

pub fn is_media(path: &Path) -> bool {
    is_audio(path) || is_video(path)
}

/// Seconds as `1:05`, or `1:02:03` past an hour.
pub fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// ffmpeg, looked for once (and again after an installation).
enum Lookup {
    Unknown,
    Looking(JoinHandle<Option<Tools>>),
    Done(Option<Tools>),
}

enum State {
    /// Needs ffmpeg, which is being looked for.
    NeedsTools,
    Probing(JoinHandle<Result<(Tools, Probe), String>>),
    Ready(Box<Media>),
    MissingFfmpeg,
    Failed(String),
}

struct Entry {
    modified: Option<SystemTime>,
    state: State,
    /// The pass it was last drawn in: a tab no longer shown pauses.
    drawn: u64,
    /// Where the position slider is held, until it is let go.
    dragging: Option<f64>,
}

pub struct MediaPlugin {
    entries: HashMap<PathBuf, Entry>,
    /// The sound device, opened when a first file plays.
    output: Option<Result<rodio::MixerDeviceSink, String>>,
    lookup: Lookup,
    install: Arc<Mutex<Install>>,
    volume: f32,
    muted: bool,
}

impl Default for MediaPlugin {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            output: None,
            lookup: Lookup::Unknown,
            install: Arc::default(),
            volume: 1.0,
            muted: false,
        }
    }
}

impl MediaPlugin {
    fn mixer(&mut self) -> Result<rodio::mixer::Mixer, String> {
        let output = self.output.get_or_insert_with(|| {
            rodio::DeviceSinkBuilder::open_default_sink()
                .map(|mut sink| {
                    sink.log_on_drop(false);
                    sink
                })
                .map_err(|e| e.to_string())
        });
        match output {
            Ok(sink) => Ok(sink.mixer().clone()),
            Err(e) => Err(e.clone()),
        }
    }

    /// ffmpeg, once looked for: None while it is.
    fn tools(&mut self) -> Option<Option<Tools>> {
        if let Lookup::Looking(handle) = &self.lookup
            && handle.is_finished()
            && let Lookup::Looking(handle) = std::mem::replace(&mut self.lookup, Lookup::Unknown)
        {
            self.lookup = Lookup::Done(handle.join().ok().flatten());
        }
        match &self.lookup {
            Lookup::Unknown => {
                self.lookup = Lookup::Looking(thread::spawn(ffmpeg::locate));
                None
            }
            Lookup::Looking(_) => None,
            Lookup::Done(tools) => Some(tools.clone()),
        }
    }

    fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }

    /// What a file just opened needs: rodio plays the music it can read, ffmpeg the rest.
    fn open(&mut self, path: &Path) -> State {
        if is_audio(path) {
            let mixer = match self.mixer() {
                Ok(mixer) => mixer,
                Err(e) => return State::Failed(trf!("media.no_audio_output", e = e)),
            };
            if let Ok(decoded) = playback::decode(path, &mixer, None) {
                return State::Ready(Box::new(Media::Decoded(decoded)));
            }
        }
        State::NeedsTools
    }

    /// Moves a file on: ffmpeg found, the file read, the player made.
    fn advance(&mut self, path: &Path) {
        let Some(entry) = self.entries.get(path) else {
            return;
        };
        if matches!(entry.state, State::NeedsTools) {
            let Some(found) = self.tools() else { return };
            let state = match found {
                Some(tools) => {
                    let file = path.to_path_buf();
                    State::Probing(thread::spawn(move || {
                        ffmpeg::probe(&tools, &file).map(|probe| (tools, probe))
                    }))
                }
                None => State::MissingFfmpeg,
            };
            if let Some(entry) = self.entries.get_mut(path) {
                entry.state = state;
            }
            return;
        }
        if !matches!(&entry.state, State::Probing(handle) if handle.is_finished()) {
            return;
        }
        let Some(State::Probing(handle)) = self
            .entries
            .get_mut(path)
            .map(|e| std::mem::replace(&mut e.state, State::NeedsTools))
        else {
            return;
        };
        let state = match handle.join() {
            Ok(Ok((tools, probe))) => self.streamed(path, &tools, probe),
            Ok(Err(e)) => State::Failed(e),
            Err(_) => State::Failed(tr!("media.unreadable_file").into()),
        };
        if let Some(entry) = self.entries.get_mut(path) {
            entry.state = state;
        }
    }

    fn streamed(&mut self, path: &Path, tools: &Tools, probe: Probe) -> State {
        if probe.video.is_none() && !probe.audio {
            return State::Failed(tr!("media.nothing_to_play").into());
        }
        let mixer = match self.mixer() {
            Ok(mixer) => Some(mixer),
            // A video plays without sound; a sound cannot.
            Err(e) if probe.video.is_none() => {
                return State::Failed(trf!("media.no_audio_output", e = e));
            }
            Err(_) => None,
        };
        let mut media =
            Media::Streamed(Box::new(Streamed::new(path, tools, probe, mixer.as_ref())));
        media.set_volume(self.effective_volume());
        State::Ready(Box::new(media))
    }

    fn missing_ffmpeg(&mut self, ui: &mut egui::Ui, path: &Path) {
        let t = ui::theme(ui.ctx());
        let install = self.install.lock().unwrap().clone();
        let mut download = false;
        let mut look_again = false;
        centered(ui, 460.0, |ui| {
            ui.label(
                RichText::new(tr!("media.ffmpeg_needed"))
                    .font(ui::semibold(15.0))
                    .color(t.text),
            );
            ui.add_space(6.0);
            ui.label(RichText::new(tr!("media.ffmpeg_explained")).color(t.text_secondary));
            ui.add_space(12.0);
            match &install {
                Install::Downloading { done, total } => {
                    let megabytes = |bytes: u64| bytes as f64 / 1_048_576.0;
                    let (fraction, text) = match total {
                        Some(total) if *total > 0 => (
                            *done as f32 / *total as f32,
                            trf!(
                                "media.downloading_of",
                                done = format!("{:.0}", megabytes(*done)),
                                total = format!("{:.0}", megabytes(*total))
                            ),
                        ),
                        _ => (
                            0.0,
                            trf!(
                                "media.downloading",
                                done = format!("{:.0}", megabytes(*done))
                            ),
                        ),
                    };
                    ui.add(egui::ProgressBar::new(fraction).text(text));
                }
                Install::Unpacking => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(t.accent));
                        ui.label(tr!("media.unpacking"));
                    });
                }
                Install::Idle | Install::Failed(_) | Install::Done => {
                    if let Install::Failed(e) = &install {
                        ui.label(RichText::new(trf!("media.install_failed", e = e)).color(t.error));
                        ui.add_space(8.0);
                    }
                    if ffmpeg::platform().is_some() {
                        ui.horizontal(|ui| {
                            download =
                                ui::primary_button(ui, tr!("media.download_ffmpeg")).clicked();
                            look_again = ui::button(ui, tr!("media.look_again")).clicked();
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(trf!(
                                "media.download_details",
                                dir = ffmpeg::managed_dir().display()
                            ))
                            .small()
                            .color(t.text_secondary),
                        );
                    } else {
                        ui.label(
                            RichText::new(tr!("media.install_with_package_manager"))
                                .color(t.text_secondary),
                        );
                        ui.add_space(8.0);
                        look_again = ui::button(ui, tr!("media.look_again")).clicked();
                    }
                }
            }
        });
        if download {
            ffmpeg::install(self.install.clone());
        }
        if look_again {
            self.lookup = Lookup::Unknown;
            if let Some(entry) = self.entries.get_mut(path) {
                entry.state = State::NeedsTools;
            }
        }
        if matches!(install, Install::Downloading { .. } | Install::Unpacking) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn player(&mut self, ui: &mut egui::Ui, path: &Path) {
        let t = ui::theme(ui.ctx());
        let volume = self.effective_volume();
        let Some(entry) = self.entries.get_mut(path) else {
            return;
        };
        let State::Ready(media) = &mut entry.state else {
            return;
        };
        media.set_volume(volume);
        if let Some(after) = media.tick(ui.ctx()) {
            ui.ctx().request_repaint_after(after);
        }
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let stage = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - BAR));
        let bar = Rect::from_min_max(pos2(rect.min.x, rect.max.y - BAR), rect.max);
        let painter = ui.painter_at(rect);
        let video = media.is_video();
        painter.rect_filled(stage, 0.0, if video { Color32::BLACK } else { t.frame });

        // The picture, as large as the stage allows, or the card of a song.
        if let Some(texture) = media.picture() {
            let size = texture.size_vec2();
            let scale = (stage.width() / size.x).min(stage.height() / size.y);
            let shown = Rect::from_center_size(stage.center(), size * scale);
            painter.image(
                texture.id(),
                shown,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else if !video {
            let icon = Rect::from_center_size(stage.center() - vec2(0.0, 24.0), vec2(96.0, 96.0));
            egui::Image::new(ui::icon_source("media/icon", t.dark)).paint_at(ui, icon);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            painter.text(
                pos2(stage.center().x, icon.max.y + 18.0),
                egui::Align2::CENTER_TOP,
                name.unwrap_or_default(),
                ui::semibold(15.0),
                t.text,
            );
            if let Some(duration) = media.duration() {
                painter.text(
                    pos2(stage.center().x, icon.max.y + 42.0),
                    egui::Align2::CENTER_TOP,
                    format_time(duration),
                    egui::FontId::proportional(12.5),
                    t.text_secondary,
                );
            }
        }
        if let Some(error) = media.error() {
            painter.text(
                stage.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(13.0),
                t.error,
            );
        }
        let stage_response =
            ui.interact(stage, ui.id().with(("media_stage", path)), Sense::click());
        let mut toggle = stage_response.clicked();

        // The bar: play, time, position, volume.
        painter.rect_filled(bar, 0.0, t.island);
        painter.hline(bar.x_range(), bar.min.y, Stroke::new(1.0_f32, t.separator));
        let mut seek_to = None;
        let mut volume_changed = None;
        let mut mute_clicked = false;
        let mut bar_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(bar.shrink2(vec2(10.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let playing = media.playing();
        let (icon, tip) = if playing {
            ("media/icons/pause", tr!("media.pause"))
        } else {
            ("run", tr!("media.play"))
        };
        toggle |= ui::icon_button(&mut bar_ui, icon, tip).clicked();
        bar_ui.add_space(6.0);
        let position = entry.dragging.unwrap_or_else(|| media.position());
        let duration = media.duration();
        let time = match duration {
            Some(duration) => format!("{} / {}", format_time(position), format_time(duration)),
            None => format_time(position),
        };
        bar_ui.label(
            RichText::new(time)
                .monospace()
                .size(12.0)
                .color(t.text_secondary),
        );
        bar_ui.add_space(10.0);
        let right = 140.0;
        if let Some(duration) = duration {
            let width = (bar_ui.available_width() - right).max(60.0);
            bar_ui.spacing_mut().slider_width = width;
            let mut value = position;
            let response = bar_ui.add(
                egui::Slider::new(&mut value, 0.0..=duration)
                    .show_value(false)
                    .trailing_fill(true),
            );
            if response.is_pointer_button_down_on() {
                entry.dragging = Some(value);
            } else if let Some(at) = entry.dragging.take() {
                seek_to = Some(at);
            }
        } else {
            bar_ui.add_space((bar_ui.available_width() - right).max(0.0));
        }
        bar_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut level = self.volume;
            ui.spacing_mut().slider_width = 80.0;
            if ui
                .add(egui::Slider::new(&mut level, 0.0..=1.0).show_value(false))
                .on_hover_text(tr!("media.volume"))
                .changed()
            {
                volume_changed = Some(level);
            }
            let (icon, tip) = if self.muted || self.volume == 0.0 {
                ("media/icons/mute", tr!("media.unmute"))
            } else {
                ("media/icons/volume", tr!("media.mute"))
            };
            mute_clicked = ui::icon_button(ui, icon, tip).clicked();
        });

        // Space plays or pauses, the arrows move by five seconds, while the pointer is on
        // the player and no field has the keyboard.
        let pointer_here = ui.rect_contains_pointer(rect);
        let free = ui.ctx().memory(|m| m.focused().is_none());
        if pointer_here && free {
            ui.input_mut(|i| {
                toggle |= i.consume_key(Modifiers::NONE, Key::Space);
                if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                    seek_to = Some(position - STEP);
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                    seek_to = Some(position + STEP);
                }
            });
        }
        if let Some(at) = seek_to {
            let at = duration.map_or(at, |d| at.min(d));
            // A file rodio cannot seek in stays where it is.
            let _ = media.seek(at);
        }
        if toggle {
            if media.playing() {
                media.pause();
            } else {
                media.play();
            }
            ui.ctx().request_repaint();
        }
        if let Some(level) = volume_changed {
            self.volume = level;
            self.muted = false;
        }
        if mute_clicked {
            if self.volume == 0.0 {
                self.volume = 1.0;
                self.muted = false;
            } else {
                self.muted = !self.muted;
            }
        }
    }
}

/// A column of `width` in the middle of the space left.
fn centered(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let rect = ui.available_rect_before_wrap();
    let column = Rect::from_center_size(
        rect.center(),
        vec2(width.min(rect.width() - 32.0).max(120.0), rect.height()),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(column)
            .layout(Layout::top_down(Align::Min).with_main_align(Align::Center)),
        add,
    );
}

fn waiting(ui: &mut egui::Ui, text: &str) {
    let t = ui::theme(ui.ctx());
    centered(ui, 360.0, |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0).color(t.accent));
            ui.label(RichText::new(text).color(t.text_secondary));
        });
    });
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
}

impl BuiltinPlugin for MediaPlugin {
    fn id(&self) -> &'static str {
        "media"
    }
    fn name(&self) -> &'static str {
        tr!("media.media_player")
    }
    fn description(&self) -> &'static str {
        tr!("media.plays_music_and_videos")
    }
    fn icon(&self) -> &'static str {
        "media/icon"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("media")
    }

    fn opens_binary(&self, path: &Path) -> bool {
        is_media(path)
    }
    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        is_media(path).then_some(EditorLayout::Preview)
    }
    fn layout_switch(&self, _path: &Path) -> bool {
        false
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        // A closed tab stops its file; a tab no longer shown pauses it.
        let open: HashSet<PathBuf> = host
            .open_documents()
            .into_iter()
            .filter_map(|d| d.path)
            .collect();
        self.entries.retain(|path, _| open.contains(path));
        let pass = ctx.cumulative_pass_nr();
        for entry in self.entries.values_mut() {
            if entry.drawn + 2 < pass
                && let State::Ready(media) = &mut entry.state
                && media.playing()
            {
                media.pause();
            }
        }
        // ffmpeg just installed: the files waiting for it go on.
        let installed = {
            let mut install = self.install.lock().unwrap();
            let done = *install == Install::Done;
            if done {
                *install = Install::Idle;
            }
            done
        };
        if installed {
            self.lookup = Lookup::Unknown;
            for entry in self.entries.values_mut() {
                if matches!(entry.state, State::MissingFfmpeg) {
                    entry.state = State::NeedsTools;
                }
            }
        }
    }

    fn preview(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        _text: &mut String,
        _scroll: Option<f32>,
    ) {
        let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
        if self
            .entries
            .get(path)
            .is_none_or(|e| e.modified != modified)
        {
            let state = self.open(path);
            self.entries.insert(
                path.to_path_buf(),
                Entry {
                    modified,
                    state,
                    drawn: 0,
                    dragging: None,
                },
            );
        }
        self.advance(path);
        if let Some(entry) = self.entries.get_mut(path) {
            entry.drawn = ui.ctx().cumulative_pass_nr();
        }
        let t = ui::theme(ui.ctx());
        match &self.entries[path].state {
            State::NeedsTools => waiting(ui, tr!("media.looking_for_ffmpeg")),
            State::Probing(_) => waiting(ui, tr!("media.reading_file")),
            State::MissingFfmpeg => self.missing_ffmpeg(ui, path),
            State::Failed(e) => {
                let e = e.clone();
                centered(ui, 460.0, |ui| {
                    ui.label(RichText::new(e).color(t.error));
                });
            }
            State::Ready(_) => self.player(ui, path),
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/media/mod/tests.rs"]
mod tests;
