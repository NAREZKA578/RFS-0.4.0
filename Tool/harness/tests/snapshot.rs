// Moved out of crates/net/src/snapshot.rs (side-only testing rule).
use rfs_core::entity::EntityId;
use rfs_core::math::{Quatf, Transform, Vec3f};
use rfs_core::packet::{EntityFlags, EntityType, ProjectileType, StateDeltaPacket};
use rfs_core::time::Tick;
use rfs_net::{
    DeltaCompressor, EntitySnapshot, ProjectileSnapshot, Snapshot, SnapshotBuffer, entity_layer,
    LAYER_COMPARTMENT, LAYER_PLAYER, LAYER_PROJECTILE, LAYER_SHIP, MAX_DELTA_CREATED,
    MAX_DELTA_PROJECTILES, MAX_DELTA_UPDATED,
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
        is_resync: false,
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

fn test_projectile(id: u64, position: Vec3f) -> ProjectileSnapshot {
    ProjectileSnapshot {
        entity_id: EntityId::new(id),
        projectile_type: ProjectileType::Cannonball,
        position,
        velocity: Vec3f::new(0.0, 0.0, 10.0),
        spawn_tick: 0,
        lifetime: 5.0,
        owner: EntityId::new(1),
        damage: 10.0,
        penetration: 1.0,
    }
}

/// Bug №271: the per-delta caps used to `truncate` the lists and send the
/// result anyway, while the client's base was advanced to the same tick. The
/// entities behind the cut were then diffed against a base the client had never
/// received — dropped as a base mismatch (№267) or applied over wrong state —
/// and the loss was permanent, with only a server-side `warn!` to show for it.
///
/// The cap is now a withhold: nothing is sent and the base stays put, so the
/// content goes out whole on a later tick.
#[test]
fn a_delta_over_the_created_cap_is_withheld_and_the_base_stays_put() {
    let comp = DeltaCompressor::new();
    let base = empty_snapshot(10);
    let mut target = empty_snapshot(11);
    for id in 0..(MAX_DELTA_CREATED as u64 + 1) {
        target
            .entities
            .push(test_entity(id, EntityType::Player, 100.0));
    }

    let delta = comp.create_delta(1, LAYER_PLAYER, &base, &target);
    assert!(
        delta.is_empty(),
        "an over-cap delta must be withheld, not truncated and sent"
    );

    // The base must still be the original one, not the withheld target tick.
    // If the ring had advanced to 11, the next delta would declare base 11 and
    // the client would reject it for a base mismatch — permanently.
    let mut small = empty_snapshot(12);
    small.entities.push(test_entity(900, EntityType::Player, 100.0));
    let next = comp.create_delta(1, LAYER_PLAYER, &base, &small);
    assert_eq!(
        next.base_tick,
        Tick(10),
        "the base must survive a withheld delta unchanged"
    );
    assert_eq!(next.created.len(), 1, "a small delta still goes out normally");
    assert!(!next.is_empty());
}

/// The same defect for the updated cap.
#[test]
fn a_delta_over_the_updated_cap_is_withheld() {
    let comp = DeltaCompressor::new();
    let mut base = empty_snapshot(10);
    let mut target = empty_snapshot(11);
    for id in 0..(MAX_DELTA_UPDATED as u64 + 1) {
        // Every entity must actually change, or `compute_entity_update` finds
        // no diff for it and the cap is never reached — the test would then
        // pass for the wrong reason.
        let health = 50.0 - id as f32;
        base.entities.push(test_entity(id, EntityType::Player, 100.0));
        target.entities.push(test_entity(id, EntityType::Player, health));
    }

    let delta = comp.create_delta(2, LAYER_PLAYER, &base, &target);
    assert!(
        delta.is_empty(),
        "an over-cap delta must be withheld, not truncated and sent"
    );
}

/// And for the projectile cap, which is a combined created+updated budget.
#[test]
fn a_delta_over_the_projectile_cap_is_withheld() {
    let comp = DeltaCompressor::new();
    let base = empty_snapshot(10);
    let mut target = empty_snapshot(11);
    for id in 0..(MAX_DELTA_PROJECTILES as u64 + 1) {
        target
            .projectiles
            .push(test_projectile(id, Vec3f::new(id as f32, 0.0, 0.0)));
    }

    let delta = comp.create_delta(3, LAYER_PROJECTILE, &base, &target);
    assert!(
        delta.is_empty(),
        "an over-cap delta must be withheld, not truncated and sent"
    );
}

/// A delta exactly at the cap is legitimate and must still be sent — the
/// withhold must not swallow a full-but-fits delta, or an overloaded server
/// would deadlock into never publishing the layer.
#[test]
fn a_delta_exactly_at_the_cap_is_still_sent() {
    let comp = DeltaCompressor::new();
    let base = empty_snapshot(10);
    let mut target = empty_snapshot(11);
    for id in 0..MAX_DELTA_CREATED as u64 {
        target
            .entities
            .push(test_entity(id, EntityType::Player, 100.0));
    }

    let delta = comp.create_delta(4, LAYER_PLAYER, &base, &target);
    assert_eq!(delta.created.len(), MAX_DELTA_CREATED);
    assert!(!delta.is_empty(), "a delta at the cap must still be sent");
}
