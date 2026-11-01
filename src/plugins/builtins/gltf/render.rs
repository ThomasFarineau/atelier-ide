//! The GPU side of the model viewer: an OpenGL paint callback of egui_glow.
//!
//! The model is drawn into a framebuffer of its own, with a depth buffer and multisampling, then
//! copied into the window where egui laid out the viewer: the window needs neither a depth
//! buffer nor multisampling for it. The buffers and textures of a model are sent to the GPU the
//! first time it is drawn, and freed at the next frame once its tab is closed.

use super::scene::{Alpha, Draw, Scene, Wrap};
use eframe::{egui, egui_glow, glow, glow::HasContext};
use glam::{Mat4, Vec3};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// What a frame of a viewer draws.
pub struct Frame {
    pub model: u64,
    pub scene: Arc<Scene>,
    /// Changes whenever the pose does: deformed vertices are sent again.
    pub generation: u64,
    pub draws: Arc<Vec<Draw>>,
    pub view: Mat4,
    pub projection: Mat4,
    pub eye: Vec3,
    /// Direction toward the key light.
    pub light: Vec3,
    /// Background, sRGB 0 to 1.
    pub background: [f32; 3],
    /// Lines of the ground grid, as vertices of [`STRIDE`] floats.
    pub grid: Arc<Vec<f32>>,
}

/// Floats a vertex: position, normal, texture coordinates, color.
const STRIDE: usize = 12;

const VERTEX: &str = r#"
in vec3 a_position;
in vec3 a_normal;
in vec2 a_uv;
in vec4 a_color;
uniform mat4 u_view_projection;
uniform mat4 u_model;
uniform mat4 u_normal;
out vec3 v_world;
out vec3 v_normal;
out vec2 v_uv;
out vec4 v_color;
void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    v_world = world.xyz;
    v_normal = mat3(u_normal) * a_normal;
    v_uv = a_uv;
    v_color = a_color;
    gl_Position = u_view_projection * world;
}
"#;

const FRAGMENT: &str = r#"
precision mediump float;
in vec3 v_world;
in vec3 v_normal;
in vec2 v_uv;
in vec4 v_color;
uniform vec4 u_base;
uniform int u_textured;
uniform sampler2D u_texture;
uniform float u_cutoff;
uniform vec3 u_emissive;
uniform vec3 u_eye;
uniform vec3 u_light;
uniform int u_unlit;
out vec4 out_color;
void main() {
    if (u_unlit == 1) {
        out_color = v_color;
        return;
    }
    vec4 base = u_base * v_color;
    if (u_textured == 1) {
        vec4 t = texture(u_texture, v_uv);
        base *= vec4(pow(t.rgb, vec3(2.2)), t.a);
    }
    if (base.a < u_cutoff) {
        discard;
    }
    vec3 n = length(v_normal) > 0.0001 ? normalize(v_normal) : vec3(0.0, 1.0, 0.0);
    vec3 v = normalize(u_eye - v_world);
    // Both faces are lit as the one in front.
    if (dot(n, v) < 0.0) {
        n = -n;
    }
    float key = max(dot(n, u_light), 0.0);
    float fill = max(dot(n, normalize(vec3(-u_light.x, 0.3, -u_light.z))), 0.0) * 0.25;
    vec3 sky = mix(vec3(0.16, 0.15, 0.14), vec3(0.32, 0.34, 0.38), n.y * 0.5 + 0.5);
    float specular = pow(max(dot(n, normalize(u_light + v)), 0.0), 48.0) * 0.18;
    vec3 color = base.rgb * (sky + key * 0.9 + fill) + specular + u_emissive;
    out_color = vec4(pow(clamp(color, 0.0, 1.0), vec3(1.0 / 2.2)), base.a);
}
"#;

struct Program {
    program: glow::Program,
    uniforms: HashMap<&'static str, glow::UniformLocation>,
}

const UNIFORMS: [&str; 11] = [
    "u_view_projection",
    "u_model",
    "u_normal",
    "u_base",
    "u_textured",
    "u_texture",
    "u_cutoff",
    "u_emissive",
    "u_eye",
    "u_light",
    "u_unlit",
];

struct Primitive {
    vao: glow::VertexArray,
    dynamic: glow::Buffer,
    fixed: glow::Buffer,
    indices: glow::Buffer,
    count: i32,
    /// Generation and node of the deformed vertices in `dynamic`.
    uploaded: Option<(u64, usize)>,
}

struct Model {
    primitives: Vec<Vec<Primitive>>,
    textures: Vec<glow::Texture>,
    grid: Option<(glow::VertexArray, glow::Buffer, i32, usize)>,
}

struct Target {
    size: [i32; 2],
    samples: i32,
    msaa: glow::Framebuffer,
    msaa_color: glow::Renderbuffer,
    msaa_depth: glow::Renderbuffer,
    resolve: glow::Framebuffer,
    resolve_color: glow::Renderbuffer,
}

#[derive(Default)]
pub struct Gpu {
    program: Option<Result<Program, String>>,
    models: HashMap<u64, Model>,
    /// Models whose tab closed, freed at the next paint.
    garbage: Vec<u64>,
    target: Option<Target>,
}

impl Gpu {
    /// The shaders could not be built: the viewer says so.
    pub fn error(&self) -> Option<&str> {
        match &self.program {
            Some(Err(e)) => Some(e),
            _ => None,
        }
    }

    pub fn forget(&mut self, model: u64) {
        self.garbage.push(model);
    }
}

/// The paint callback that draws `frame` in `rect`.
pub fn callback(gpu: Arc<Mutex<Gpu>>, rect: egui::Rect, frame: Frame) -> egui::PaintCallback {
    egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
            let mut gpu = gpu.lock().unwrap();
            // SAFETY: called by egui_glow on its thread, with its context current.
            unsafe { gpu.paint(painter.gl(), &info, &frame) };
        })),
    }
}

fn as_bytes(floats: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding and any byte is a valid u8.
    unsafe { std::slice::from_raw_parts(floats.as_ptr().cast(), std::mem::size_of_val(floats)) }
}

fn index_bytes(indices: &[u32]) -> &[u8] {
    // SAFETY: as for `as_bytes`.
    unsafe { std::slice::from_raw_parts(indices.as_ptr().cast(), std::mem::size_of_val(indices)) }
}

unsafe fn compile(gl: &glow::Context) -> Result<Program, String> {
    unsafe {
        let version = egui_glow::ShaderVersion::get(gl);
        if !version.is_new_shader_interface() {
            return Err(tr!("gltf.opengl_too_old").into());
        }
        let program = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, source) in [
            (glow::VERTEX_SHADER, VERTEX),
            (glow::FRAGMENT_SHADER, FRAGMENT),
        ] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(
                shader,
                &format!("{}\n{source}", version.version_declaration()),
            );
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(gl.get_shader_info_log(shader));
            }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        for (i, name) in ["a_position", "a_normal", "a_uv", "a_color"]
            .into_iter()
            .enumerate()
        {
            gl.bind_attrib_location(program, i as u32, name);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        let uniforms = UNIFORMS
            .into_iter()
            .filter_map(|name| Some((name, gl.get_uniform_location(program, name)?)))
            .collect();
        Ok(Program { program, uniforms })
    }
}

/// A vertex array of the four attributes: positions and normals in `dynamic`, texture
/// coordinates and colors in `fixed` (or everything in `dynamic` when `fixed` is None).
unsafe fn attributes(gl: &glow::Context, dynamic: glow::Buffer, fixed: Option<glow::Buffer>) {
    unsafe {
        let f = 4;
        let (stride_a, stride_b) = match fixed {
            Some(_) => (6 * f, 6 * f),
            None => (STRIDE as i32 * f, STRIDE as i32 * f),
        };
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(dynamic));
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride_a, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride_a, 3 * f);
        let base = match fixed {
            Some(buffer) => {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                0
            }
            None => 6 * f,
        };
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride_b, base);
        gl.enable_vertex_attrib_array(3);
        gl.vertex_attrib_pointer_f32(3, 4, glow::FLOAT, false, stride_b, base + 2 * f);
    }
}

unsafe fn upload(gl: &glow::Context, scene: &Scene) -> Result<Model, String> {
    unsafe {
        let mut textures = Vec::new();
        for texture in &scene.textures {
            let id = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(id));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                texture.width as i32,
                texture.height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&texture.rgba)),
            );
            gl.generate_mipmap(glow::TEXTURE_2D);
            let wrap = |w: Wrap| match w {
                Wrap::Repeat => glow::REPEAT,
                Wrap::Clamp => glow::CLAMP_TO_EDGE,
                Wrap::Mirror => glow::MIRRORED_REPEAT,
            } as i32;
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                wrap(texture.wrap[0]),
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                wrap(texture.wrap[1]),
            );
            let (min, mag) = match texture.nearest {
                true => (glow::NEAREST_MIPMAP_LINEAR, glow::NEAREST),
                false => (glow::LINEAR_MIPMAP_LINEAR, glow::LINEAR),
            };
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, min as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, mag as i32);
            textures.push(id);
        }
        gl.bind_texture(glow::TEXTURE_2D, None);

        let mut primitives = Vec::new();
        for mesh in &scene.meshes {
            let mut list = Vec::new();
            for p in &mesh.primitives {
                let mut dynamic_data = Vec::with_capacity(p.positions.len() * 6);
                let mut fixed_data = Vec::with_capacity(p.positions.len() * 6);
                for i in 0..p.positions.len() {
                    dynamic_data.extend_from_slice(&p.positions[i].to_array());
                    dynamic_data.extend_from_slice(&p.normals[i].to_array());
                    fixed_data.extend_from_slice(&p.uvs[i]);
                    fixed_data.extend_from_slice(&p.colors[i]);
                }
                let vao = gl.create_vertex_array()?;
                gl.bind_vertex_array(Some(vao));
                let dynamic = gl.create_buffer()?;
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(dynamic));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    as_bytes(&dynamic_data),
                    glow::DYNAMIC_DRAW,
                );
                let fixed = gl.create_buffer()?;
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(fixed));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    as_bytes(&fixed_data),
                    glow::STATIC_DRAW,
                );
                attributes(gl, dynamic, Some(fixed));
                let indices = gl.create_buffer()?;
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(indices));
                gl.buffer_data_u8_slice(
                    glow::ELEMENT_ARRAY_BUFFER,
                    index_bytes(&p.indices),
                    glow::STATIC_DRAW,
                );
                gl.bind_vertex_array(None);
                list.push(Primitive {
                    vao,
                    dynamic,
                    fixed,
                    indices,
                    count: p.indices.len() as i32,
                    uploaded: None,
                });
            }
            primitives.push(list);
        }
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        Ok(Model {
            primitives,
            textures,
            grid: None,
        })
    }
}

unsafe fn free(gl: &glow::Context, model: Model) {
    unsafe {
        for p in model.primitives.into_iter().flatten() {
            gl.delete_vertex_array(p.vao);
            gl.delete_buffer(p.dynamic);
            gl.delete_buffer(p.fixed);
            gl.delete_buffer(p.indices);
        }
        for t in model.textures {
            gl.delete_texture(t);
        }
        if let Some((vao, buffer, _, _)) = model.grid {
            gl.delete_vertex_array(vao);
            gl.delete_buffer(buffer);
        }
    }
}

unsafe fn free_target(gl: &glow::Context, t: Target) {
    unsafe {
        gl.delete_framebuffer(t.msaa);
        gl.delete_framebuffer(t.resolve);
        gl.delete_renderbuffer(t.msaa_color);
        gl.delete_renderbuffer(t.msaa_depth);
        gl.delete_renderbuffer(t.resolve_color);
    }
}

unsafe fn target(gl: &glow::Context, size: [i32; 2]) -> Result<Target, String> {
    unsafe {
        let samples = gl.get_parameter_i32(glow::MAX_SAMPLES).clamp(0, 4);
        let storage = |samples: i32, format: u32| -> Result<glow::Renderbuffer, String> {
            let rb = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rb));
            gl.renderbuffer_storage_multisample(
                glow::RENDERBUFFER,
                samples,
                format,
                size[0],
                size[1],
            );
            Ok(rb)
        };
        let msaa_color = storage(samples, glow::RGBA8)?;
        let msaa_depth = storage(samples, glow::DEPTH_COMPONENT24)?;
        let resolve_color = storage(0, glow::RGBA8)?;
        gl.bind_renderbuffer(glow::RENDERBUFFER, None);
        let msaa = gl.create_framebuffer()?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(msaa));
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::RENDERBUFFER,
            Some(msaa_color),
        );
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::DEPTH_ATTACHMENT,
            glow::RENDERBUFFER,
            Some(msaa_depth),
        );
        let complete = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        let resolve = gl.create_framebuffer()?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(resolve));
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::RENDERBUFFER,
            Some(resolve_color),
        );
        let target = Target {
            size,
            samples,
            msaa,
            msaa_color,
            msaa_depth,
            resolve,
            resolve_color,
        };
        if complete != glow::FRAMEBUFFER_COMPLETE
            || gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
        {
            free_target(gl, target);
            return Err(tr!("gltf.no_framebuffer").into());
        }
        Ok(target)
    }
}

impl Gpu {
    unsafe fn paint(&mut self, gl: &glow::Context, info: &egui::PaintCallbackInfo, frame: &Frame) {
        unsafe {
            // The framebuffer egui draws the window into, read before anything binds another:
            // creating the offscreen targets binds theirs, and egui would then go on drawing the
            // rest of the window, and every later frame, into it, out of sight.
            let previous = gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING);
            for id in std::mem::take(&mut self.garbage) {
                if let Some(model) = self.models.remove(&id) {
                    free(gl, model);
                }
            }
            if self.program.is_none() {
                self.program = Some(compile(gl));
            }
            let Some(Ok(program)) = &self.program else {
                return;
            };
            let viewport = info.viewport_in_pixels();
            let size = [viewport.width_px.max(1), viewport.height_px.max(1)];
            if self.target.as_ref().is_none_or(|t| t.size != size) {
                if let Some(old) = self.target.take() {
                    free_target(gl, old);
                }
                let created = target(gl, size);
                gl.bind_framebuffer(glow::FRAMEBUFFER, previous);
                match created {
                    Ok(t) => self.target = Some(t),
                    Err(e) => {
                        self.program = Some(Err(e));
                        return;
                    }
                }
            }
            if let std::collections::hash_map::Entry::Vacant(e) = self.models.entry(frame.model) {
                match upload(gl, &frame.scene) {
                    Ok(model) => {
                        e.insert(model);
                    }
                    Err(e) => {
                        self.program = Some(Err(e));
                        return;
                    }
                }
            }
            let target = self.target.as_ref().unwrap();
            let model = self.models.get_mut(&frame.model).unwrap();

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.msaa));
            gl.viewport(0, 0, size[0], size[1]);
            gl.disable(glow::SCISSOR_TEST);
            let [r, g, b] = frame.background;
            gl.clear_color(r, g, b, 1.0);
            gl.depth_mask(true);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::BLEND);
            if target.samples > 0 {
                gl.enable(glow::MULTISAMPLE);
            }

            gl.use_program(Some(program.program));
            let u = |name: &str| program.uniforms.get(name);
            let view_projection = frame.projection * frame.view;
            gl.uniform_matrix_4_f32_slice(
                u("u_view_projection"),
                false,
                &view_projection.to_cols_array(),
            );
            gl.uniform_3_f32_slice(u("u_eye"), &frame.eye.to_array());
            gl.uniform_3_f32_slice(u("u_light"), &frame.light.normalize().to_array());
            gl.uniform_1_i32(u("u_texture"), 0);
            gl.active_texture(glow::TEXTURE0);

            // Opaque first, then what blends, from the farthest.
            let scene = &frame.scene;
            let mut blended: Vec<(f32, &Draw)> = Vec::new();
            let draw_one = |draw: &Draw, model: &mut Model| {
                let primitive = &scene.meshes[draw.mesh].primitives[draw.primitive];
                let Some(gpu) = model
                    .primitives
                    .get_mut(draw.mesh)
                    .and_then(|m| m.get_mut(draw.primitive))
                else {
                    return;
                };
                if let Some(data) = &draw.deformed
                    && gpu.uploaded != Some((frame.generation, draw.node))
                {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(gpu.dynamic));
                    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, as_bytes(data));
                    gpu.uploaded = Some((frame.generation, draw.node));
                }
                let material = &scene.materials[primitive.material];
                gl.uniform_matrix_4_f32_slice(u("u_model"), false, &draw.model.to_cols_array());
                let normal = draw.model.inverse().transpose();
                gl.uniform_matrix_4_f32_slice(u("u_normal"), false, &normal.to_cols_array());
                gl.uniform_4_f32_slice(u("u_base"), &material.base_color);
                gl.uniform_3_f32_slice(u("u_emissive"), &material.emissive);
                gl.uniform_1_f32(
                    u("u_cutoff"),
                    match material.alpha {
                        Alpha::Mask(cutoff) => cutoff,
                        _ => -1.0,
                    },
                );
                let texture = material.texture.and_then(|t| model.textures.get(t));
                gl.uniform_1_i32(u("u_textured"), texture.is_some() as i32);
                gl.bind_texture(glow::TEXTURE_2D, texture.copied());
                gl.bind_vertex_array(Some(gpu.vao));
                gl.draw_elements(glow::TRIANGLES, gpu.count, glow::UNSIGNED_INT, 0);
            };
            gl.uniform_1_i32(u("u_unlit"), 0);
            for draw in frame.draws.iter() {
                let primitive = &scene.meshes[draw.mesh].primitives[draw.primitive];
                if scene.materials[primitive.material].alpha == Alpha::Blend {
                    let center = draw.model.transform_point3(primitive.center);
                    blended.push((frame.view.transform_point3(center).z, draw));
                } else {
                    draw_one(draw, model);
                }
            }

            // The grid, then the transparent parts, over the opaque ones.
            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ZERO,
                glow::ONE,
            );
            gl.depth_mask(false);
            let grid_key = Arc::as_ptr(&frame.grid) as usize;
            if model.grid.as_ref().is_none_or(|g| g.3 != grid_key) {
                if let Some((vao, buffer, _, _)) = model.grid.take() {
                    gl.delete_vertex_array(vao);
                    gl.delete_buffer(buffer);
                }
                if let (Ok(vao), Ok(buffer)) = (gl.create_vertex_array(), gl.create_buffer()) {
                    gl.bind_vertex_array(Some(vao));
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                    gl.buffer_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        as_bytes(&frame.grid),
                        glow::STATIC_DRAW,
                    );
                    attributes(gl, buffer, None);
                    gl.bind_vertex_array(None);
                    model.grid = Some((vao, buffer, (frame.grid.len() / STRIDE) as i32, grid_key));
                }
            }
            if let Some((vao, _, count, _)) = model.grid {
                gl.uniform_1_i32(u("u_unlit"), 1);
                gl.uniform_matrix_4_f32_slice(u("u_model"), false, &Mat4::IDENTITY.to_cols_array());
                gl.bind_vertex_array(Some(vao));
                gl.draw_arrays(glow::LINES, 0, count);
                gl.uniform_1_i32(u("u_unlit"), 0);
            }
            blended.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, draw) in blended {
                draw_one(draw, model);
            }

            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.depth_mask(true);
            gl.disable(glow::DEPTH_TEST);

            // Multisampled → plain framebuffer of the same format → the window.
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.msaa));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(target.resolve));
            gl.blit_framebuffer(
                0,
                0,
                size[0],
                size[1],
                0,
                0,
                size[0],
                size[1],
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.resolve));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, previous);
            // The scissor egui set keeps the copy inside the clip rectangle.
            gl.enable(glow::SCISSOR_TEST);
            gl.blit_framebuffer(
                0,
                0,
                size[0],
                size[1],
                viewport.left_px,
                viewport.from_bottom_px,
                viewport.left_px + size[0],
                viewport.from_bottom_px + size[1],
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, previous);
        }
    }
}

/// Lines of a ground grid under a box, faded toward its edges.
pub fn grid(min: Vec3, max: Vec3, color: [f32; 3]) -> Vec<f32> {
    let extent = (max - min).max_element().max(1e-3);
    let step = 10f32.powf((extent / 4.0).log10().floor());
    let half = ((extent * 1.5 / step).ceil() * step).max(step);
    let center = ((min + max) / 2.0 / step).round() * step;
    let y = min.y;
    let lines = (2.0 * half / step).round() as i32;
    let mut out = Vec::with_capacity((lines as usize + 1) * 4 * STRIDE);
    let mut vertex = |x: f32, z: f32, alpha: f32| {
        out.extend_from_slice(&[x, y, z, 0.0, 1.0, 0.0, 0.0, 0.0]);
        out.extend_from_slice(&[color[0], color[1], color[2], alpha]);
    };
    for i in 0..=lines {
        let offset = -half + i as f32 * step;
        let alpha = 0.45 * (1.0 - (offset / half).abs()).powf(0.6);
        let (x, z) = (center.x + offset, center.z + offset);
        vertex(x, center.z - half, 0.0);
        vertex(x, center.z, alpha);
        vertex(x, center.z, alpha);
        vertex(x, center.z + half, 0.0);
        vertex(center.x - half, z, 0.0);
        vertex(center.x, z, alpha);
        vertex(center.x, z, alpha);
        vertex(center.x + half, z, 0.0);
    }
    out
}
