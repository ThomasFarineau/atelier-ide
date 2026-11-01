//! The CPU side of a glTF model: what the file holds (meshes, materials, textures, node tree,
//! skins and animations), and the pose of the scene at a time of an animation.
//!
//! Skinned and morphed meshes are deformed here, on the CPU: the GPU only gets the vertices to
//! draw (see `render.rs`).

use glam::{Mat4, Quat, Vec3};
use std::path::Path;

pub struct Target {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
}

pub struct Primitive {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Empty when the primitive is not skinned.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub targets: Vec<Target>,
    /// Index in [`Scene::materials`]; the last material is the default one.
    pub material: usize,
    /// Middle of the bounding box, to sort transparent primitives.
    pub center: Vec3,
}

pub struct Mesh {
    pub primitives: Vec<Primitive>,
    /// Default weights of the morph targets.
    pub weights: Vec<f32>,
}

impl Mesh {
    fn morphs(&self) -> bool {
        self.primitives.iter().any(|p| !p.targets.is_empty())
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Alpha {
    Opaque,
    /// Pixels under the cutoff are not drawn.
    Mask(f32),
    Blend,
}

pub struct Material {
    /// Linear RGBA.
    pub base_color: [f32; 4],
    /// Index in [`Scene::textures`].
    pub texture: Option<usize>,
    pub alpha: Alpha,
    pub emissive: [f32; 3],
}

impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: [0.8, 0.8, 0.8, 1.0],
            texture: None,
            alpha: Alpha::Opaque,
            emissive: [0.0; 3],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrap {
    Repeat,
    Clamp,
    Mirror,
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// sRGB, 8 bits per channel.
    pub rgba: Vec<u8>,
    pub wrap: [Wrap; 2],
    pub nearest: bool,
}

pub struct Node {
    pub children: Vec<usize>,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
    pub weights: Option<Vec<f32>>,
}

pub struct Skin {
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Property {
    Translation,
    Rotation,
    Scale,
    Weights,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interpolation {
    Linear,
    Step,
    CubicSpline,
}

pub struct Channel {
    pub node: usize,
    pub property: Property,
    pub interpolation: Interpolation,
    /// Seconds, increasing.
    pub times: Vec<f32>,
    /// `width` values per key, three times that for a cubic spline (in tangent, value, out
    /// tangent).
    pub values: Vec<f32>,
    pub width: usize,
}

pub struct Animation {
    pub name: String,
    pub channels: Vec<Channel>,
    /// Seconds: the last key of its channels.
    pub duration: f32,
}

pub struct Scene {
    pub nodes: Vec<Node>,
    /// Every node of the scene shown, each after its parent.
    pub order: Vec<usize>,
    pub parents: Vec<Option<usize>>,
    pub meshes: Vec<Mesh>,
    pub skins: Vec<Skin>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub animations: Vec<Animation>,
    pub triangles: usize,
    pub vertices: usize,
}

/// The node tree at a time of an animation.
pub struct Pose {
    pub world: Vec<Mat4>,
    /// Morph target weights of each node.
    pub weights: Vec<Vec<f32>>,
}

/// One primitive to draw, where it is.
pub struct Draw {
    pub mesh: usize,
    pub primitive: usize,
    pub node: usize,
    pub model: Mat4,
    /// Positions and normals (six floats a vertex) of a skinned or morphed primitive, already
    /// in place: then `model` is the identity for a skinned one.
    pub deformed: Option<Vec<f32>>,
}

/// Largest file read, in bytes.
pub const MAX_SIZE: u64 = 512 * 1024 * 1024;

pub fn load(path: &Path) -> Result<Scene, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_SIZE {
        return Err(tr!("gltf.file_too_large").into());
    }
    let (document, buffers, images) = gltf::import(path).map_err(|e| e.to_string())?;
    Scene::new(&document, &buffers, &images)
}

#[cfg(test)]
pub fn load_slice(bytes: &[u8]) -> Result<Scene, String> {
    let (document, buffers, images) = gltf::import_slice(bytes).map_err(|e| e.to_string())?;
    Scene::new(&document, &buffers, &images)
}

fn rgba(image: &gltf::image::Data) -> Vec<u8> {
    use gltf::image::Format;
    let pixels = image.pixels.as_slice();
    let (channels, size) = match image.format {
        Format::R8 => (1, 1),
        Format::R8G8 => (2, 1),
        Format::R8G8B8 => (3, 1),
        Format::R8G8B8A8 => (4, 1),
        Format::R16 => (1, 2),
        Format::R16G16 => (2, 2),
        Format::R16G16B16 => (3, 2),
        Format::R16G16B16A16 => (4, 2),
        Format::R32G32B32FLOAT => (3, 4),
        Format::R32G32B32A32FLOAT => (4, 4),
    };
    let channel = |c: &[u8]| -> u8 {
        match size {
            1 => c[0],
            // Little-endian: the high byte is the second.
            2 => c[1],
            _ => {
                let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                (v.clamp(0.0, 1.0) * 255.0).round() as u8
            }
        }
    };
    let mut out = Vec::with_capacity((image.width * image.height * 4) as usize);
    for pixel in pixels.chunks_exact(channels * size) {
        let c: Vec<u8> = pixel.chunks_exact(size).map(channel).collect();
        out.extend_from_slice(&match channels {
            1 => [c[0], c[0], c[0], 255],
            2 => [c[0], c[0], c[0], c[1]],
            3 => [c[0], c[1], c[2], 255],
            _ => [c[0], c[1], c[2], c[3]],
        });
    }
    out
}

fn wrap(mode: gltf::texture::WrappingMode) -> Wrap {
    use gltf::texture::WrappingMode;
    match mode {
        WrappingMode::Repeat => Wrap::Repeat,
        WrappingMode::ClampToEdge => Wrap::Clamp,
        WrappingMode::MirroredRepeat => Wrap::Mirror,
    }
}

/// Triangle list of a strip or a fan.
pub fn triangles(mode: gltf::mesh::Mode, indices: Vec<u32>) -> Option<Vec<u32>> {
    use gltf::mesh::Mode;
    match mode {
        Mode::Triangles => Some(indices),
        Mode::TriangleStrip => Some(
            (2..indices.len())
                .flat_map(|i| match i % 2 {
                    0 => [indices[i - 2], indices[i - 1], indices[i]],
                    _ => [indices[i - 1], indices[i - 2], indices[i]],
                })
                .collect(),
        ),
        Mode::TriangleFan => Some(
            (2..indices.len())
                .flat_map(|i| [indices[0], indices[i - 1], indices[i]])
                .collect(),
        ),
        _ => None,
    }
}

/// `values` when there is one per vertex, else `fill` for each.
fn fit<T: Clone>(values: Vec<T>, fill: T, n: usize) -> Vec<T> {
    if values.len() == n {
        values
    } else {
        vec![fill; n]
    }
}

/// Smooth normals, for a primitive that has none.
pub fn compute_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for t in indices.as_chunks::<3>().0 {
        let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
        let n = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
        normals[a] += n;
        normals[b] += n;
        normals[c] += n;
    }
    normals.iter().map(|n| n.normalize_or(Vec3::Y)).collect()
}

impl Scene {
    pub fn new(
        document: &gltf::Document,
        buffers: &[gltf::buffer::Data],
        images: &[gltf::image::Data],
    ) -> Result<Self, String> {
        let textures = document
            .textures()
            .map(|texture| {
                let sampler = texture.sampler();
                let image = images.get(texture.source().index());
                Texture {
                    width: image.map_or(1, |i| i.width),
                    height: image.map_or(1, |i| i.height),
                    rgba: image.map_or_else(|| vec![255; 4], rgba),
                    wrap: [wrap(sampler.wrap_s()), wrap(sampler.wrap_t())],
                    nearest: sampler.mag_filter() == Some(gltf::texture::MagFilter::Nearest),
                }
            })
            .collect();

        let mut materials: Vec<Material> = document
            .materials()
            .map(|m| {
                let pbr = m.pbr_metallic_roughness();
                Material {
                    base_color: pbr.base_color_factor(),
                    texture: pbr
                        .base_color_texture()
                        .filter(|info| info.tex_coord() == 0)
                        .map(|info| info.texture().index()),
                    alpha: match m.alpha_mode() {
                        gltf::material::AlphaMode::Opaque => Alpha::Opaque,
                        gltf::material::AlphaMode::Mask => {
                            Alpha::Mask(m.alpha_cutoff().unwrap_or(0.5))
                        }
                        gltf::material::AlphaMode::Blend => Alpha::Blend,
                    },
                    emissive: m.emissive_factor(),
                }
            })
            .collect();
        let default_material = materials.len();
        materials.push(Material::default());

        let read = |b: gltf::Buffer| buffers.get(b.index()).map(|d| d.0.as_slice());
        let mut triangle_count = 0;
        let mut vertex_count = 0;
        let mut meshes = Vec::new();
        for mesh in document.meshes() {
            let mut primitives = Vec::new();
            for primitive in mesh.primitives() {
                let reader = primitive.reader(read);
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let positions: Vec<Vec3> = positions.map(Vec3::from).collect();
                let n = positions.len();
                let indices = match reader.read_indices() {
                    Some(indices) => indices.into_u32().collect(),
                    None => (0..n as u32).collect(),
                };
                let Some(mut indices) = triangles(primitive.mode(), indices) else {
                    continue;
                };
                // A triangle with a vertex that does not exist is left out.
                let mut kept = Vec::with_capacity(indices.len());
                for t in indices.as_chunks::<3>().0 {
                    if t.iter().all(|&i| (i as usize) < n) {
                        kept.extend_from_slice(t);
                    }
                }
                indices = kept;
                let normals = match reader.read_normals() {
                    Some(normals) => {
                        let normals: Vec<Vec3> = normals.map(Vec3::from).collect();
                        if normals.len() == n {
                            normals
                        } else {
                            compute_normals(&positions, &indices)
                        }
                    }
                    None => compute_normals(&positions, &indices),
                };
                let uvs = fit(
                    reader
                        .read_tex_coords(0)
                        .map(|t| t.into_f32().collect())
                        .unwrap_or_default(),
                    [0.0; 2],
                    n,
                );
                let colors = fit(
                    reader
                        .read_colors(0)
                        .map(|c| c.into_rgba_f32().collect())
                        .unwrap_or_default(),
                    [1.0; 4],
                    n,
                );
                let mut joints: Vec<[u16; 4]> = reader
                    .read_joints(0)
                    .map(|j| j.into_u16().collect())
                    .unwrap_or_default();
                let mut weights: Vec<[f32; 4]> = reader
                    .read_weights(0)
                    .map(|w| w.into_f32().collect())
                    .unwrap_or_default();
                if joints.len() != n || weights.len() != n {
                    joints.clear();
                    weights.clear();
                }
                let targets = reader
                    .read_morph_targets()
                    .map(|(p, normals, _)| Target {
                        positions: p
                            .map(|p| p.map(Vec3::from).collect())
                            .filter(|p: &Vec<Vec3>| p.len() == n)
                            .unwrap_or_default(),
                        normals: normals
                            .map(|p| p.map(Vec3::from).collect())
                            .filter(|p: &Vec<Vec3>| p.len() == n)
                            .unwrap_or_default(),
                    })
                    .collect();
                let (min, max) = positions
                    .iter()
                    .fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
                triangle_count += indices.len() / 3;
                vertex_count += n;
                primitives.push(Primitive {
                    center: (min + max) / 2.0,
                    positions,
                    normals,
                    uvs,
                    colors,
                    indices,
                    joints,
                    weights,
                    targets,
                    material: primitive
                        .material()
                        .index()
                        .filter(|&i| i < default_material)
                        .unwrap_or(default_material),
                });
            }
            meshes.push(Mesh {
                primitives,
                weights: mesh.weights().map(<[f32]>::to_vec).unwrap_or_default(),
            });
        }

        let nodes: Vec<Node> = document
            .nodes()
            .map(|node| {
                let (t, r, s) = node.transform().decomposed();
                Node {
                    children: node.children().map(|c| c.index()).collect(),
                    translation: Vec3::from(t),
                    rotation: Quat::from_array(r).normalize(),
                    scale: Vec3::from(s),
                    mesh: node.mesh().map(|m| m.index()),
                    skin: node.skin().map(|s| s.index()),
                    weights: node.weights().map(<[f32]>::to_vec),
                }
            })
            .collect();

        let skins = document
            .skins()
            .map(|skin| {
                let joints: Vec<usize> = skin.joints().map(|j| j.index()).collect();
                let mut inverse_bind: Vec<Mat4> = skin
                    .reader(read)
                    .read_inverse_bind_matrices()
                    .map(|m| m.map(|m| Mat4::from_cols_array_2d(&m)).collect())
                    .unwrap_or_default();
                inverse_bind.resize(joints.len(), Mat4::IDENTITY);
                Skin {
                    joints,
                    inverse_bind,
                }
            })
            .collect();

        let animations = document
            .animations()
            .enumerate()
            .map(|(i, animation)| {
                let channels: Vec<Channel> = animation
                    .channels()
                    .filter_map(|channel| read_channel(&channel, read))
                    .collect();
                let duration = channels
                    .iter()
                    .filter_map(|c| c.times.last().copied())
                    .fold(0.0, f32::max);
                Animation {
                    name: animation
                        .name()
                        .map(str::to_string)
                        .unwrap_or_else(|| trf!("gltf.animation_number", n = i + 1)),
                    channels,
                    duration,
                }
            })
            .collect();

        // The node tree, from the roots of the scene shown.
        let mut parents = vec![None; nodes.len()];
        for (i, node) in nodes.iter().enumerate() {
            for &child in &node.children {
                if child < nodes.len() && parents[child].is_none() {
                    parents[child] = Some(i);
                }
            }
        }
        let roots: Vec<usize> = match document
            .default_scene()
            .or_else(|| document.scenes().next())
        {
            Some(scene) => scene.nodes().map(|n| n.index()).collect(),
            None => (0..nodes.len()).filter(|&i| parents[i].is_none()).collect(),
        };
        let mut order = Vec::with_capacity(nodes.len());
        let mut seen = vec![false; nodes.len()];
        let mut stack: Vec<usize> = roots.into_iter().rev().collect();
        while let Some(node) = stack.pop() {
            if node >= nodes.len() || seen[node] {
                continue;
            }
            seen[node] = true;
            order.push(node);
            stack.extend(nodes[node].children.iter().rev().copied());
        }
        // A root of the scene is no one's child, whatever the tree says.
        let mut placed = vec![false; nodes.len()];
        for &node in &order {
            if parents[node].is_some_and(|p| !placed[p]) {
                parents[node] = None;
            }
            placed[node] = true;
        }

        Ok(Self {
            nodes,
            order,
            parents,
            meshes,
            skins,
            materials,
            textures,
            animations,
            triangles: triangle_count,
            vertices: vertex_count,
        })
    }

    /// The node tree at `time` seconds of `animation`, or at rest.
    pub fn pose(&self, animation: Option<usize>, time: f32) -> Pose {
        let mut local: Vec<(Vec3, Quat, Vec3)> = self
            .nodes
            .iter()
            .map(|n| (n.translation, n.rotation, n.scale))
            .collect();
        let mut weights: Vec<Vec<f32>> = self
            .nodes
            .iter()
            .map(|n| {
                n.weights.clone().unwrap_or_else(|| {
                    n.mesh
                        .and_then(|m| self.meshes.get(m))
                        .map(|m| m.weights.clone())
                        .unwrap_or_default()
                })
            })
            .collect();
        if let Some(animation) = animation.and_then(|a| self.animations.get(a)) {
            let mut value = Vec::new();
            for channel in &animation.channels {
                if channel.node >= local.len() {
                    continue;
                }
                value.resize(channel.width, 0.0);
                channel.sample(time, &mut value);
                let (t, r, s) = &mut local[channel.node];
                match channel.property {
                    Property::Translation => *t = Vec3::from_slice(&value),
                    Property::Rotation => {
                        *r = Quat::from_slice(&value).normalize();
                    }
                    Property::Scale => *s = Vec3::from_slice(&value),
                    Property::Weights => weights[channel.node].clone_from(&value),
                }
            }
        }
        let mut world = vec![Mat4::IDENTITY; self.nodes.len()];
        for &node in &self.order {
            let (t, r, s) = local[node];
            let matrix = Mat4::from_scale_rotation_translation(s, r, t);
            world[node] = match self.parents[node] {
                Some(parent) => world[parent] * matrix,
                None => matrix,
            };
        }
        Pose { world, weights }
    }

    /// What to draw for a pose.
    pub fn draws(&self, pose: &Pose) -> Vec<Draw> {
        let mut draws = Vec::new();
        for &index in &self.order {
            let node = &self.nodes[index];
            let Some(mesh_index) = node.mesh.filter(|&m| m < self.meshes.len()) else {
                continue;
            };
            let mesh = &self.meshes[mesh_index];
            let skin = node.skin.and_then(|s| self.skins.get(s));
            let joints: Option<Vec<Mat4>> = skin.map(|skin| {
                skin.joints
                    .iter()
                    .zip(&skin.inverse_bind)
                    .map(|(&joint, inverse)| {
                        pose.world.get(joint).copied().unwrap_or(Mat4::IDENTITY) * *inverse
                    })
                    .collect()
            });
            let deforms = skin.is_some() || mesh.morphs();
            for (p, primitive) in mesh.primitives.iter().enumerate() {
                let skinned = joints.is_some() && !primitive.joints.is_empty();
                draws.push(Draw {
                    mesh: mesh_index,
                    primitive: p,
                    node: index,
                    model: if skinned {
                        Mat4::IDENTITY
                    } else {
                        pose.world[index]
                    },
                    deformed: deforms.then(|| {
                        deform(
                            primitive,
                            &pose.weights[index],
                            joints.as_deref().filter(|_| skinned),
                        )
                    }),
                });
            }
        }
        draws
    }

    /// Box around what the draws show, in world space.
    pub fn bounds(&self, draws: &[Draw]) -> Option<(Vec3, Vec3)> {
        let mut min = Vec3::MAX;
        let mut max = Vec3::MIN;
        for draw in draws {
            let primitive = &self.meshes[draw.mesh].primitives[draw.primitive];
            match &draw.deformed {
                Some(data) => {
                    for v in data.as_chunks::<6>().0 {
                        let p = draw.model.transform_point3(Vec3::new(v[0], v[1], v[2]));
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
                None => {
                    for p in &primitive.positions {
                        let p = draw.model.transform_point3(*p);
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            }
        }
        (min.x <= max.x && min.is_finite() && max.is_finite()).then_some((min, max))
    }
}

/// Positions and normals of a primitive with its morph targets and skin applied.
fn deform(primitive: &Primitive, weights: &[f32], joints: Option<&[Mat4]>) -> Vec<f32> {
    let mut positions = primitive.positions.clone();
    let mut normals = primitive.normals.clone();
    for (target, &w) in primitive.targets.iter().zip(weights) {
        if w == 0.0 {
            continue;
        }
        for (p, d) in positions.iter_mut().zip(&target.positions) {
            *p += *d * w;
        }
        for (n, d) in normals.iter_mut().zip(&target.normals) {
            *n += *d * w;
        }
    }
    let mut out = Vec::with_capacity(positions.len() * 6);
    for i in 0..positions.len() {
        let (mut p, mut n) = (positions[i], normals[i]);
        if let Some(joints) = joints {
            let mut m = Mat4::ZERO;
            let mut total = 0.0;
            for k in 0..4 {
                let w = primitive.weights[i][k];
                if let Some(joint) = joints.get(primitive.joints[i][k] as usize)
                    && w != 0.0
                {
                    m += *joint * w;
                    total += w;
                }
            }
            if total > 0.0 {
                m *= 1.0 / total;
                p = m.transform_point3(p);
                n = m.transform_vector3(n);
            }
        }
        let n = n.normalize_or(Vec3::Y);
        out.extend_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z]);
    }
    out
}

fn read_channel<'a, 's, F>(channel: &gltf::animation::Channel<'a>, read: F) -> Option<Channel>
where
    F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
{
    use gltf::animation::{Property as P, util::ReadOutputs};
    let reader = channel.reader(read);
    let times: Vec<f32> = reader.read_inputs()?.collect();
    let values: Vec<f32> = match reader.read_outputs()? {
        ReadOutputs::Translations(v) => v.flatten().collect(),
        ReadOutputs::Rotations(v) => v.into_f32().flatten().collect(),
        ReadOutputs::Scales(v) => v.flatten().collect(),
        ReadOutputs::MorphTargetWeights(v) => v.into_f32().collect(),
    };
    let interpolation = match channel.sampler().interpolation() {
        gltf::animation::Interpolation::Linear => Interpolation::Linear,
        gltf::animation::Interpolation::Step => Interpolation::Step,
        gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
    };
    let keys = times.len()
        * if interpolation == Interpolation::CubicSpline {
            3
        } else {
            1
        };
    if keys == 0 || values.is_empty() || !values.len().is_multiple_of(keys) {
        return None;
    }
    Some(Channel {
        node: channel.target().node().index(),
        property: match channel.target().property() {
            P::Translation => Property::Translation,
            P::Rotation => Property::Rotation,
            P::Scale => Property::Scale,
            P::MorphTargetWeights => Property::Weights,
        },
        interpolation,
        width: values.len() / keys,
        times,
        values,
    })
}

impl Channel {
    fn key(&self, i: usize) -> &[f32] {
        let w = self.width;
        match self.interpolation {
            Interpolation::CubicSpline => &self.values[(i * 3 + 1) * w..][..w],
            _ => &self.values[i * w..][..w],
        }
    }

    /// The value at `time`, in `out` (`width` floats).
    pub fn sample(&self, time: f32, out: &mut [f32]) {
        let last = self.times.len() - 1;
        if time <= self.times[0] || last == 0 {
            out.copy_from_slice(self.key(0));
            return;
        }
        if time >= self.times[last] {
            out.copy_from_slice(self.key(last));
            return;
        }
        let i = self.times.partition_point(|&t| t <= time) - 1;
        let (t0, t1) = (self.times[i], self.times[i + 1]);
        let dt = t1 - t0;
        let s = if dt > 0.0 { (time - t0) / dt } else { 0.0 };
        let (a, b) = (self.key(i), self.key(i + 1));
        match self.interpolation {
            Interpolation::Step => out.copy_from_slice(a),
            Interpolation::Linear if self.property == Property::Rotation && self.width == 4 => {
                let q = Quat::from_slice(a).slerp(Quat::from_slice(b), s);
                out.copy_from_slice(&q.to_array());
            }
            Interpolation::Linear => {
                for k in 0..self.width {
                    out[k] = a[k] + (b[k] - a[k]) * s;
                }
            }
            Interpolation::CubicSpline => {
                let w = self.width;
                let out_tangent = &self.values[(i * 3 + 2) * w..][..w];
                let in_tangent = &self.values[((i + 1) * 3) * w..][..w];
                let (s2, s3) = (s * s, s * s * s);
                let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
                let h10 = s3 - 2.0 * s2 + s;
                let h01 = -2.0 * s3 + 3.0 * s2;
                let h11 = s3 - s2;
                for k in 0..w {
                    out[k] = h00 * a[k]
                        + h10 * dt * out_tangent[k]
                        + h01 * b[k]
                        + h11 * dt * in_tangent[k];
                }
                if self.property == Property::Rotation && w == 4 {
                    let q = Quat::from_slice(out).normalize();
                    out.copy_from_slice(&q.to_array());
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/gltf/scene/tests.rs"]
mod tests;
