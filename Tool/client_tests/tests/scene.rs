// Integration tests for the render::scene module.
//
// Covers: Scene, SceneBuilder, Entity, EntityBuilder, Transform,
// Aabb, Frustum, FrustumCuller.

use glam::{Mat4, Vec3};
use rfs_client::render::camera::Camera;
use rfs_client::render::scene::culling::{Aabb, Frustum, FrustumCuller};
use rfs_client::render::scene::entity::EntityBuilder;
use rfs_client::render::scene::scene::SceneBuilder;
use rfs_client::render::scene::{Entity, Scene, Transform};
use std::sync::Arc;

const EPSILON: f32 = 1e-4;

#[test]
fn scene_new_and_name() {
    let scene = Scene::new("test");
    assert_eq!(scene.name(), "test");
    assert_eq!(scene.entity_count(), 0);
}

#[test]
fn scene_default_name() {
    let scene = Scene::default();
    assert_eq!(scene.name(), "default");
    assert_eq!(scene.entity_count(), 0);
}

#[test]
fn scene_builder_builds_scene() {
    let entity = Entity::new("ship");
    let scene = SceneBuilder::new("battle").with_entity(entity).build();
    assert_eq!(scene.name(), "battle");
    assert_eq!(scene.entity_count(), 1);
}

#[test]
fn entity_builder_creates_entity() {
    let entity = EntityBuilder::new()
        .with_id(7)
        .with_name("x")
        .with_tag("player")
        .build();
    assert_eq!(entity.id(), 7);
    assert_eq!(entity.name(), "x");
    assert!(entity.has_tag("player"));
}

#[test]
fn entity_scene_assigns_ids() {
    let mut scene = Scene::new("ids");
    let first = Entity::new("a");
    let second = Entity::new("b");
    let id1 = scene.add_entity(first);
    let id2 = scene.add_entity(second);
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(scene.entity_count(), 2);
    assert_eq!(scene.get_entity(id1).unwrap().id(), id1);
    assert_eq!(scene.get_entity(id2).unwrap().name(), "b");
}

#[test]
fn scene_remove_entity() {
    let mut scene = Scene::new("rm");
    let id = scene.add_entity(Entity::new("a"));
    assert_eq!(scene.entity_count(), 1);
    assert!(scene.remove_entity(id).is_some());
    assert_eq!(scene.entity_count(), 0);
    assert!(scene.remove_entity(id).is_none());
    assert!(scene.get_entity(id).is_none());
}

#[test]
fn entity_component_roundtrip() {
    let mut entity = Entity::new("componented");
    entity.add_component(12345u32);
    entity.add_component("hello".to_string());
    assert!(entity.has_component::<u32>());
    assert_eq!(*entity.get_component::<u32>().unwrap(), 12345);
    assert_eq!(entity.get_component::<String>().unwrap(), "hello");
    assert!(entity.get_component::<f64>().is_none());

    // Mutable access
    *entity.get_component_mut::<u32>().unwrap() = 999;
    assert_eq!(*entity.get_component::<u32>().unwrap(), 999);

    // Removal
    let removed: u32 = entity.remove_component::<u32>().unwrap();
    assert_eq!(removed, 999);
    assert!(!entity.has_component::<u32>());
}

#[test]
fn entity_flags() {
    let mut entity = Entity::new("flags");
    assert!(entity.is_active());
    assert!(entity.is_visible());
    entity.set_active(false);
    entity.set_visible(false);
    assert!(!entity.is_active());
    assert!(!entity.is_visible());
}

#[test]
fn entity_transform_component() {
    let mut entity = Entity::new("transformed");
    let transform = Transform::with_position(Vec3::new(1.0, 2.0, 3.0));
    entity.add_component(transform);
    let t = entity.transform().expect("transform component");
    assert_eq!(t.position(), Vec3::new(1.0, 2.0, 3.0));
    entity
        .transform_mut()
        .expect("mutable transform")
        .set_position(Vec3::new(4.0, 5.0, 6.0));
    assert_eq!(entity.transform().unwrap().position(), Vec3::new(4.0, 5.0, 6.0));
}

#[test]
fn transform_default_is_identity() {
    let t = Transform::default();
    assert_eq!(t.position(), Vec3::ZERO);
    assert_eq!(t.rotation(), glam::Quat::IDENTITY);
    assert_eq!(t.scale(), Vec3::ONE);
}

#[test]
fn transform_matrix_translation() {
    let t = Transform::with_position(Vec3::new(1.0, 2.0, 3.0));
    let matrix = t.matrix();
    // Matrix column 3 is the translation component with w == 1.
    assert_eq!(matrix.w_axis.truncate(), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(matrix.w_axis.w, 1.0);
}

#[test]
fn transform_translate() {
    let mut t = Transform::default();
    t.translate(Vec3::new(1.0, 1.0, 1.0));
    assert_eq!(t.position(), Vec3::new(1.0, 1.0, 1.0));
}

#[test]
fn aabb_basics() {
    let aabb = Aabb::new(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(aabb.center(), Vec3::ZERO);
    assert_eq!(aabb.size(), Vec3::new(2.0, 4.0, 6.0));
    assert!(aabb.contains(Vec3::ZERO));
    assert!(aabb.contains(Vec3::new(1.0, 2.0, 3.0)));
    assert!(!aabb.contains(Vec3::new(1.1, 0.0, 0.0)));
}

#[test]
fn aabb_intersects() {
    let a = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
    let overlapping = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
    let disjoint = Aabb::new(Vec3::new(5.0, 5.0, 5.0), Vec3::new(6.0, 6.0, 6.0));
    assert!(a.intersects(&overlapping));
    assert!(!a.intersects(&disjoint));
}

#[test]
fn aabb_transform_translates_bounds() {
    let aabb = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
    let transform = Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0));
    let transformed = aabb.transform(transform);
    assert!((transformed.min.x - 9.0).abs() < EPSILON);
    assert!((transformed.max.x - 11.0).abs() < EPSILON);
    assert!((transformed.center().y - 0.0).abs() < EPSILON);
}

#[test]
fn frustum_from_view_projection_culls() {
    // Camera at origin looking towards +Z with a symmetric orthographic
    // projection (x: -1..1, y: -1..1, near 0.1, far 100).
    let cam = rfs_client::render::camera::OrthographicCamera::new(-1.0, 1.0, -1.0, 1.0, 0.1, 100.0);

    let vp = cam.view_projection_matrix();
    let frustum = Frustum::from_matrix(vp);

    // A point straight in front of the camera must be visible.
    let near_point = cam.position() + cam.forward() * 1.0;
    assert!(frustum.is_visible(&Aabb::new(near_point, near_point)));

    // A point far behind the camera must be culled.
    let behind = cam.position() - cam.forward() * 5.0;
    assert!(!frustum.is_visible(&Aabb::new(behind, behind)));

    // A point past the far plane must be culled.
    let beyond_far = cam.position() + cam.forward() * 5000.0;
    assert!(!frustum.is_visible(&Aabb::new(beyond_far, beyond_far)));
}

#[test]
fn frustum_culler_visible_and_culled() {
    let cam = rfs_client::render::camera::OrthographicCamera::new(-1.0, 1.0, -1.0, 1.0, 0.1, 100.0);
    let mut culler = FrustumCuller::new();
    culler.update(cam.view_projection_matrix());

    let inside = Aabb::new(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
    let outside = Aabb::new(cam.position() - cam.forward() * 10.0, cam.position() - cam.forward() * 10.0);
    assert!(culler.is_visible(&inside));
    assert!(!culler.is_visible(&outside));

    // Point-based test agrees with the AABB test.
    assert!(culler.is_visible_point(Vec3::new(0.0, 0.0, 1.0)));
    assert!(!culler.is_visible_point(Vec3::new(0.0, 0.0, -10.0)));
}

#[test]
fn scene_builder_with_camera_sets_active_camera() {
    use rfs_client::render::camera::PerspectiveCamera;
    use rfs_client::render::scene::components::CameraComponent;

    let mut camera_entity = Entity::new("camera");
    camera_entity.add_component(CameraComponent::new(Arc::new(PerspectiveCamera::default())));
    let scene = SceneBuilder::new("cam_scene").with_camera(camera_entity).build();
    assert!(scene.active_camera().is_some());
    assert!(scene.camera_view_projection_matrix().is_finite());
}

#[test]
fn entity_builder_transform_and_renderable() {
    use rfs_client::render::scene::components::Renderable;

    let entity = EntityBuilder::new()
        .with_id(3)
        .with_name("carrier")
        .with_transform(Transform::with_position(Vec3::new(5.0, 0.0, 5.0)))
        .with_renderable(Renderable::new().casts_shadows(true).lod_level(2))
        .build();

    assert_eq!(entity.transform().unwrap().position(), Vec3::new(5.0, 0.0, 5.0));
    assert!(entity.casts_shadows());
    assert!(entity.is_opaque());
    let renderable = entity.renderable().unwrap();
    assert_eq!(renderable.lod_level, 2);
}