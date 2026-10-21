// Bug №272: a hit must survive a lost `ShipHit` EVENT.
//
// Variant A of that bug made events unreliable, because an event carries no
// state and the client's world lives in the layer deltas. Everything an event
// reported was already in a layer — health, flooding, station occupancy,
// projectile position — except the impact normal and the struck compartment.
// Those moved into layer 0 as `ShipSnapshotData::last_hit`.
//
// These tests pin the three properties that make that safe. If the field is
// dropped anywhere between the simulation and the layer, or if the delta path
// does not carry it, the hit effect silently disappears again — which is
// exactly the failure mode the event used to paper over.
use rfs_core::entity::{EntityId, HitMark, ShipEntity};
use rfs_core::math::{Transform, Vec3f};
use rfs_core::packet::{EntityFlags, EntityType, StateDeltaPacket};
use rfs_core::time::Tick;
use rfs_net::{DeltaCompressor, EntitySnapshot, Snapshot, LAYER_SHIP};

fn ship_with_hit(hit: Option<HitMark>) -> ShipEntity {
    ShipEntity {
        entity_id: EntityId::new(10),
        ship_class_id: 1,
        transform: Transform::IDENTITY,
        velocity: Vec3f::ZERO,
        angular_velocity: Vec3f::ZERO,
        health: 9000.0,
        max_health: 10000.0,
        flags: EntityFlags::NONE,
        fuel: 500.0,
        max_fuel: 10000.0,
        speed: 0.0,
        max_speed: 30.0,
        heading: 0.0,
        rudder_angle: 0.0,
        throttle: 0.0,
        compartments: vec![],
        stations: vec![],
        team: 0,
        last_hit: hit,
    }
}

fn hit_at(tick: u32) -> HitMark {
    HitMark {
        hit_tick: tick,
        position: Vec3f::new(12.0, 3.0, -4.0),
        normal: Vec3f::new(0.0, 0.0, 1.0),
        compartment: Some(EntityId::new(77)),
        damage: 42.5,
    }
}

fn snapshot_of(tick: u64, ship: Option<ShipEntity>) -> Snapshot {
    Snapshot {
        tick: Tick(tick),
        time: tick as f64 / 30.0,
        entities: ship.map(|s| EntitySnapshot::from(&s)).into_iter().collect(),
        projectiles: Vec::new(),
        events: Vec::new(),
    }
}

/// The hit mark must be part of the layer-0 entity, not just of an event.
#[test]
fn a_hit_becomes_part_of_the_ship_layer_entity() {
    let mark = hit_at(5);
    let entity = EntitySnapshot::from(&ship_with_hit(Some(mark)));
    let data = entity.ship_data.as_ref().expect("layer 0 carries ship_data");
    let carried = data.last_hit.expect("the hit must be in the ship layer");
    assert_eq!(carried, mark);
    assert_eq!(carried.compartment, Some(EntityId::new(77)));
    assert_eq!(carried.normal, Vec3f::new(0.0, 0.0, 1.0));
}

/// The real test: a client that receives the layer but never the event must end
/// up with the hit. This is the exact loss variant A introduces, so it is what
/// the field exists to prevent.
#[test]
fn a_client_recovers_the_hit_from_the_layer_alone() {
    let base = snapshot_of(1, Some(ship_with_hit(None)));
    let target = snapshot_of(2, Some(ship_with_hit(Some(hit_at(2)))));

    let comp = DeltaCompressor::new();
    let delta = comp.create_delta(1, LAYER_SHIP, &base, &target);
    assert!(!delta.is_empty(), "the hit must produce a layer delta");

    // Applied as a wire packet, exactly as the client would.
    let wire = StateDeltaPacket {
        layer: LAYER_SHIP,
        base_tick: 1,
        server_tick: 2,
        server_time: 2.0 / 30.0,
        is_resync: false,
        created: delta.created.iter().map(|e| e.to_state()).collect(),
        updated: delta.updated.iter().map(|u| u.to_state_update()).collect(),
        destroyed: delta.destroyed.clone(),
        projectile_created: Vec::new(),
        projectile_updated: Vec::new(),
        projectile_destroyed: Vec::new(),
    };

    let after = base.apply_delta(&wire);
    let recovered = after
        .entities
        .iter()
        .find(|e| e.entity_type == EntityType::Ship)
        .and_then(|e| e.ship_data.as_ref())
        .and_then(|d| d.last_hit)
        .expect("the hit must survive the layer round trip with no event in sight");

    assert_eq!(recovered, hit_at(2));
}

/// Two shells in the same tick: the layer keeps the latest, so the effect is
/// never stale in the other direction.
#[test]
fn a_later_hit_replaces_an_earlier_one_in_the_layer() {
    let comp = DeltaCompressor::new();
    let base = snapshot_of(1, Some(ship_with_hit(Some(hit_at(1)))));
    let target = snapshot_of(2, Some(ship_with_hit(Some(hit_at(2)))));

    let delta = comp.create_delta(1, LAYER_SHIP, &base, &target);
    let wire = StateDeltaPacket {
        layer: LAYER_SHIP,
        base_tick: 1,
        server_tick: 2,
        server_time: 0.0,
        is_resync: false,
        created: delta.created.iter().map(|e| e.to_state()).collect(),
        updated: delta.updated.iter().map(|u| u.to_state_update()).collect(),
        destroyed: delta.destroyed.clone(),
        projectile_created: Vec::new(),
        projectile_updated: Vec::new(),
        projectile_destroyed: Vec::new(),
    };
    let after = base.apply_delta(&wire);
    let carried = after.entities[0]
        .ship_data
        .as_ref()
        .and_then(|d| d.last_hit)
        .expect("hit still present");
    assert_eq!(
        carried.hit_tick, 2,
        "the newest hit must win, or the client draws a stale impact"
    );
}

/// No hit means no mark: a ship that was never hit must not claim otherwise.
#[test]
fn an_undamaged_ship_carries_no_hit() {
    let entity = EntitySnapshot::from(&ship_with_hit(None));
    let data = entity.ship_data.as_ref().expect("ship_data present");
    assert!(data.last_hit.is_none());
}

/// A resync is a full layer definition built from the ship's CURRENT state, so
/// it carries whatever the ship holds right now — including the hit mark. That
/// is correct and it is what the test originally got wrong: I expected the
/// mark to be dropped on resync, on the theory that an impact effect is not
/// state a client needs after a gap. But the mark is part of the snapshot, not
/// an event, so "the layer says there is a hit here" is simply true, and
/// suppressing it would make a resynced client disagree with every other
/// client about the same ship. Pinned to keep that deliberate.
#[test]
fn a_resync_carries_the_current_hit_and_health() {
    let base = snapshot_of(1, Some(ship_with_hit(Some(hit_at(1)))));
    let target = snapshot_of(9, Some(ship_with_hit(Some(hit_at(9)))));

    let comp = DeltaCompressor::new();
    let empty_base = Snapshot {
        tick: Tick(1),
        time: 0.0,
        entities: Vec::new(),
        projectiles: Vec::new(),
        events: Vec::new(),
    };
    let delta = comp.create_delta(1, LAYER_SHIP, &empty_base, &target);
    let wire = StateDeltaPacket {
        layer: LAYER_SHIP,
        base_tick: 1,
        server_tick: 9,
        server_time: 0.0,
        is_resync: true,
        created: delta.created.iter().map(|e| e.to_state()).collect(),
        updated: Vec::new(),
        destroyed: Vec::new(),
        projectile_created: Vec::new(),
        projectile_updated: Vec::new(),
        projectile_destroyed: Vec::new(),
    };

    let after = base.apply_delta(&wire);
    let data = after.entities[0].ship_data.as_ref().expect("ship_data");
    assert_eq!(
        data.last_hit,
        Some(hit_at(9)),
        "a resynced client must agree with every other client about the same ship"
    );
    assert_eq!(
        after.entities[0].health, 9000.0,
        "health must still come through a resync — that is the state that matters"
    );
}