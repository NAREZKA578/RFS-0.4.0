// Integration tests for the render::camera module.
//
// Covers: Camera trait, PerspectiveCamera, OrthographicCamera,
// CameraBuilder, CameraType, CameraController, CameraKey.

use glam::{Mat4, Vec3};
use std::time::Duration;
use rfs_client::render::camera::{
    camera::{CameraBuilder, CameraType},
    controller::CameraKey,
    Camera, CameraController, OrthographicCamera, PerspectiveCamera,
};

const EPSILON: f32 = 1e-4;

/// Project a world-space point to NDC using the given matrix.
fn ndc(vp: &Mat4, point: Vec3) -> Vec3 {
    let clip = *vp * point.extend(1.0);
    clip.truncate() / clip.w
}

fn mats_close(a: Mat4, b: Mat4, eps: f32) -> bool {
    a.to_cols_array()
        .iter()
        .zip(b.to_cols_array().iter())
        .all(|(x, y)| (x - y).abs() <= eps)
}

#[test]
fn perspective_camera_fields() {
    let cam = PerspectiveCamera::new(Vec3::new(1.0, 2.0, 3.0), 1.1, 16.0 / 9.0, 0.5, 500.0);
    assert_eq!(cam.position, Vec3::new(1.0, 2.0, 3.0));
    assert!((cam.fov - 1.1).abs() < EPSILON);
    assert!((cam.aspect_ratio - 16.0 / 9.0).abs() < EPSILON);
    assert!((cam.near_plane - 0.5).abs() < EPSILON);
    assert!((cam.far_plane - 500.0).abs() < EPSILON);
}

#[test]
fn perspective_camera_default() {
    let cam = PerspectiveCamera::default();
    assert_eq!(cam.position, Vec3::new(0.0, 0.0, 5.0));
    assert!((cam.fov - std::f32::consts::PI / 3.0).abs() < EPSILON);
    assert_eq!(cam.aspect_ratio, 16.0 / 9.0);
}

#[test]
fn view_matrix_non_identity_for_offset_camera() {
    let cam = PerspectiveCamera::new(Vec3::new(5.0, 0.0, 0.0), 1.0, 1.0, 0.1, 100.0);
    assert!(!mats_close(cam.view_matrix(), Mat4::IDENTITY, EPSILON));
    let origin = PerspectiveCamera::new(Vec3::ZERO, 1.0, 1.0, 0.1, 100.0);
    assert!(!mats_close(origin.view_matrix(), Mat4::IDENTITY, EPSILON));
}

#[test]
fn perspective_projection_maps_near_and_far_to_ndc() {
    let cam = PerspectiveCamera::new(Vec3::ZERO, std::f32::consts::PI / 2.0, 1.0, 0.1, 100.0);
    let vp = cam.view_projection_matrix();
    let near_world = cam.position() + cam.forward() * cam.near_plane();
    let far_world = cam.position() + cam.forward() * cam.far_plane();
    let near_ndc = ndc(&vp, near_world);
    let far_ndc = ndc(&vp, far_world);
    assert!((near_ndc.z - (-1.0)).abs() < 0.01, "near NDC z = {}", near_ndc.z);
    assert!((far_ndc.z - 1.0).abs() < 0.01, "far NDC z = {}", far_ndc.z);
}

#[test]
fn perspective_projection_perspective_divide() {
    let cam = PerspectiveCamera::new(Vec3::ZERO, std::f32::consts::PI / 2.0, 1.0, 0.1, 100.0);
    let vp = cam.view_projection_matrix();
    let center = cam.position() + cam.forward() * 5.0;
    let center_ndc = ndc(&vp, center);
    assert!((center_ndc.z).abs() < 1.0);
    assert!(center_ndc.z > -1.0);
}

#[test]
fn view_projection_is_composition() {
    let cam = PerspectiveCamera::new(Vec3::new(0.0, 3.0, 10.0), 0.9, 4.0 / 3.0, 0.1, 200.0);
    let composed = cam.projection_matrix() * cam.view_matrix();
    assert!(mats_close(composed, cam.view_projection_matrix(), EPSILON));
}

#[test]
fn direction_vectors_unit_length() {
    let cam = PerspectiveCamera::default();
    assert!((cam.forward().length() - 1.0).abs() < EPSILON);
    assert!((cam.up().length() - 1.0).abs() < EPSILON);
    assert!((cam.right().length() - 1.0).abs() < EPSILON);
    assert!(cam.forward().dot(cam.up()).abs() < EPSILON);
    assert!(cam.forward().dot(cam.right()).abs() < EPSILON);
}

#[test]
fn perspective_camera_getters() {
    let cam = PerspectiveCamera::new(Vec3::ZERO, 1.2, 2.0, 0.01, 1000.0);
    assert!((cam.near_plane() - 0.01).abs() < EPSILON);
    assert!((cam.far_plane() - 1000.0).abs() < EPSILON);
    assert!((cam.aspect_ratio() - 2.0).abs() < EPSILON);
    assert!((cam.fov() - 1.2).abs() < EPSILON);
    assert_eq!(cam.position(), Vec3::ZERO);
}

#[test]
fn camera_builder_perspective() {
    let cam: Box<dyn Camera> = CameraBuilder::new(CameraType::Perspective)
        .with_position(Vec3::new(0.0, 5.0, 20.0))
        .with_fov(1.1)
        .with_aspect_ratio(2.0)
        .build();
    assert_eq!(cam.position(), Vec3::new(0.0, 5.0, 20.0));
    assert!((cam.fov() - 1.1).abs() < EPSILON);
    assert!((cam.aspect_ratio() - 2.0).abs() < EPSILON);
}

#[test]
fn camera_builder_orthographic() {
    let cam: Box<dyn Camera> = CameraBuilder::new(CameraType::Orthographic)
        .with_position(Vec3::new(1.0, 1.0, 1.0))
        .build();
    assert_eq!(cam.position(), Vec3::new(1.0, 1.0, 1.0));
    // Orthographic cameras report a FOV of 0.
    assert_eq!(cam.fov(), 0.0);
}

#[test]
fn orthographic_camera_construction() {
    let cam = OrthographicCamera::new(-10.0, 10.0, -5.0, 5.0, 0.1, 1000.0);
    assert_eq!(cam.position, Vec3::ZERO);
    assert!((cam.left - -10.0).abs() < EPSILON);
    assert!((cam.right - 10.0).abs() < EPSILON);
    assert!((cam.bottom - -5.0).abs() < EPSILON);
    assert!((cam.top - 5.0).abs() < EPSILON);
    assert_eq!(cam.aspect_ratio(), 2.0);
}

#[test]
fn orthographic_projection_maps_near_and_far_to_ndc() {
    let cam = OrthographicCamera::new(-5.0, 5.0, -5.0, 5.0, 0.1, 100.0);
    let vp = cam.view_projection_matrix();
    let near_world = cam.position() + cam.forward() * cam.near_plane();
    let far_world = cam.position() + cam.forward() * cam.far_plane();
    assert!((ndc(&vp, near_world).z - (-1.0)).abs() < 0.01);
    assert!((ndc(&vp, far_world).z - 1.0).abs() < 0.01);
}

#[test]
fn camera_controller_defaults() {
    let controller = CameraController::default();
    assert_eq!(controller.move_speed, 5.0);
    assert_eq!(controller.rotate_speed, 1.5);
    assert_eq!(controller.zoom_speed, 1.0);
    assert_eq!(controller.mouse_sensitivity, 0.1);
    assert!(!controller.invert_y);
    assert!(controller.enabled);
}

#[test]
fn camera_controller_move_forward_backward() {
    let mut controller =
        CameraController::new(Box::new(PerspectiveCamera::default()) as Box<dyn Camera>);
    let start = controller.camera().position();

    controller.handle_keyboard(CameraKey::Forward, true);
    controller.update(Duration::from_secs_f32(1.0));
    controller.handle_keyboard(CameraKey::Forward, false);

    let after_forward = controller.camera().position();
    let delta = after_forward - start;
    assert!((delta.length() - controller.move_speed).abs() < 0.01);
    assert!(delta.z > 0.0);

    controller.handle_keyboard(CameraKey::Backward, true);
    controller.update(Duration::from_secs_f32(1.0));
    controller.handle_keyboard(CameraKey::Backward, false);

    let end = controller.camera().position();
    assert!((end - start).length() < 0.01);
}

#[test]
fn camera_controller_move_left_right() {
    let mut controller =
        CameraController::new(Box::new(PerspectiveCamera::default()) as Box<dyn Camera>);
    let start = controller.camera().position();

    controller.handle_keyboard(CameraKey::Right, true);
    controller.update(Duration::from_secs_f32(1.0));
    controller.handle_keyboard(CameraKey::Right, false);

    let end = controller.camera().position();
    assert!((end - start).length() > 0.9);
    assert!((end - start).length() <= controller.move_speed + 0.01);
}

#[test]
fn camera_controller_rotate_changes_rotation() {
    let mut controller =
        CameraController::new(Box::new(PerspectiveCamera::default()) as Box<dyn Camera>);
    let forward_before = controller.camera().forward();

    controller.handle_keyboard(CameraKey::RotateRight, true);
    controller.update(Duration::from_secs_f32(1.0));
    controller.handle_keyboard(CameraKey::RotateRight, false);

    assert!((controller.yaw - controller.rotate_speed).abs() < 0.001);
    let forward_after = controller.camera().forward();
    assert!((forward_before - forward_after).length() > 0.1);
    assert!(forward_after.x > 0.1);
}

#[test]
fn camera_controller_zoom_orthographic() {
    let mut controller = CameraController::new(Box::new(OrthographicCamera::new(
        -10.0, 10.0, -10.0, 10.0, 0.1, 100.0,
    )));
    let before = {
        let cam = controller
            .camera()
            .as_any()
            .downcast_ref::<OrthographicCamera>()
            .unwrap();
        cam.right - cam.left
    };

    controller.handle_mouse_wheel(10.0);

    let after = {
        let cam = controller
            .camera()
            .as_any()
            .downcast_ref::<OrthographicCamera>()
            .unwrap();
        cam.right - cam.left
    };
    assert!(after < before);
}

#[test]
fn camera_controller_disable_blocks_updates() {
    let mut controller =
        CameraController::new(Box::new(PerspectiveCamera::default()) as Box<dyn Camera>);
    let start = controller.camera().position();
    controller.disable();
    assert!(!controller.enabled);

    controller.handle_keyboard(CameraKey::Forward, true);
    controller.update(Duration::from_secs_f32(1.0));

    let end = controller.camera().position();
    assert_eq!(start, end);
}

#[test]
fn camera_controller_mouse_input() {
    let mut controller =
        CameraController::new(Box::new(PerspectiveCamera::default()) as Box<dyn Camera>);
    let forward_before = controller.camera().forward();
    controller.handle_mouse(20.0, 0.0);
    // yaw -= dx * sensitivity => yaw becomes negative.
    assert!((controller.yaw - (-20.0 * 0.1)).abs() < 0.001);
    assert!((controller.camera().forward() - forward_before).length() > 0.01);
}