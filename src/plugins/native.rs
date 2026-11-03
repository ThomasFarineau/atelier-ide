//! Native plugins: external plugins whose manifest names a dynamic library (`library`). Once the
//! plugin is on, the library is loaded and the plugin it exports joins the built-in ones, as a
//! [`NativePlugin`].
//!
//! The library and the IDE share only C types and functions (`atelier_plugin_api::abi`): any
//! library of the same [`abi::VERSION`] loads, whatever compiler and crates built it. The plugin
//! draws with an egui of its own: each surface (the preview of a file, a tool window) sends it
//! the input of the frame, and paints the meshes and textures it gives back ([`Surface`]). A
//! library is never unloaded: its code may still be behind a paint callback; turning the plugin
//! on again makes a new plugin from it.

use super::{
    Plugin as Manifest,
    builtin::{BuiltinPlugin, Command, Dock, EditorLayout, Host, ToolWindow},
};
use atelier_plugin_api::{abi, protocol};
use eframe::egui::{
    self, Color32, ColorImage, ImageData, LayerId, Order, Pos2, Rect, Sense, Shape, TextureId,
    TextureOptions, epaint::ImageDelta, pos2,
};
use std::{
    collections::HashMap,
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
};

/// The libraries loaded, by file.
static LOADED: LazyLock<Mutex<HashMap<PathBuf, &'static abi::PluginApi>>> =
    LazyLock::new(Default::default);

/// Loads the library of `manifest` (once) and makes its plugin.
pub fn load(manifest: &Manifest) -> Result<NativePlugin, String> {
    let path = manifest
        .library_path()
        .ok_or_else(|| tr!("invalid_library_name").to_string())?;
    let api = {
        let mut loaded = LOADED.lock().unwrap();
        match loaded.get(&path) {
            Some(api) => *api,
            None => {
                let api = open(&path)?;
                loaded.insert(path.clone(), api);
                api
            }
        }
    };
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    let instance = Instance::new(api)
        .ok_or_else(|| trf!("native_library_error", path = path.display(), e = "create"))?;
    let mut plugin = NativePlugin {
        id: leak(&manifest.id),
        name: leak(&manifest.name),
        description: leak(&manifest.description),
        version: leak(&manifest.version),
        instance,
        info: protocol::Info::default(),
        language: String::new(),
        tool_windows: &[],
        commands: Vec::new(),
        icons: HashMap::new(),
        surfaces: HashMap::new(),
        ctx: None,
    };
    plugin.refresh_info();
    Ok(plugin)
}

/// Opens a library and checks the version of its interface.
fn open(path: &Path) -> Result<&'static abi::PluginApi, String> {
    if !path.is_file() {
        return Err(trf!("native_library_missing", path = path.display()));
    }
    let failed =
        |e: &dyn std::fmt::Display| trf!("native_library_error", path = path.display(), e = e);
    // SAFETY: loading a library runs its initializers: the user installed the plugin, and the
    // Plugins window tells that a native plugin runs inside the IDE.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| failed(&e))?;
    // SAFETY: the symbol is the `PluginApi` static of `export_plugin!`, valid while the library
    // is loaded (for the life of the IDE). Its `abi` comes first and is read before the rest.
    let api: &'static abi::PluginApi = unsafe {
        let symbol = library
            .get::<*const abi::PluginApi>(abi::SYMBOL)
            .map_err(|_| trf!("native_library_old", path = path.display()))?;
        &**symbol
    };
    if api.abi != abi::VERSION || api.size < std::mem::size_of::<abi::PluginApi>() {
        return Err(trf!(
            "native_library_mismatch",
            path = path.display(),
            built = api.abi,
            mine = abi::VERSION
        ));
    }
    // Kept loaded for the life of the IDE (see the module).
    std::mem::forget(library);
    Ok(api)
}

// ---- The host: what the plugin may ask the IDE ----------------------------------------------

enum Request {
    Notify(String, bool),
    OpenFile(PathBuf),
    ShowToolWindow(String),
}

/// What the plugin may read from the IDE at any time, from any thread.
#[derive(Default)]
struct Shared {
    ctx: Mutex<Option<egui::Context>>,
    requests: Mutex<Vec<Request>>,
    project: Mutex<protocol::Project>,
    /// The look of the IDE, as JSON, with its generation.
    style: Mutex<(u64, Vec<u8>)>,
}

impl Shared {
    fn call(&self, method: &str, input: &[u8]) -> Vec<u8> {
        let json = |value: &dyn erased::Serialize| value.to_json();
        match method {
            protocol::NOTIFY => {
                if let Ok(n) = serde_json::from_slice::<protocol::Notify>(input) {
                    self.request(Request::Notify(n.message, n.error));
                }
                Vec::new()
            }
            protocol::OPEN_FILE => {
                if let Ok(m) = serde_json::from_slice::<protocol::PathMessage>(input) {
                    self.request(Request::OpenFile(m.path));
                }
                Vec::new()
            }
            protocol::SHOW_TOOL_WINDOW => {
                if let Ok(m) = serde_json::from_slice::<protocol::IdMessage>(input) {
                    self.request(Request::ShowToolWindow(m.id));
                }
                Vec::new()
            }
            protocol::PROJECT => json(&*self.project.lock().unwrap()),
            protocol::STYLE => self.style.lock().unwrap().1.clone(),
            protocol::FONTS => fonts(),
            protocol::ICON => serde_json::from_slice::<protocol::IconRequest>(input)
                .ok()
                .and_then(|r| {
                    let icon = crate::icon_set::find(&r.name)
                        .or_else(|| crate::icon_set::find("moreHorizontal"))?;
                    let svg = icon.variant(r.dark)?.bytes;
                    rasterize(svg, r.size, r.size)
                })
                .unwrap_or_default(),
            protocol::SVG => serde_json::from_slice::<protocol::SvgRequest>(input)
                .ok()
                .and_then(|r| rasterize(r.svg.as_bytes(), r.width, r.height))
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn request(&self, request: Request) {
        self.requests.lock().unwrap().push(request);
        self.repaint();
    }

    fn repaint(&self) {
        if let Some(ctx) = &*self.ctx.lock().unwrap() {
            ctx.request_repaint();
        }
    }
}

/// `serde_json::to_vec` behind a trait object, for the answers of [`Shared::call`].
mod erased {
    pub trait Serialize {
        fn to_json(&self) -> Vec<u8>;
    }
    impl<T: serde::Serialize> Serialize for T {
        fn to_json(&self) -> Vec<u8> {
            serde_json::to_vec(self).unwrap_or_default()
        }
    }
}

/// The fonts of the IDE, for the egui of the plugins.
fn fonts() -> Vec<u8> {
    let (_, Some(definitions)) = crate::ui::installed_fonts() else {
        return Vec::new();
    };
    let family = |f: &egui::FontFamily| match f {
        egui::FontFamily::Proportional => "Proportional".to_string(),
        egui::FontFamily::Monospace => "Monospace".to_string(),
        egui::FontFamily::Name(name) => name.to_string(),
    };
    let fonts = protocol::Fonts {
        data: definitions
            .font_data
            .iter()
            .map(|(name, data)| protocol::FontData {
                name: name.clone(),
                bytes: data.font.to_vec(),
                index: data.index,
            })
            .collect(),
        families: definitions
            .families
            .iter()
            .map(|(f, names)| (family(f), names.clone()))
            .collect(),
    };
    protocol::encode_fonts(&fonts)
}

/// An SVG drawn to fit in `width` × `height` pixels, centered ([`protocol::encode_image`]).
fn rasterize(svg: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let (width, height) = (width.clamp(1, 4096), height.clamp(1, 4096));
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = (width as f32 / size.width()).min(height as f32 / size.height());
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    let dx = (width as f32 - size.width() * scale) / 2.0;
    let dy = (height as f32 - size.height() * scale) / 2.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy),
        &mut pixmap.as_mut(),
    );
    Some(protocol::encode_image(width, height, pixmap.data()))
}

unsafe extern "C" fn host_call(
    host: *mut c_void,
    method: abi::Str,
    input: abi::Str,
) -> abi::Buffer {
    // SAFETY: `host` is the `Shared` of an `Instance`, alive while its plugin is; the strings
    // are valid for the call.
    let (shared, method, input) =
        unsafe { (&*host.cast::<Shared>(), method.as_str(), input.as_bytes()) };
    let answer =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shared.call(method, input)))
            .unwrap_or_default();
    if answer.is_empty() {
        abi::Buffer::EMPTY
    } else {
        abi::Buffer::from_vec(answer)
    }
}

unsafe extern "C" fn host_free(_host: *mut c_void, buffer: abi::Buffer) {
    // SAFETY: made by `host_call`, from a vector of this library.
    drop(unsafe { buffer.into_vec() });
}

unsafe extern "C" fn host_repaint(host: *mut c_void) {
    // SAFETY: as in `host_call`.
    unsafe { &*host.cast::<Shared>() }.repaint();
}

// ---- The plugin ----------------------------------------------------------------------------

/// A plugin made by a library, with what it may call. The plugin is called under the lock,
/// from the thread of the interface (frames, calls) and of the painter (OpenGL callbacks).
struct Instance {
    api: &'static abi::PluginApi,
    plugin: Mutex<*mut c_void>,
    shared: Arc<Shared>,
    /// Kept where the plugin can read it until it is destroyed.
    _host: Box<abi::HostApi>,
}

// SAFETY: the plugin is only reached under the lock of `plugin`; the rest is thread-safe.
unsafe impl Send for Instance {}
unsafe impl Sync for Instance {}

impl Instance {
    fn new(api: &'static abi::PluginApi) -> Option<Arc<Self>> {
        let shared = Arc::new(Shared::default());
        let host = Box::new(abi::HostApi {
            size: std::mem::size_of::<abi::HostApi>(),
            host: Arc::as_ptr(&shared).cast_mut().cast(),
            call: host_call,
            free: host_free,
            request_repaint: host_repaint,
        });
        // SAFETY: the plugin keeps `host` until `destroy`, and `Instance` keeps it as long.
        let plugin = unsafe { (api.create)(&*host) };
        (!plugin.is_null()).then(|| {
            Arc::new(Self {
                api,
                plugin: Mutex::new(plugin),
                shared,
                _host: host,
            })
        })
    }

    /// Asks the plugin something ([`protocol`]); empty when it does not answer.
    fn call(&self, method: &str, input: &[u8]) -> Vec<u8> {
        let plugin = self.plugin.lock().unwrap();
        // SAFETY: the plugin is alive, and frees what it gave.
        unsafe {
            let buffer = (self.api.call)(*plugin, abi::Str::new(method), abi::Str::bytes(input));
            let bytes = buffer.as_bytes().to_vec();
            (self.api.free)(buffer);
            bytes
        }
    }

    fn call_json(&self, method: &str, input: &impl serde::Serialize) -> Vec<u8> {
        self.call(method, &serde_json::to_vec(input).unwrap_or_default())
    }

    fn paint(&self, callback: u64, info: &abi::PaintInfo) {
        let plugin = self.plugin.lock().unwrap();
        // SAFETY: called by the painter, with its OpenGL context current.
        unsafe { (self.api.paint)(*plugin, callback, info) };
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let plugin = *self.plugin.get_mut().unwrap();
        // SAFETY: nothing calls the plugin anymore.
        unsafe { (self.api.destroy)(plugin) };
    }
}

/// What the IDE keeps of a surface of the plugin.
#[derive(Default)]
struct Surface {
    /// The textures of the plugin in this surface, as the IDE's.
    textures: HashMap<u64, TextureId>,
    /// Where the windows and menus of the plugin were at the last frame.
    floating: Vec<Rect>,
    /// The pointer was over the surface at the last frame.
    hovered: bool,
    /// A button was pressed in the surface and is not released yet: the pointer stays the
    /// plugin's.
    captured: bool,
    /// The plugin failed in this surface.
    failed: bool,
}

/// A native plugin, as the IDE sees its built-in plugins.
pub struct NativePlugin {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    version: &'static str,
    instance: Arc<Instance>,
    info: protocol::Info,
    /// The language `info` was asked in.
    language: String,
    tool_windows: &'static [ToolWindow],
    commands: Vec<Command>,
    /// The keys of the IDE's icon set of the icons of the plugin, by their name.
    icons: HashMap<String, &'static str>,
    surfaces: HashMap<(u32, String), Surface>,
    ctx: Option<egui::Context>,
}

impl NativePlugin {
    /// Asks the plugin what it brings, in the language of the interface.
    fn refresh_info(&mut self) {
        let language = crate::i18n::current().to_string();
        let info = self.instance.call_json(
            protocol::INFO,
            &protocol::InfoRequest {
                language: language.clone(),
            },
        );
        self.info = serde_json::from_slice(&info).unwrap_or_default();
        self.language = language;
        let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
        self.icons = self
            .info
            .icons
            .iter()
            .map(|icon| {
                let key = crate::icon_set::register(
                    &format!("plugin:{}/{}", self.id, icon.name),
                    icon.svg.as_bytes(),
                    icon.svg_dark.as_deref().map(str::as_bytes),
                );
                (icon.name.clone(), key)
            })
            .collect();
        let icon = |name: &str| -> &'static str {
            self.icons.get(name).copied().unwrap_or_else(|| leak(name))
        };
        let tool_windows: Vec<ToolWindow> = self
            .info
            .tool_windows
            .iter()
            .map(|t| ToolWindow {
                id: leak(&t.id),
                title: crate::i18n::Text(leak(&t.title)),
                icon: icon(&t.icon),
                dock: match t.dock {
                    protocol::Dock::Left => Dock::Left,
                    protocol::Dock::Bottom => Dock::Bottom,
                    protocol::Dock::Right => Dock::Right,
                },
                shortcut: "",
            })
            .collect();
        self.tool_windows = Box::leak(tool_windows.into_boxed_slice());
        self.commands = self
            .info
            .commands
            .iter()
            .map(|c| Command {
                id: leak(&c.id),
                label: c.label.clone(),
            })
            .collect();
    }

    fn file_type(&self, path: &Path) -> Option<&protocol::FileType> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        self.info
            .file_types
            .iter()
            .find(|t| t.extensions.contains(&extension))
    }

    /// The look of the IDE, kept for the plugin when it changes.
    fn refresh_style(&self, ctx: &egui::Context) {
        let theme = crate::ui::theme(ctx);
        let c = |c: Color32| c.to_srgba_unmultiplied();
        let style = protocol::Style {
            egui: serde_json::to_value(&*ctx.style()).unwrap_or_default(),
            theme: protocol::Theme {
                dark: theme.dark,
                frame: c(theme.frame),
                island: c(theme.island),
                popup: c(theme.popup),
                border: c(theme.border),
                separator: c(theme.separator),
                hover: c(theme.hover),
                pressed: c(theme.pressed),
                selection: c(theme.selection),
                selection_inactive: c(theme.selection_inactive),
                tool_open: c(theme.tool_open),
                accent: c(theme.accent),
                accent_hover: c(theme.accent_hover),
                text: c(theme.text),
                text_secondary: c(theme.text_secondary),
                text_disabled: c(theme.text_disabled),
                link: c(theme.link),
                text_selection: c(theme.text_selection),
                field: c(theme.field),
                added: c(theme.added),
                modified: c(theme.modified),
                deleted: c(theme.deleted),
                success: c(theme.success),
                warning: c(theme.warning),
                error: c(theme.error),
            },
        };
        let json = serde_json::to_vec(&style).unwrap_or_default();
        let mut kept = self.instance.shared.style.lock().unwrap();
        if kept.1 != json {
            *kept = (kept.0 + 1, json);
        }
    }

    /// Draws a surface of the plugin in what is left of `ui`.
    fn surface(&mut self, ui: &mut egui::Ui, kind: u32, key: &str) {
        let ctx = ui.ctx().clone();
        let content = ui.available_rect_before_wrap();
        ui.allocate_rect(content, Sense::hover());
        let id = ui.id().with(("native", self.id, kind, key));
        let response = ui.interact(content, id, Sense::click_and_drag());
        let (plugin_id, name, instance) = (self.id, self.name, self.instance.clone());
        let surface = self.surfaces.entry((kind, key.to_string())).or_default();
        if surface.failed {
            crate::ui::paint_centered(ui, content, &trf!("native_plugin_failed", name = name));
            return;
        }
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let over_floating = pointer.is_some_and(|p| {
            surface.floating.iter().any(|r| r.contains(p))
                && ctx
                    .layer_id_at(p)
                    .is_some_and(|l| l.order == Order::Foreground)
        });
        let hovered = response.hovered() || over_floating || surface.captured;
        if ctx.input(|i| i.pointer.any_pressed()) {
            if hovered {
                ctx.memory_mut(|m| m.request_focus(id));
            } else if ctx.memory(|m| m.has_focus(id)) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        }
        let focused = ctx.memory(|m| m.has_focus(id));
        if focused {
            ctx.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        // The input of the frame, as the plugin reads it.
        let (raw, modifiers, time, dt) = ctx.input(|i| {
            (
                i.raw.events.clone(),
                modifier_bits(i.modifiers),
                i.time,
                i.stable_dt,
            )
        });
        let mut texts: Vec<String> = Vec::new();
        let mut events: Vec<(abi::Event, Option<usize>, Option<usize>)> = Vec::new();
        let mut text_of = |text: &str| {
            texts.push(text.to_string());
            Some(texts.len() - 1)
        };
        if hovered
            && !surface.hovered
            && let Some(p) = pointer
        {
            events.push((pointer_event(abi::event::POINTER_MOVED, p), None, None));
        }
        let mut captured = surface.captured;
        for event in &raw {
            use abi::event::*;
            let item = match event {
                egui::Event::PointerMoved(p) if hovered => {
                    (pointer_event(POINTER_MOVED, *p), None, None)
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } if hovered || captured => {
                    captured = *pressed;
                    let mut e = pointer_event(POINTER_BUTTON, *pos);
                    e.button = match button {
                        egui::PointerButton::Primary => 0,
                        egui::PointerButton::Secondary => 1,
                        egui::PointerButton::Middle => 2,
                        egui::PointerButton::Extra1 => 3,
                        egui::PointerButton::Extra2 => 4,
                    };
                    e.pressed = u8::from(*pressed);
                    e.modifiers = modifier_bits(*modifiers);
                    (e, None, None)
                }
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                } if hovered => {
                    let mut e = pointer_event(WHEEL, pos2(delta.x, delta.y));
                    e.unit = match unit {
                        egui::MouseWheelUnit::Point => 0,
                        egui::MouseWheelUnit::Line => 1,
                        egui::MouseWheelUnit::Page => 2,
                    };
                    e.modifiers = modifier_bits(*modifiers);
                    (e, None, None)
                }
                egui::Event::Zoom(factor) if hovered => {
                    (pointer_event(ZOOM, pos2(*factor, 0.0)), None, None)
                }
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                } if focused => {
                    let e = abi::Event {
                        kind: KEY,
                        pressed: u8::from(*pressed),
                        repeat: u8::from(*repeat),
                        modifiers: modifier_bits(*modifiers),
                        ..Default::default()
                    };
                    (
                        e,
                        text_of(key.name()),
                        physical_key.and_then(|k| text_of(k.name())),
                    )
                }
                egui::Event::Text(text) if focused => (
                    abi::Event {
                        kind: TEXT,
                        ..Default::default()
                    },
                    text_of(text),
                    None,
                ),
                egui::Event::Paste(text) if focused => (
                    abi::Event {
                        kind: PASTE,
                        ..Default::default()
                    },
                    text_of(text),
                    None,
                ),
                egui::Event::Copy if focused => (
                    abi::Event {
                        kind: COPY,
                        ..Default::default()
                    },
                    None,
                    None,
                ),
                egui::Event::Cut if focused => (
                    abi::Event {
                        kind: CUT,
                        ..Default::default()
                    },
                    None,
                    None,
                ),
                egui::Event::WindowFocused(on) => (
                    abi::Event {
                        kind: FOCUS,
                        pressed: u8::from(*on),
                        ..Default::default()
                    },
                    None,
                    None,
                ),
                _ => continue,
            };
            events.push(item);
        }
        if (!hovered && surface.hovered) || raw.contains(&egui::Event::PointerGone) {
            events.push((
                abi::Event {
                    kind: abi::event::POINTER_GONE,
                    ..Default::default()
                },
                None,
                None,
            ));
        }
        surface.hovered = hovered;
        surface.captured = captured;
        let events: Vec<abi::Event> = events
            .into_iter()
            .map(|(mut e, text, physical)| {
                if let Some(i) = text {
                    e.text = abi::Str::new(&texts[i]);
                }
                if let Some(i) = physical {
                    e.physical = abi::Str::new(&texts[i]);
                }
                e
            })
            .collect();
        let language = crate::i18n::current();
        let style_generation = instance.shared.style.lock().unwrap().0;
        let input = abi::FrameInput {
            size: std::mem::size_of::<abi::FrameInput>(),
            surface: kind,
            key: abi::Str::new(key),
            screen: abi_rect(ctx.content_rect()),
            content: abi_rect(content),
            pixels_per_point: ctx.pixels_per_point(),
            time,
            dt,
            focused: u8::from(focused),
            modifiers,
            events: events.as_ptr(),
            events_len: events.len(),
            language: abi::Str::new(language),
            style_generation,
            fonts_generation: crate::ui::installed_fonts().0,
        };
        let painted = {
            let plugin = instance.plugin.lock().unwrap();
            // SAFETY: the plugin is alive; `input` and what it points to live for the call.
            let output = unsafe { (instance.api.frame)(*plugin, &input) };
            // SAFETY: the output of the plugin stays valid until the next call, made after
            // this one under the same lock.
            (!output.is_null()).then(|| unsafe { read_output(&*output) })
        };
        let Some(output) = painted else {
            surface.failed = true;
            ctx.request_repaint();
            return;
        };
        // Textures first: the meshes use them.
        let textures = ctx.tex_manager();
        let mut textures = textures.write();
        for update in output.textures {
            let name = format!("plugin:{plugin_id}/{key}/{}", update.id);
            match surface.textures.get(&update.id) {
                Some(id) => {
                    let delta = match update.pos {
                        None => ImageDelta::full(update.image, update.options),
                        Some(pos) => ImageDelta::partial(pos, update.image, update.options),
                    };
                    textures.set(*id, delta);
                }
                None if update.pos.is_none() => {
                    let id = textures.alloc(name, update.image, update.options);
                    surface.textures.insert(update.id, id);
                }
                None => {}
            }
        }
        for freed in output.freed {
            if let Some(id) = surface.textures.remove(&freed) {
                textures.free(id);
            }
        }
        drop(textures);
        let texture = |id: u64| surface.textures.get(&id).copied();
        let painter = ui.painter_at(content);
        for primitive in &output.base {
            paint(&painter, primitive, content, &texture, &instance);
        }
        let float_layer = LayerId::new(Order::Foreground, id.with("floating"));
        let floating = ctx.layer_painter(float_layer);
        let screen = ctx.content_rect();
        for primitive in &output.floating {
            paint(&floating, primitive, screen, &texture, &instance);
        }
        // Where the windows and menus of the plugin are, the IDE under them gets no pointer.
        for (i, rect) in output.floating_rects.iter().enumerate() {
            egui::Area::new(id.with(("block", i)))
                .order(Order::Foreground)
                .fixed_pos(rect.min)
                .constrain(false)
                .show(&ctx, |ui| {
                    ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
                });
        }
        surface.floating = output.floating_rects;
        if (hovered || output.wants_pointer)
            && let Some(cursor) = egui::CursorIcon::ALL
                .iter()
                .find(|c| format!("{c:?}") == output.cursor)
        {
            ctx.set_cursor_icon(*cursor);
        }
        if !output.copied_text.is_empty() {
            ctx.copy_text(output.copied_text);
        }
        if !output.open_url.is_empty() {
            ctx.open_url(egui::OpenUrl::new_tab(output.open_url));
        }
        match output.repaint_after_ms {
            0 => ctx.request_repaint(),
            u64::MAX => {}
            ms => ctx.request_repaint_after(std::time::Duration::from_millis(ms)),
        }
    }

    /// Lets go of the textures of a surface.
    fn forget(&mut self, kind: u32, key: &str) {
        if let (Some(surface), Some(ctx)) =
            (self.surfaces.remove(&(kind, key.to_string())), &self.ctx)
        {
            let textures = ctx.tex_manager();
            let mut textures = textures.write();
            for id in surface.textures.into_values() {
                textures.free(id);
            }
        }
    }
}

impl Drop for NativePlugin {
    fn drop(&mut self) {
        let keys: Vec<_> = self.surfaces.keys().cloned().collect();
        for (kind, key) in keys {
            self.forget(kind, &key);
        }
    }
}

fn modifier_bits(m: egui::Modifiers) -> u32 {
    use abi::modifier::*;
    [
        (m.alt, ALT),
        (m.ctrl, CTRL),
        (m.shift, SHIFT),
        (m.mac_cmd, MAC_CMD),
        (m.command, COMMAND),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |bits, (_, bit)| bits | bit)
}

fn pointer_event(kind: u32, p: Pos2) -> abi::Event {
    abi::Event {
        kind,
        x: p.x,
        y: p.y,
        ..Default::default()
    }
}

fn abi_rect(r: Rect) -> abi::Rect {
    abi::Rect {
        min_x: r.min.x,
        min_y: r.min.y,
        max_x: r.max.x,
        max_y: r.max.y,
    }
}

fn rect(r: abi::Rect) -> Rect {
    Rect::from_min_max(pos2(r.min_x, r.min_y), pos2(r.max_x, r.max_y))
}

struct TextureUpdate {
    id: u64,
    pos: Option<[usize; 2]>,
    image: ImageData,
    options: TextureOptions,
}

/// A primitive of the plugin, copied out of its memory.
enum Painted {
    Mesh {
        clip: Rect,
        texture: u64,
        mesh: egui::Mesh,
    },
    Callback {
        clip: Rect,
        rect: Rect,
        id: u64,
    },
}

struct Output {
    base: Vec<Painted>,
    floating: Vec<Painted>,
    floating_rects: Vec<Rect>,
    textures: Vec<TextureUpdate>,
    freed: Vec<u64>,
    cursor: String,
    wants_pointer: bool,
    repaint_after_ms: u64,
    copied_text: String,
    open_url: String,
}

/// # Safety
/// The pointers of `slice` are valid, or null with a length of 0.
unsafe fn slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller's.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// Copies what the plugin gave.
///
/// # Safety
/// `output` and its pointers are valid.
unsafe fn read_output(output: &abi::FrameOutput) -> Output {
    // SAFETY: the caller's.
    let primitives = |ptr, len| -> Vec<Painted> {
        unsafe { slice::<abi::Primitive>(ptr, len) }
            .iter()
            .filter_map(|p| match p.kind {
                abi::primitive::MESH => {
                    // SAFETY: the caller's.
                    let (vertices, indices) = unsafe {
                        (
                            slice(p.vertices, p.vertices_len),
                            slice(p.indices, p.indices_len),
                        )
                    };
                    let vertices = vertices
                        .iter()
                        .map(|v| egui::epaint::Vertex {
                            pos: pos2(v.x, v.y),
                            uv: pos2(v.u, v.v),
                            color: Color32::from_rgba_premultiplied(
                                v.color[0], v.color[1], v.color[2], v.color[3],
                            ),
                        })
                        .collect::<Vec<_>>();
                    let len = vertices.len() as u32;
                    // A mesh whose indices point out of it is dropped rather than painted.
                    indices.iter().all(|&i| i < len).then(|| Painted::Mesh {
                        clip: rect(p.clip),
                        texture: p.texture,
                        mesh: egui::Mesh {
                            indices: indices.to_vec(),
                            vertices,
                            texture_id: TextureId::default(),
                        },
                    })
                }
                abi::primitive::CALLBACK => Some(Painted::Callback {
                    clip: rect(p.clip),
                    rect: rect(p.rect),
                    id: p.texture,
                }),
                _ => None,
            })
            .collect()
    };
    // SAFETY: the caller's.
    unsafe {
        Output {
            base: primitives(output.base, output.base_len),
            floating: primitives(output.floating, output.floating_len),
            floating_rects: slice(output.floating_rects, output.floating_rects_len)
                .iter()
                .map(|r| rect(*r))
                .collect(),
            textures: slice(output.textures, output.textures_len)
                .iter()
                .filter_map(|t| {
                    let (width, height) = (t.width as usize, t.height as usize);
                    let pixels = slice(t.pixels, width.checked_mul(height)?.checked_mul(4)?);
                    if pixels.len() != width * height * 4 {
                        return None;
                    }
                    let filter = |f: u8| match f {
                        1 => egui::TextureFilter::Nearest,
                        _ => egui::TextureFilter::Linear,
                    };
                    let image = ColorImage::new(
                        [width, height],
                        pixels
                            .chunks_exact(4)
                            .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
                            .collect(),
                    );
                    Some(TextureUpdate {
                        id: t.id,
                        pos: (t.whole == 0).then_some([t.x as usize, t.y as usize]),
                        image: ImageData::Color(Arc::new(image)),
                        options: TextureOptions {
                            magnification: filter(t.magnification),
                            minification: filter(t.minification),
                            wrap_mode: match t.wrap {
                                1 => egui::TextureWrapMode::Repeat,
                                2 => egui::TextureWrapMode::MirroredRepeat,
                                _ => egui::TextureWrapMode::ClampToEdge,
                            },
                            mipmap_mode: match t.mipmap {
                                1 => Some(egui::TextureFilter::Linear),
                                2 => Some(egui::TextureFilter::Nearest),
                                _ => None,
                            },
                        },
                    })
                })
                .collect(),
            freed: slice(output.freed, output.freed_len).to_vec(),
            cursor: output.cursor.as_str().to_string(),
            wants_pointer: output.wants_pointer != 0,
            repaint_after_ms: output.repaint_after_ms,
            copied_text: output.copied_text.as_str().to_string(),
            open_url: output.open_url.as_str().to_string(),
        }
    }
}

/// Paints a primitive of the plugin, clipped to `within`.
fn paint(
    painter: &egui::Painter,
    primitive: &Painted,
    within: Rect,
    texture: &dyn Fn(u64) -> Option<TextureId>,
    instance: &Arc<Instance>,
) {
    match primitive {
        Painted::Mesh {
            clip,
            texture: id,
            mesh,
        } => {
            let Some(texture) = texture(*id) else {
                return;
            };
            let mut mesh = mesh.clone();
            mesh.texture_id = texture;
            painter
                .with_clip_rect(clip.intersect(within))
                .add(Shape::mesh(mesh));
        }
        Painted::Callback { clip, rect, id } => {
            let (instance, id) = (instance.clone(), *id);
            let callback = eframe::egui_glow::CallbackFn::new(move |info, _painter| {
                let info = abi::PaintInfo {
                    viewport: abi_rect(info.viewport),
                    clip: abi_rect(info.clip_rect),
                    pixels_per_point: info.pixels_per_point,
                    screen_width: info.screen_size_px[0],
                    screen_height: info.screen_size_px[1],
                    get_proc_address: super::gl::get_proc_address,
                };
                instance.paint(id, &info);
            });
            painter
                .with_clip_rect(clip.intersect(within))
                .add(egui::PaintCallback {
                    rect: *rect,
                    callback: Arc::new(callback),
                });
        }
    }
}

impl BuiltinPlugin for NativePlugin {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn version(&self) -> &'static str {
        self.version
    }
    fn external(&self) -> bool {
        true
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        self.tool_windows
    }
    fn commands(&self) -> Vec<Command> {
        self.commands
            .iter()
            .map(|c| Command {
                id: c.id,
                label: c.label.clone(),
            })
            .collect()
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
            *self.instance.shared.ctx.lock().unwrap() = Some(ctx.clone());
        }
        if self.language != crate::i18n::current() {
            self.refresh_info();
        }
        self.refresh_style(ctx);
        {
            let mut project = self.instance.shared.project.lock().unwrap();
            let root = host.root().to_path_buf();
            project.root = (!root.as_os_str().is_empty()).then_some(root);
            project.active_file = host.active_file();
            project.data_dir = crate::cli::data_dir();
        }
        let requests = std::mem::take(&mut *self.instance.shared.requests.lock().unwrap());
        for request in requests {
            match request {
                Request::Notify(message, false) => host.notify(message),
                Request::Notify(message, true) => host.balloon(self.name, message, true),
                Request::OpenFile(path) => host.open_file(path),
                Request::ShowToolWindow(id) => {
                    if let Some(tool) = self.tool_windows.iter().find(|t| t.id == id) {
                        host.show_tool_window(tool);
                    }
                }
            }
        }
    }

    fn run_command(&mut self, id: &str, _host: &mut dyn Host) {
        self.instance.call_json(
            protocol::RUN_COMMAND,
            &protocol::RunCommand { id: id.into() },
        );
    }

    fn tool_window(&mut self, id: &str, ui: &mut egui::Ui, _host: &mut dyn Host) {
        if self.tool_windows.iter().any(|t| t.id == id) {
            self.surface(ui, abi::surface::TOOL_WINDOW, id);
        }
    }

    fn opens_binary(&self, path: &Path) -> bool {
        self.file_type(path).is_some_and(|t| t.binary)
    }
    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        self.file_type(path).map(|t| match t.layout {
            protocol::Layout::Text => EditorLayout::Text,
            protocol::Layout::Split => EditorLayout::Split,
            protocol::Layout::Preview => EditorLayout::Preview,
        })
    }
    fn layout_switch(&self, path: &Path) -> bool {
        self.file_type(path).is_some_and(|t| t.layout_switch)
    }
    fn file_icon(&self, path: &Path) -> Option<&'static str> {
        let name = self.file_type(path)?.icon.as_ref()?;
        self.icons.get(name).copied()
    }
    fn file_closed(&mut self, _ctx: &egui::Context, path: &Path) {
        let key = path.to_string_lossy().into_owned();
        self.forget(abi::surface::PREVIEW, &key);
        self.instance.call_json(
            protocol::FILE_CLOSED,
            &protocol::PathMessage {
                path: path.to_path_buf(),
            },
        );
    }
    fn preview(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        _text: &mut String,
        _scroll: Option<f32>,
    ) {
        let key = path.to_string_lossy().into_owned();
        self.surface(ui, abi::surface::PREVIEW, &key);
    }
}

#[cfg(test)]
#[path = "../../test/plugins/native/tests.rs"]
mod tests;
