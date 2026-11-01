//! 3D model viewer built-in plugin: glTF files (`.glb`, `.gltf`) open in a viewer instead of the
//! text editor, with an orbit camera and a player for their animations.
//!
//! The file is read in the background (`scene.rs`), then drawn with OpenGL (`render.rs`).
//! Dragging turns around the model, a right or middle drag (or Shift and drag) moves it, the
//! wheel zooms and a double click frames it again. Space plays or pauses the animation.

pub mod render;
pub mod scene;

use crate::{
    plugins::builtin::{BuiltinPlugin, EditorLayout},
    ui,
};
use eframe::egui::{
    self, Align, Key, Layout, Modifiers, PointerButton, Rect, RichText, Sense, Stroke, UiBuilder,
    pos2, vec2,
};
use glam::{Mat4, Vec3};
use render::Gpu;
use scene::{Draw, Scene};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::SystemTime,
};

/// Height of the bar of controls.
const BAR: f32 = 44.0;
/// Vertical field of view, in radians.
const FOV: f32 = 0.785;
/// The speeds offered for an animation.
const SPEEDS: [f32; 6] = [0.25, 0.5, 1.0, 1.5, 2.0, 4.0];

pub fn is_model(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "glb" | "gltf"))
}

/// A number that names each model loaded, for its buffers on the GPU.
fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A camera turning around a point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Orbit {
    pub target: Vec3,
    /// Radians around the vertical axis, and above the horizon.
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Orbit {
    /// Looking at the whole box, from the front, a little above and to the right.
    pub fn framing(min: Vec3, max: Vec3) -> Self {
        let radius = ((max - min).length() / 2.0).max(1e-4);
        Self {
            target: (min + max) / 2.0,
            yaw: 0.6,
            pitch: 0.35,
            distance: radius / (FOV / 2.0).sin() * 1.05,
        }
    }

    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(cp * sy, sp, cp * cy) * self.distance
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Y)
    }

    pub fn rotate(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.01;
        self.pitch = (self.pitch + dy * 0.01).clamp(-1.55, 1.55);
    }

    /// Moves the target by a drag of `dx`, `dy` points in a view `height` points high.
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        let view = self.view();
        let right = Vec3::new(view.x_axis.x, view.y_axis.x, view.z_axis.x);
        let up = Vec3::new(view.x_axis.y, view.y_axis.y, view.z_axis.y);
        let scale = 2.0 * self.distance * (FOV / 2.0).tan() / height.max(1.0);
        self.target += (-right * dx + up * dy) * scale;
    }

    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).max(1e-5);
    }
}

/// The animation player of a model.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Player {
    pub animation: Option<usize>,
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub looping: bool,
}

impl Player {
    pub fn new(animations: usize) -> Self {
        Self {
            animation: (animations > 0).then_some(0),
            time: 0.0,
            playing: animations > 0,
            speed: 1.0,
            looping: true,
        }
    }

    /// Moves time on by `dt` seconds of an animation of `duration`.
    pub fn advance(&mut self, dt: f32, duration: f32) {
        if !self.playing || self.animation.is_none() {
            return;
        }
        if duration <= 0.0 {
            self.time = 0.0;
            return;
        }
        self.time += dt * self.speed;
        if self.time >= duration {
            if self.looping {
                self.time %= duration;
            } else {
                self.time = duration;
                self.playing = false;
            }
        }
    }

    /// Play from the start when the end was reached.
    pub fn toggle(&mut self, duration: f32) {
        if !self.playing && !self.looping && self.time >= duration {
            self.time = 0.0;
        }
        self.playing = !self.playing;
    }
}

struct Model {
    id: u64,
    scene: Arc<Scene>,
    camera: Orbit,
    home: Orbit,
    player: Player,
    grid: Arc<Vec<f32>>,
    /// The pose drawn, and what it was computed for.
    draws: Arc<Vec<Draw>>,
    posed: (Option<usize>, f32),
    generation: u64,
    show_grid: bool,
    dragging: Option<f32>,
}

impl Model {
    fn new(scene: Scene, grid_color: [f32; 3]) -> Self {
        let scene = Arc::new(scene);
        let player = Player::new(scene.animations.len());
        let draws = scene.draws(&scene.pose(player.animation, 0.0));
        let (min, max) = scene
            .bounds(&draws)
            .unwrap_or((Vec3::splat(-1.0), Vec3::ONE));
        let home = Orbit::framing(min, max);
        Self {
            id: next_id(),
            grid: Arc::new(render::grid(min, max, grid_color)),
            scene,
            camera: home,
            home,
            player,
            draws: Arc::new(draws),
            posed: (player.animation, 0.0),
            generation: next_id(),
            show_grid: true,
            dragging: None,
        }
    }

    fn duration(&self) -> f32 {
        self.player
            .animation
            .and_then(|a| self.scene.animations.get(a))
            .map_or(0.0, |a| a.duration)
    }

    /// Computes the pose again when the animation or its time changed.
    fn pose(&mut self) {
        let wanted = (self.player.animation, self.player.time);
        if wanted != self.posed {
            let pose = self.scene.pose(wanted.0, wanted.1);
            self.draws = Arc::new(self.scene.draws(&pose));
            self.posed = wanted;
            self.generation = next_id();
        }
    }
}

enum State {
    Loading(Receiver<Result<Scene, String>>),
    Ready(Box<Model>),
    Failed(String),
}

struct Entry {
    modified: Option<SystemTime>,
    state: State,
}

#[derive(Default)]
pub struct GltfPlugin {
    entries: HashMap<PathBuf, Entry>,
    gpu: Arc<Mutex<Gpu>>,
}

pub fn format_time(seconds: f32) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor();
    format!("{:.0}:{:05.2}", minutes, seconds - minutes * 60.0)
}

fn srgb(c: egui::Color32) -> [f32; 3] {
    [c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0)
}

impl GltfPlugin {
    fn open(ctx: &egui::Context, path: &Path) -> State {
        let (send, receive) = mpsc::channel();
        let path = path.to_path_buf();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = send.send(scene::load(&path));
            ctx.request_repaint();
        });
        State::Loading(receive)
    }

    fn forget(&mut self, entry: Entry) {
        if let State::Ready(model) = entry.state {
            self.gpu.lock().unwrap().forget(model.id);
        }
    }

    fn viewer(&mut self, ui: &mut egui::Ui, path: &Path) {
        let t = ui::theme(ui.ctx());
        let gpu_error = self.gpu.lock().unwrap().error().map(str::to_string);
        let Some(Entry {
            state: State::Ready(model),
            ..
        }) = self.entries.get_mut(path)
        else {
            return;
        };
        let rect = ui.available_rect_before_wrap();
        let stage = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - BAR));
        let bar = Rect::from_min_max(pos2(rect.min.x, rect.max.y - BAR), rect.max);
        ui.allocate_rect(rect, Sense::hover());

        // The camera.
        let response = ui.interact(
            stage,
            ui.id().with(("gltf_stage", path)),
            Sense::click_and_drag(),
        );
        let delta = response.drag_delta();
        let shift = ui.input(|i| i.modifiers.shift);
        if response.dragged_by(PointerButton::Secondary)
            || response.dragged_by(PointerButton::Middle)
            || (response.dragged_by(PointerButton::Primary) && shift)
        {
            model.camera.pan(delta.x, delta.y, stage.height());
        } else if response.dragged_by(PointerButton::Primary) {
            model.camera.rotate(delta.x, delta.y);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                model.camera.zoom((-scroll * 0.002).exp());
            }
        }
        if response.double_clicked() {
            model.camera = model.home;
        }

        // The animation.
        let duration = model.duration();
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        model.player.advance(dt, duration);
        if model.player.playing {
            ui.ctx().request_repaint();
        }
        model.pose();

        let camera = model.camera;
        let view = camera.view();
        let aspect = stage.width() / stage.height().max(1.0);
        let near = (camera.distance * 0.01).max(1e-5);
        let projection = Mat4::perspective_rh_gl(FOV, aspect, near, camera.distance * 100.0);
        let eye = camera.eye();
        let forward = (camera.target - eye).normalize_or(Vec3::NEG_Z);
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward);
        let light = (-right * 0.45 + up * 0.75 - forward * 0.5).normalize();
        let empty = Arc::new(Vec::new());
        ui.painter().add(render::callback(
            self.gpu.clone(),
            stage,
            render::Frame {
                model: model.id,
                scene: model.scene.clone(),
                generation: model.generation,
                draws: model.draws.clone(),
                view,
                projection,
                eye,
                light,
                background: srgb(t.island),
                grid: if model.show_grid {
                    model.grid.clone()
                } else {
                    empty
                },
            },
        ));

        let painter = ui.painter_at(rect);
        let scene = &model.scene;
        let mut info = trf!(
            "gltf.statistics",
            triangles = scene.triangles,
            vertices = scene.vertices,
            meshes = scene.meshes.len()
        );
        if !scene.skins.is_empty() {
            info.push_str(&trf!("gltf.skins", n = scene.skins.len()));
        }
        painter.text(
            stage.min + vec2(12.0, 10.0),
            egui::Align2::LEFT_TOP,
            info,
            egui::FontId::proportional(12.0),
            t.text_secondary,
        );
        if let Some(e) = gpu_error {
            painter.text(
                stage.center(),
                egui::Align2::CENTER_CENTER,
                e,
                egui::FontId::proportional(13.0),
                t.error,
            );
        }
        response.on_hover_text_at_pointer(tr!("gltf.controls"));

        // The bar of controls.
        painter.rect_filled(bar, 0.0, t.island);
        painter.hline(bar.x_range(), bar.min.y, Stroke::new(1.0_f32, t.separator));
        let mut bar_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(bar.shrink2(vec2(10.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let mut toggle = false;
        let has_animation = !scene.animations.is_empty();
        if has_animation {
            let (icon, tip) = if model.player.playing {
                ("media/icons/pause", tr!("gltf.pause"))
            } else {
                ("run", tr!("gltf.play"))
            };
            toggle |= ui::icon_button(&mut bar_ui, icon, tip).clicked();
            bar_ui.add_space(6.0);
            let position = model.dragging.unwrap_or(model.player.time);
            bar_ui.label(
                RichText::new(format!(
                    "{} / {}",
                    format_time(position),
                    format_time(duration)
                ))
                .monospace()
                .size(12.0)
                .color(t.text_secondary),
            );
            bar_ui.add_space(10.0);
            let right = 330.0;
            let width = (bar_ui.available_width() - right).max(60.0);
            bar_ui.spacing_mut().slider_width = width;
            let mut value = position;
            let slider = bar_ui.add(
                egui::Slider::new(&mut value, 0.0..=duration.max(1e-3))
                    .show_value(false)
                    .trailing_fill(true),
            );
            if slider.is_pointer_button_down_on() {
                // Scrubbing shows the pose under the pointer.
                model.dragging = Some(value);
                model.player.time = value;
            } else if let Some(at) = model.dragging.take() {
                model.player.time = at;
            }
        } else {
            bar_ui.label(
                RichText::new(tr!("gltf.no_animation"))
                    .size(12.0)
                    .color(t.text_secondary),
            );
        }
        let player = &mut model.player;
        let show_grid = &mut model.show_grid;
        let animations = &scene.animations;
        bar_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui::icon_toggle(ui, "showAsTree", *show_grid, tr!("gltf.grid")).clicked() {
                *show_grid = !*show_grid;
            }
            if !has_animation {
                return;
            }
            if ui::icon_toggle(ui, "refresh", player.looping, tr!("gltf.loop")).clicked() {
                player.looping = !player.looping;
            }
            egui::ComboBox::from_id_salt("gltf_speed")
                .width(64.0)
                .selected_text(format!("{}×", player.speed))
                .show_ui(ui, |ui| {
                    for speed in SPEEDS {
                        ui.selectable_value(&mut player.speed, speed, format!("{speed}×"));
                    }
                })
                .response
                .on_hover_text(tr!("gltf.speed"));
            let current = player
                .animation
                .and_then(|a| animations.get(a))
                .map_or("", |a| a.name.as_str());
            egui::ComboBox::from_id_salt("gltf_animation")
                .width(150.0)
                .truncate()
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, animation) in animations.iter().enumerate() {
                        let text =
                            format!("{}  ({})", animation.name, format_time(animation.duration));
                        if ui
                            .selectable_label(player.animation == Some(i), text)
                            .clicked()
                        {
                            player.animation = Some(i);
                            player.time = 0.0;
                            player.playing = true;
                        }
                    }
                })
                .response
                .on_hover_text(tr!("gltf.animation"));
        });

        // Space plays or pauses, the arrows step by a tenth of a second.
        let pointer_here = ui.rect_contains_pointer(rect);
        let free = ui.ctx().memory(|m| m.focused().is_none());
        if has_animation && pointer_here && free {
            ui.input_mut(|i| {
                toggle |= i.consume_key(Modifiers::NONE, Key::Space);
                for (key, step) in [(Key::ArrowLeft, -0.1), (Key::ArrowRight, 0.1)] {
                    if i.consume_key(Modifiers::NONE, key) {
                        model.player.playing = false;
                        model.player.time = (model.player.time + step).clamp(0.0, duration);
                    }
                }
            });
        }
        if toggle {
            model.player.toggle(duration);
            ui.ctx().request_repaint();
        }
    }
}

impl BuiltinPlugin for GltfPlugin {
    fn id(&self) -> &'static str {
        "gltf"
    }
    fn name(&self) -> &'static str {
        tr!("gltf.model_viewer")
    }
    fn description(&self) -> &'static str {
        tr!("gltf.shows_gltf_models")
    }
    fn icon(&self) -> &'static str {
        "gltf/icon"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("gltf")
    }

    fn opens_binary(&self, path: &Path) -> bool {
        is_model(path)
    }
    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        is_model(path).then_some(EditorLayout::Preview)
    }
    fn layout_switch(&self, _path: &Path) -> bool {
        false
    }

    fn file_closed(&mut self, _ctx: &egui::Context, path: &Path) {
        if let Some(entry) = self.entries.remove(path) {
            self.forget(entry);
        }
    }

    fn preview(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        _text: &mut String,
        _scroll: Option<f32>,
    ) {
        let t = ui::theme(ui.ctx());
        let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
        if self
            .entries
            .get(path)
            .is_none_or(|e| e.modified != modified)
        {
            let state = Self::open(ui.ctx(), path);
            if let Some(old) = self
                .entries
                .insert(path.to_path_buf(), Entry { modified, state })
            {
                self.forget(old);
            }
        }
        let entry = self.entries.get_mut(path).unwrap();
        if let State::Loading(receive) = &entry.state {
            entry.state = match receive.try_recv() {
                Ok(Ok(scene)) => State::Ready(Box::new(Model::new(scene, srgb(t.text_secondary)))),
                Ok(Err(e)) => State::Failed(trf!("gltf.unreadable_model", e = e)),
                Err(TryRecvError::Empty) => {
                    let rect = ui.available_rect_before_wrap();
                    ui.scope_builder(
                        UiBuilder::new()
                            .max_rect(rect)
                            .layout(Layout::top_down(Align::Center).with_main_align(Align::Center)),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(14.0).color(t.accent));
                                ui.label(
                                    RichText::new(tr!("gltf.reading_model"))
                                        .color(t.text_secondary),
                                );
                            });
                        },
                    );
                    return;
                }
                Err(TryRecvError::Disconnected) => State::Failed(tr!("gltf.reading_failed").into()),
            };
        }
        match &self.entries[path].state {
            State::Ready(_) => self.viewer(ui, path),
            State::Failed(e) => {
                let rect = ui.available_rect_before_wrap();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    e,
                    egui::FontId::proportional(13.0),
                    t.error,
                );
            }
            State::Loading(_) => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/gltf/mod/tests.rs"]
mod tests;
