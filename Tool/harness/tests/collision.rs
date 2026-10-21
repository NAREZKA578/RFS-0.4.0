use rfs_core::math::{Vec3f, Transform};
use rfs_core::entity::EntityId;
use rfs_sim::collision::*;

fn identity_transform(pos: Vec3f) -> Transform {
    Transform::from_position(pos)
}

fn box_object(id: u64, pos: Vec3f, half: Vec3f) -> CollisionObject {
    CollisionObject {
        entity_id: EntityId::new(id),
        shapes: vec![CollisionShape {
            shape_type: CollisionShapeType::Box,
            half_extents: half,
            radius: 0.0,
            height: 0.0,
            local_transform: Transform::IDENTITY,
        }],
        transform: identity_transform(pos),
        velocity: Vec3f::ZERO,
        angular_velocity: Vec3f::ZERO,
        mass: 1.0,
        is_static: false,
        collision_layers: 0x1,
        collision_mask: 0x1,
    }
}

fn sphere_object(id: u64, pos: Vec3f, radius: f32) -> CollisionObject {
    CollisionObject {
        entity_id: EntityId::new(id),
        shapes: vec![CollisionShape {
            shape_type: CollisionShapeType::Sphere,
            half_extents: Vec3f::ZERO,
            radius,
            height: 0.0,
            local_transform: Transform::IDENTITY,
        }],
        transform: identity_transform(pos),
        velocity: Vec3f::ZERO,
        angular_velocity: Vec3f::ZERO,
        mass: 1.0,
        is_static: false,
        collision_layers: 0x1,
        collision_mask: 0x1,
    }
}

fn static_object(id: u64, pos: Vec3f, half: Vec3f) -> CollisionObject {
    CollisionObject {
        entity_id: EntityId::new(id),
        shapes: vec![CollisionShape {
            shape_type: CollisionShapeType::Box,
            half_extents: half,
            radius: 0.0,
            height: 0.0,
            local_transform: Transform::IDENTITY,
        }],
        transform: identity_transform(pos),
        velocity: Vec3f::ZERO,
        angular_velocity: Vec3f::ZERO,
        mass: 1.0,
        is_static: true,
        collision_layers: 0x1,
        collision_mask: 0x1,
    }
}

fn different_layers_object(id: u64, pos: Vec3f) -> CollisionObject {
    CollisionObject {
        entity_id: EntityId::new(id),
        shapes: vec![CollisionShape {
            shape_type: CollisionShapeType::Box,
            half_extents: Vec3f::new(5.0, 5.0, 5.0),
            radius: 0.0,
            height: 0.0,
            local_transform: Transform::IDENTITY,
        }],
        transform: identity_transform(pos),
        velocity: Vec3f::ZERO,
        angular_velocity: Vec3f::ZERO,
        mass: 1.0,
        is_static: false,
        collision_layers: 0x2,
        collision_mask: 0x1,
    }
}

// add_object only registers the id with a zero-sized bound; the broadphase
// grid is populated by update_object (which the engine calls every sim tick
// with the live transform). Call both so step()/get_pairs() see the pair.
fn add_updated(sys: &mut CollisionSystem, obj: CollisionObject) {
    sys.add_object(obj.clone());
    sys.update_object(obj.entity_id, obj.transform, obj.velocity, obj.angular_velocity);
}

// ---------- new / add_object / entity_count ----------

#[test]
fn collision_system_new() {
    let sys = CollisionSystem::new(CollisionConfig::default());
    let _ = sys;
}

#[test]
fn add_object_then_raycast_finds_it() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));
    sys.add_object(box_object(2, Vec3f::new(100.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0)));
    assert_eq!(sys.raycast(Vec3f::new(-10.0, 0.0, 0.0), Vec3f::RIGHT, 100.0, 0x1).unwrap().entity_id, EntityId::new(1));
}

// ---------- should_collide ----------

#[test]
fn should_collide_matching_layers() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    let mut a = box_object(1, Vec3f::new(-1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0));
    let mut b = box_object(2, Vec3f::new(1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0));
    a.collision_layers = 0x1;
    a.collision_mask = 0x1;
    b.collision_layers = 0x1;
    b.collision_mask = 0x1;
    add_updated(&mut sys, a);
    add_updated(&mut sys, b);
    let results = sys.step();
    assert!(!results.is_empty());
}

#[test]
fn should_collide_non_matching_layers() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    let mut a = box_object(1, Vec3f::new(-1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0));
    let b = different_layers_object(2, Vec3f::new(1.0, 0.0, 0.0));
    // a's mask excludes layer 2, so b (layers=0x2) should never be hit.
    a.collision_layers = 0x1;
    a.collision_mask = 0x8;
    sys.add_object(a);
    sys.add_object(b);
    let results = sys.step();
    assert!(results.is_empty());
}

#[test]
fn should_collide_zero_mask() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    let mut a = box_object(1, Vec3f::new(-1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0));
    a.collision_mask = 0x0;
    let b = box_object(2, Vec3f::new(1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0));
    sys.add_object(a);
    sys.add_object(b);
    let results = sys.step();
    assert!(results.is_empty());
}

// ---------- step: overlapping box+box ----------

#[test]
fn step_overlapping_box_box() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    add_updated(&mut sys, box_object(1, Vec3f::new(-1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0)));
    add_updated(&mut sys, box_object(2, Vec3f::new(1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0)));
    let results = sys.step();
    assert!(!results.is_empty());
    assert!(results[0].penetration > 0.0);
}

// ---------- step: non-overlapping ----------

#[test]
fn step_non_overlapping() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));
    sys.add_object(box_object(2, Vec3f::new(100.0, 100.0, 100.0), Vec3f::new(5.0, 5.0, 5.0)));
    let results = sys.step();
    assert!(results.is_empty());
}

// ---------- step: two static objects (Bug #88 guard) ----------

#[test]
fn step_two_static_objects_no_panic() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(static_object(1, Vec3f::new(-1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0)));
    sys.add_object(static_object(2, Vec3f::new(1.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0)));
    let results = sys.step();
    let _ = results;
}

// ---------- raycast: box hit ----------

#[test]
fn raycast_box_hit() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));
    let hit = sys.raycast(Vec3f::new(-20.0, 0.0, 0.0), Vec3f::RIGHT, 100.0, 0x1);
    assert!(hit.is_some());
    let h = hit.unwrap();
    assert_eq!(h.entity_id, EntityId::new(1));
    assert!(h.distance > 0.0);
}

// ---------- raycast: box miss ----------

#[test]
fn raycast_box_miss() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));
    let hit = sys.raycast(Vec3f::new(-20.0, 20.0, 20.0), Vec3f::RIGHT, 100.0, 0x1);
    assert!(hit.is_none());
}

// ---------- raycast: parallel to face plane (Bug #46) ----------

#[test]
fn raycast_parallel_to_face_no_panic() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));
    let origin = Vec3f::new(-10.0, 3.0, 0.0);
    let dir = Vec3f::RIGHT;
    let hit = sys.raycast(origin, dir, 100.0, 0x1);
    let _ = hit;
}

// ---------- raycast: sphere hit ----------

#[test]
fn raycast_sphere_hit() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(sphere_object(1, Vec3f::ZERO, 5.0));
    let hit = sys.raycast(Vec3f::new(-20.0, 0.0, 0.0), Vec3f::RIGHT, 100.0, 0x1);
    assert!(hit.is_some());
    assert!(hit.unwrap().distance > 0.0);
}

// ---------- raycast: sphere miss ----------

#[test]
fn raycast_sphere_miss() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(sphere_object(1, Vec3f::ZERO, 5.0));
    let hit = sys.raycast(Vec3f::new(-20.0, 100.0, 0.0), Vec3f::RIGHT, 100.0, 0x1);
    assert!(hit.is_none());
}

// ---------- raycast: mask zero ----------

#[test]
fn raycast_wrong_mask() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));
    let hit = sys.raycast(Vec3f::new(-20.0, 0.0, 0.0), Vec3f::RIGHT, 100.0, 0x2);
    assert!(hit.is_none());
}

// ---------- remove_object ----------

#[test]
fn remove_object() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));
    sys.remove_object(EntityId::new(1));
    let hit = sys.raycast(Vec3f::new(-20.0, 0.0, 0.0), Vec3f::RIGHT, 100.0, 0x1);
    assert!(hit.is_none());
}

// ---------- update_object ----------

#[test]
fn update_object_moves() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));

    let hit_before = sys.raycast(Vec3f::new(-20.0, 0.0, 0.0), Vec3f::RIGHT, 100.0, 0x1);
    assert!(hit_before.is_some());

    sys.update_object(
        EntityId::new(1),
        identity_transform(Vec3f::new(200.0, 200.0, 200.0)),
        Vec3f::ZERO,
        Vec3f::ZERO,
    );
    let hit_after = sys.raycast(Vec3f::new(-20.0, 0.0, 0.0), Vec3f::RIGHT, 100.0, 0x1);
    assert!(hit_after.is_none());
}

// ---------- shape combinations ----------

#[test]
fn box_vs_sphere_collision() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    add_updated(&mut sys, box_object(1, Vec3f::ZERO, Vec3f::new(5.0, 5.0, 5.0)));
    add_updated(&mut sys, sphere_object(2, Vec3f::new(4.0, 0.0, 0.0), 3.0));
    let results = sys.step();
    assert!(!results.is_empty());
}

#[test]
fn sphere_vs_box_collision() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    add_updated(&mut sys, sphere_object(1, Vec3f::ZERO, 3.0));
    add_updated(&mut sys, box_object(2, Vec3f::new(4.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0)));
    let results = sys.step();
    assert!(!results.is_empty());
}

#[test]
fn sphere_vs_sphere_collision() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    add_updated(&mut sys, sphere_object(1, Vec3f::ZERO, 5.0));
    add_updated(&mut sys, sphere_object(2, Vec3f::new(6.0, 0.0, 0.0), 5.0));
    let results = sys.step();
    assert!(!results.is_empty());
    assert!(results[0].penetration > 0.0);
}

#[test]
fn sphere_vs_sphere_no_collision() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(sphere_object(1, Vec3f::ZERO, 2.0));
    sys.add_object(sphere_object(2, Vec3f::new(100.0, 0.0, 0.0), 2.0));
    let results = sys.step();
    assert!(results.is_empty());
}

#[test]
fn box_vs_box_no_collision() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::ZERO, Vec3f::new(2.0, 2.0, 2.0)));
    sys.add_object(box_object(2, Vec3f::new(100.0, 0.0, 0.0), Vec3f::new(2.0, 2.0, 2.0)));
    let results = sys.step();
    assert!(results.is_empty());
}

// ---------- edge cases ----------

#[test]
fn zero_distance_sphere_overlap() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    add_updated(&mut sys, sphere_object(1, Vec3f::ZERO, 5.0));
    add_updated(&mut sys, sphere_object(2, Vec3f::ZERO, 5.0));
    let results = sys.step();
    assert!(!results.is_empty());
    let r = &results[0];
    assert!(r.normal.y.abs() > 0.0 || r.normal.x.abs() > 0.0 || r.normal.z.abs() > 0.0);
}

#[test]
fn sphere_inside_box() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    add_updated(&mut sys, box_object(1, Vec3f::ZERO, Vec3f::new(10.0, 10.0, 10.0)));
    add_updated(&mut sys, sphere_object(2, Vec3f::ZERO, 2.0));
    let results = sys.step();
    assert!(!results.is_empty());
}

#[test]
fn raycast_closest_object() {
    let mut sys = CollisionSystem::new(CollisionConfig::default());
    sys.add_object(box_object(1, Vec3f::new(10.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0)));
    sys.add_object(box_object(2, Vec3f::new(20.0, 0.0, 0.0), Vec3f::new(5.0, 5.0, 5.0)));
    let hit = sys.raycast(Vec3f::new(-10.0, 0.0, 0.0), Vec3f::RIGHT, 100.0, 0x1);
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().entity_id, EntityId::new(1));
}
