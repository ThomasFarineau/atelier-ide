use super::*;

#[test]
fn models_are_recognized_and_have_no_layout_switch() {
    let plugin = GltfPlugin::default();
    assert!(plugin.opens_binary(Path::new("assets/Robot.GLB")));
    assert!(plugin.opens_binary(Path::new("scene.gltf")));
    assert!(!plugin.opens_binary(Path::new("scene.obj")));
    assert_eq!(
        plugin.editor_layout(Path::new("x.glb")),
        Some(EditorLayout::Preview)
    );
    assert!(!plugin.layout_switch(Path::new("x.glb")));
}

#[test]
fn the_player_loops_or_stops_at_the_end() {
    let mut player = Player::new(1);
    assert_eq!(player.animation, Some(0));
    assert!(player.playing);
    player.advance(1.5, 2.0);
    player.advance(1.0, 2.0);
    assert!((player.time - 0.5).abs() < 1e-6);

    player.looping = false;
    player.speed = 2.0;
    player.advance(1.0, 2.0);
    assert_eq!(player.time, 2.0);
    assert!(!player.playing);
    // Playing again after the end starts over.
    player.toggle(2.0);
    assert!(player.playing);
    assert_eq!(player.time, 0.0);
}

#[test]
fn without_animation_the_player_does_nothing() {
    let mut player = Player::new(0);
    assert_eq!(player.animation, None);
    assert!(!player.playing);
    player.advance(1.0, 0.0);
    assert_eq!(player.time, 0.0);
}

#[test]
fn the_camera_frames_the_box_and_turns_around_it() {
    let mut orbit = Orbit::framing(Vec3::splat(-1.0), Vec3::ONE);
    assert_eq!(orbit.target, Vec3::ZERO);
    let distance = orbit.eye().distance(orbit.target);
    assert!((distance - orbit.distance).abs() < 1e-4);
    // The whole box is in the view: farther than its radius.
    assert!(orbit.distance > 3f32.sqrt());

    orbit.rotate(100.0, 1000.0);
    assert!(orbit.pitch <= 1.55);
    let before = orbit.distance;
    orbit.zoom(0.5);
    assert_eq!(orbit.distance, before * 0.5);

    let mut moved = Orbit::framing(Vec3::splat(-1.0), Vec3::ONE);
    moved.pan(10.0, 0.0, 100.0);
    assert!(moved.target.length() > 0.0);
    assert!((moved.target.y).abs() < 1e-5);
}

#[test]
fn times_are_minutes_and_seconds() {
    assert_eq!(format_time(0.0), "0:00.00");
    assert_eq!(format_time(75.5), "1:15.50");
}

#[test]
fn the_grid_lies_under_the_model() {
    let lines = render::grid(Vec3::new(-1.0, -2.0, -1.0), Vec3::ONE, [1.0; 3]);
    assert!(!lines.is_empty());
    assert_eq!(lines.len() % 12, 0);
    assert!(lines.as_chunks::<12>().0.iter().all(|v| v[1] == -2.0));
}
