// Moved out of crates/net/src/snapshot.rs (side-only testing rule).
use rfs_core::entity::EntityId;
use rfs_core::math::{Quatf, Transform, Vec3f};
use rfs_core::packet::{EntityFlags, EntityType, StateDeltaPacket};
use rfs_core::time::Tick;
use rfs_net::{
    DeltaCompressor, EntitySnapshot, Snapshot, SnapshotBuffer, entity_layer, LAYER_COMPARTMENT,
    LAYER_PLAYER, LAYER_PROJECTILE, LAYER_SHIP,
};

fn empty_snapshot(tick: u64) -> Snapshot {
    Snapshot {
        tick: Tick(tick),
        time: tick as f64,
        entities: Vec::new(),
        projectiles: Vec::new(),
        events: Vec::new(),
    }
}

fn test_entity(id: u64, entity_type: EntityType, health: f32) -> EntitySnapshot {
    EntitySnapshot {
        entity_id: EntityId::new(id),
        entity_type,
        transform: Transform::new(Vec3f::new(0.0, 0.0, 0.0), Quatf::IDENTITY, Vec3f::ONE),
        velocity: Vec3f::new(0.0, 0.0, 0.0),
        angular_velocity: Vec3f::new(0.0, 0.0, 0.0),
        health,
        max_health: 100.0,
        flags: EntityFlags::NONE,
        ship_data: None,
        station_data: None,
        player_data: None,
    }
}

#[test]
fn buffer_latest_tracks_writes() {
    let buf = SnapshotBuffer::new(64);
    assert!(buf.get_latest().is_none());
    for tick in 1..=200u64 {
        buf.write_snapshot(empty_snapshot(tick));
        let latest = buf.get_latest().expect("latest must be present after write");
        assert_eq!(latest.tick, Tick(tick));
        assert_eq!(buf.get_snapshot(Tick(tick)).map(|s| s.tick), Some(Tick(tick)));
    }
}

#[test]
fn buffer_evicts_out_of_window_ticks() {
    let buf = SnapshotBuffer::new(8);
    for tick in 1..=20u64 {
        buf.write_snapshot(empty_snapshot(tick));
    }
    assert_eq!(buf.get_latest().map(|s| s.tick), Some(Tick(20)));
    // Ticks that fell out of the window are gone, recent ones resolve.
    assert!(buf.get_snapshot(Tick(1)).is_none());
    assert_eq!(buf.get_snapshot(Tick(20)).map(|s| s.tick), Some(Tick(20)));
    assert_eq!(buf.get_snapshot(Tick(13)).map(|s| s.tick), Some(Tick(13)));
}

#[test]
fn entity_layer_mapping_matches_plan() {
    assert_eq!(entity_layer(EntityType::Ship), LAYER_SHIP);
    assert_eq!(entity_layer(EntityType::Station), LAYER_COMPARTMENT);
    assert_eq!(entity_layer(EntityType::Compartment), LAYER_COMPARTMENT);
    assert_eq!(entity_layer(EntityType::Player), LAYER_PLAYER);
    assert_eq!(entity_layer(EntityType::Projectile), LAYER_PROJECTILE);
}

#[test]
fn layered_deltas_track_bases_independently() {
    let comp = DeltaCompressor::new();
    let mut base0 = empty_snapshot(10);
    base0.entities.push(test_entity(1, EntityType::Ship, 100.0));
    let mut target0 = empty_snapshot(12);
    target0.entities.push(test_entity(1, EntityType::Ship, 90.0));
    let d0 = comp.create_delta(7, LAYER_SHIP, &base0, &target0);
    assert_eq!(d0.layer, LAYER_SHIP);
    assert_eq!(d0.updated.len(), 1);
    assert!(!d0.is_empty());

    // Same client, another layer: its base tracking starts fresh,
    // unaffected by the ship layer's tick 12.
    let mut base2 = empty_snapshot(5);
    base2.entities.push(test_entity(2, EntityType::Player, 100.0));
    let mut target2 = empty_snapshot(6);
    target2.entities.push(test_entity(2, EntityType::Player, 80.0));
    let d2 = comp.create_delta(7, LAYER_PLAYER, &base2, &target2);
    assert_eq!(d2.layer, LAYER_PLAYER);
    assert_eq!(d2.base_tick, Tick(5));
    assert_eq!(d2.updated.len(), 1);

    let empty = comp.create_delta(7, LAYER_PROJECTILE, &empty_snapshot(6), &empty_snapshot(7));
    assert!(empty.is_empty());
}

#[test]
fn apply_delta_upserts_created_without_duplicates() {
    let mut base = empty_snapshot(1);
    base.entities.push(test_entity(1, EntityType::Ship, 100.0));
    let delta = StateDeltaPacket {
        layer: LAYER_SHIP,
        base_tick: 1,
        server_tick: 2,
        server_time: 0.0,
        created: vec![test_entity(1, EntityType::Ship, 50.0).to_state()],
        updated: vec![],
        destroyed: vec![],
        projectile_created: vec![],
        projectile_updated: vec![],
        projectile_destroyed: vec![],
    };
    let target = base.apply_delta(&delta);
    assert_eq!(target.entities.len(), 1);
    assert_eq!(target.entities[0].health, 50.0);
}
