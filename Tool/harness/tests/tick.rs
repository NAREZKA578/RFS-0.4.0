// Side-only tests for the tick hit path (plan §4, bug №40).
use rfs_core::entity::EntityId;
use rfs_core::math::{Bounds, Vec3f};
use rfs_core::packet::ProjectileType;
use rfs_core::time::{Tick, FIXED_DT};
use rfs_damage::damage::DamageSystem;
use rfs_ship::loader::create_default_frigate;
use rfs_ship::ship::{Ship, ShipConfig};
use rfs_sim::ballistics::{BallisticsCalculator, BallisticsConfig};
use rfs_sim::tick::{GameEvent, Projectile, TickSystem, segment_intersects_bounds};
use std::sync::Arc;

fn thin_box() -> Bounds {
    Bounds::new(Vec3f::new(-0.5, -50.0, -50.0), Vec3f::new(0.5, 50.0, 50.0))
}

#[test]
fn swept_hits_thin_target_crossed_between_ticks() {
    // 200 m in one tick through a 1 m plate: a point test tunnels, swept hits.
    assert!(segment_intersects_bounds(
        Vec3f::new(-100.0, 0.0, 0.0),
        Vec3f::new(100.0, 0.0, 0.0),
        &thin_box(),
    ));
}

#[test]
fn swept_misses_parallel_pass() {
    assert!(!segment_intersects_bounds(
        Vec3f::new(-100.0, 60.0, 0.0),
        Vec3f::new(100.0, 60.0, 0.0),
        &thin_box(),
    ));
}

#[test]
fn swept_zero_length_segment() {
    assert!(segment_intersects_bounds(
        Vec3f::new(0.0, 0.0, 0.0),
        Vec3f::new(0.0, 0.0, 0.0),
        &thin_box(),
    ));
    assert!(!segment_intersects_bounds(
        Vec3f::new(5.0, 0.0, 0.0),
        Vec3f::new(5.0, 0.0, 0.0),
        &thin_box(),
    ));
}

fn make_ship() -> Ship {
    let class = create_default_frigate();
    let config: ShipConfig = class.into();
    Ship::new(config, EntityId::new(10), Arc::new(DamageSystem::new()))
}

fn make_shell(x: f32, vx: f32) -> Projectile {
    Projectile {
        entity_id: EntityId::new(5_000_001),
        projectile_type: ProjectileType::Cannonball,
        position: Vec3f::new(x, 0.0, 0.0),
        prev_position: Vec3f::new(x, 0.0, 0.0),
        velocity: Vec3f::new(vx, 0.0, 0.0),
        spawn_tick: Tick(0),
        lifetime: 0.0,
        max_lifetime: 12.0,
        distance_traveled: 0.0,
        owner: EntityId::new(999),
        weapon: EntityId::new(2001),
        damage: 100.0,
        penetration: 50.0,
        explosion_radius: 0.0,
    }
}

fn count_hits(events: &[GameEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, GameEvent::ShipHit { .. }))
        .count()
}

#[test]
fn fast_shell_hits_and_is_spent_no_repeat_damage() {
    let mut sim = TickSystem::new();
    sim.add_ship(make_ship());
    // 400 m per tick straight through the hull: tunnels under point tests.
    sim.fire_projectile(make_shell(-200.0, 12000.0));

    let r1 = sim.tick();
    // §5.1 staging: the hit is detected this tick, damage lands next tick.
    assert_eq!(count_hits(&r1.events), 0, "damage is deferred one tick");
    assert!(
        sim.get_active_projectiles().is_empty(),
        "spent shell must despawn, got {:?}",
        sim.get_active_projectiles().len()
    );

    let r2 = sim.tick();
    assert_eq!(count_hits(&r2.events), 1, "deferred damage arrives next tick");

    let r3 = sim.tick();
    assert_eq!(
        count_hits(&r3.events),
        0,
        "no repeated damage on the following ticks"
    );
}

#[test]
fn expired_projectile_despawns() {
    let mut sim = TickSystem::new();
    sim.add_ship(make_ship());
    let mut shell = make_shell(-200.0, 0.0);
    shell.max_lifetime = FIXED_DT * 0.5;
    sim.fire_projectile(shell);

    sim.tick();
    assert!(sim.get_active_projectiles().is_empty());
}

fn make_free_shell(x: f32, y: f32, vx: f32) -> Projectile {
    let mut shell = make_shell(x, vx);
    shell.position = Vec3f::new(x, y, 0.0);
    shell.prev_position = Vec3f::new(x, y, 0.0);
    shell
}

#[test]
fn ballistic_step_applies_drag_and_gravity() {
    // §4 wiring: the tick flies shells through BallisticsCalculator now.
    let mut sim = TickSystem::new();
    sim.fire_projectile(make_free_shell(0.0, 100.0, 400.0));

    sim.tick();
    let live = sim.get_active_projectiles();
    assert_eq!(live.len(), 1);
    let shell = &live[0];
    assert!(
        shell.velocity.length() < 400.0,
        "drag must bleed speed, got {}",
        shell.velocity.length()
    );
    assert!(shell.position.y < 100.0, "gravity must pull down");
}

#[test]
fn underwater_branch_decelerates_hard() {
    // Below y=0 the water branch (buoyancy + water drag) must engage:
    // air drag at 100 m/s would shave ~0.1, water kills most of it.
    let mut sim = TickSystem::new();
    sim.fire_projectile(make_free_shell(0.0, -5.0, 100.0));

    sim.tick();
    let live = sim.get_active_projectiles();
    assert_eq!(live.len(), 1);
    assert!(
        live[0].velocity.length() < 50.0,
        "water drag must bite, speed={}",
        live[0].velocity.length()
    );
}

#[test]
fn shell_despawns_at_max_range() {
    // Cannonball max_range=5000: ~380 m/tick at 12000 m/s, so ticks 1-5
    // alive, gone by tick 20 while lifetime (12 s) is barely touched.
    let mut sim = TickSystem::new();
    sim.fire_projectile(make_free_shell(0.0, 500.0, 12000.0));

    for _ in 0..5 {
        sim.tick();
    }
    assert_eq!(sim.get_active_projectiles().len(), 1, "still flying at ~1900 m");
    for _ in 0..15 {
        sim.tick();
    }
    assert!(
        sim.get_active_projectiles().is_empty(),
        "must expire by range, not by the 12 s lifetime"
    );
}

#[test]
fn ballistic_arc_rejects_degenerate_inputs() {
    // №45: vertical shots / dead configs return None, never inf/NaN.
    let calc = BallisticsCalculator::new(BallisticsConfig::default());
    assert!(calc
        .solve_ballistic_arc(Vec3f::ZERO, Vec3f::new(0.0, 100.0, 0.0), ProjectileType::Cannonball)
        .is_none());

    let pen = calc.calculate_penetration(
        calc.get_config(ProjectileType::Cannonball).expect("config"),
        Vec3f::ZERO,
        Vec3f::UP,
        50.0,
        0.0,
    );
    assert!(pen.impact_angle.is_finite());
    assert!(!pen.penetrated);
}
