use super::*;
use serde_json::json;

/// A GLB file of `document`, with `binary` as its buffer.
fn glb(mut document: serde_json::Value, mut binary: Vec<u8>) -> Vec<u8> {
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    document["buffers"] = json!([{ "byteLength": binary.len() }]);
    let mut text = serde_json::to_vec(&document).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + text.len() + 8 + binary.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&binary);
    out
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A triangle on a child node, moved along x by a linear animation of two seconds and turned a
/// quarter around y by a step one.
fn animated_triangle() -> Vec<u8> {
    let mut bin = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]); // 0..36
    bin.extend(floats(&[0.0, 2.0])); // 36..44 times
    bin.extend(floats(&[0.0, 0.0, 0.0, 4.0, 0.0, 0.0])); // 44..68 translations
    let s = std::f32::consts::FRAC_1_SQRT_2;
    bin.extend(floats(&[0.0, 0.0, 0.0, 1.0, 0.0, s, 0.0, s])); // 68..100 rotations
    let document = json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [
            { "children": [1], "translation": [0.0, 1.0, 0.0] },
            { "mesh": 0 }
        ],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
              "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
            { "bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR",
              "min": [0.0], "max": [2.0] },
            { "bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3" },
            { "bufferView": 3, "componentType": 5126, "count": 2, "type": "VEC4" }
        ],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 36 },
            { "buffer": 0, "byteOffset": 36, "byteLength": 8 },
            { "buffer": 0, "byteOffset": 44, "byteLength": 24 },
            { "buffer": 0, "byteOffset": 68, "byteLength": 32 }
        ],
        "animations": [{
            "name": "Slide",
            "samplers": [
                { "input": 1, "output": 2, "interpolation": "LINEAR" },
                { "input": 1, "output": 3, "interpolation": "STEP" }
            ],
            "channels": [
                { "sampler": 0, "target": { "node": 1, "path": "translation" } },
                { "sampler": 1, "target": { "node": 1, "path": "rotation" } }
            ]
        }]
    });
    glb(document, bin)
}

#[test]
fn a_glb_file_is_read_with_its_tree_and_animation() {
    let scene = load_slice(&animated_triangle()).unwrap();
    assert_eq!(scene.triangles, 1);
    assert_eq!(scene.vertices, 3);
    assert_eq!(scene.order, vec![0, 1]);
    assert_eq!(scene.parents, vec![None, Some(0)]);
    // No normals in the file: they are computed, facing +z.
    assert!(scene.meshes[0].primitives[0].normals[0].abs_diff_eq(Vec3::Z, 1e-6));
    // The default material is added after those of the file.
    assert_eq!(scene.materials.len(), 1);
    let animation = &scene.animations[0];
    assert_eq!(animation.name, "Slide");
    assert_eq!(animation.duration, 2.0);
    assert_eq!(animation.channels.len(), 2);
}

#[test]
fn the_pose_follows_the_animation_and_the_parent() {
    let scene = load_slice(&animated_triangle()).unwrap();
    let rest = scene.pose(None, 1.0);
    assert!(
        rest.world[1]
            .transform_point3(Vec3::ZERO)
            .abs_diff_eq(Vec3::Y, 1e-6)
    );

    // Halfway: two along x, under the parent one up; the step rotation is still the first key.
    let pose = scene.pose(Some(0), 1.0);
    let origin = pose.world[1].transform_point3(Vec3::ZERO);
    assert!(origin.abs_diff_eq(Vec3::new(2.0, 1.0, 0.0), 1e-5));
    let x = pose.world[1].transform_vector3(Vec3::X);
    assert!(x.abs_diff_eq(Vec3::X, 1e-5));

    // Past the end: the last keys.
    let pose = scene.pose(Some(0), 5.0);
    let origin = pose.world[1].transform_point3(Vec3::ZERO);
    assert!(origin.abs_diff_eq(Vec3::new(4.0, 1.0, 0.0), 1e-5));
    let x = pose.world[1].transform_vector3(Vec3::X);
    assert!(x.abs_diff_eq(Vec3::NEG_Z, 1e-5));

    let draws = scene.draws(&pose);
    assert_eq!(draws.len(), 1);
    assert!(draws[0].deformed.is_none());
    let (min, max) = scene.bounds(&draws).unwrap();
    assert!(min.abs_diff_eq(Vec3::new(4.0, 1.0, -1.0), 1e-5));
    assert!(max.abs_diff_eq(Vec3::new(4.0, 2.0, 0.0), 1e-5));
}

/// A triangle skinned to two joints: the first two vertices follow joint 0, the third joint 1,
/// which an animation moves one up.
fn skinned_triangle() -> Vec<u8> {
    let mut bin = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]); // 0..36 positions
    bin.extend([0u8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]); // 36..48 joints
    bin.extend(floats(&[
        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
    ])); // 48..96 weights
    bin.extend(floats(&[0.0, 1.0])); // 96..104 times
    bin.extend(floats(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0])); // 104..128 translations
    let document = json!({
        "asset": { "version": "2.0" },
        "scenes": [{ "nodes": [0, 1, 2] }],
        "nodes": [
            { "mesh": 0, "skin": 0, "translation": [100.0, 0.0, 0.0] },
            { },
            { }
        ],
        "skins": [{ "joints": [1, 2] }],
        "meshes": [{ "primitives": [{ "attributes": {
            "POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2
        } }] }],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
              "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
            { "bufferView": 1, "componentType": 5121, "count": 3, "type": "VEC4" },
            { "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC4" },
            { "bufferView": 3, "componentType": 5126, "count": 2, "type": "SCALAR",
              "min": [0.0], "max": [1.0] },
            { "bufferView": 4, "componentType": 5126, "count": 2, "type": "VEC3" }
        ],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 36 },
            { "buffer": 0, "byteOffset": 36, "byteLength": 12 },
            { "buffer": 0, "byteOffset": 48, "byteLength": 48 },
            { "buffer": 0, "byteOffset": 96, "byteLength": 8 },
            { "buffer": 0, "byteOffset": 104, "byteLength": 24 }
        ],
        "animations": [{
            "samplers": [{ "input": 3, "output": 4 }],
            "channels": [{ "sampler": 0, "target": { "node": 2, "path": "translation" } }]
        }]
    });
    glb(document, bin)
}

#[test]
fn skinned_vertices_follow_their_joints_not_their_node() {
    let scene = load_slice(&skinned_triangle()).unwrap();
    assert_eq!(scene.animations[0].name, "Animation 1");
    let draws = scene.draws(&scene.pose(Some(0), 0.5));
    let draw = &draws[0];
    assert_eq!(draw.model, Mat4::IDENTITY);
    let data = draw.deformed.as_ref().unwrap();
    let vertex = |i: usize| Vec3::from_slice(&data[i * 6..i * 6 + 3]);
    assert!(vertex(0).abs_diff_eq(Vec3::ZERO, 1e-5));
    assert!(vertex(1).abs_diff_eq(Vec3::X, 1e-5));
    assert!(vertex(2).abs_diff_eq(Vec3::new(0.0, 1.5, 0.0), 1e-5));
}

#[test]
fn cubic_splines_go_through_their_keys_and_follow_their_tangents() {
    let channel = Channel {
        node: 0,
        property: Property::Translation,
        interpolation: Interpolation::CubicSpline,
        times: vec![0.0, 1.0],
        // in tangent, value, out tangent of each key, one float wide.
        values: vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        width: 1,
    };
    let mut out = [0.0];
    channel.sample(0.0, &mut out);
    assert_eq!(out, [0.0]);
    channel.sample(1.0, &mut out);
    assert_eq!(out, [1.0]);
    // Flat tangents: the smoothstep curve.
    channel.sample(0.25, &mut out);
    assert!((out[0] - 0.15625).abs() < 1e-6);
}

#[test]
fn linear_rotations_turn_the_short_way() {
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let channel = Channel {
        node: 0,
        property: Property::Rotation,
        interpolation: Interpolation::Linear,
        times: vec![0.0, 1.0],
        values: vec![0.0, 0.0, 0.0, 1.0, 0.0, half, 0.0, half],
        width: 4,
    };
    let mut out = [0.0; 4];
    channel.sample(0.5, &mut out);
    let q = Quat::from_slice(&out);
    let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
    assert!(q.abs_diff_eq(expected, 1e-5));
}

#[test]
fn strips_and_fans_become_triangle_lists() {
    use gltf::mesh::Mode;
    assert_eq!(
        triangles(Mode::TriangleStrip, vec![0, 1, 2, 3]),
        Some(vec![0, 1, 2, 2, 1, 3])
    );
    assert_eq!(
        triangles(Mode::TriangleFan, vec![0, 1, 2, 3]),
        Some(vec![0, 1, 2, 0, 2, 3])
    );
    assert_eq!(triangles(Mode::Lines, vec![0, 1]), None);
}

#[test]
fn a_file_that_is_not_gltf_is_an_error() {
    assert!(load_slice(b"not a model").is_err());
}
