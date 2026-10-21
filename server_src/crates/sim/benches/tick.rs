//! Cost of one simulation tick as the fleet grows.
//!
//! Analysis §7 calls these the first benchmarks worth having: "без бенчмарка
//! разговор о 3.4.3 и 3.4.4 остаётся оценкой". Three sizes, because a cost that
//! is linear at 1 ship and quadratic at 64 is invisible at either end alone.
//!
//! The world is loaded, not empty. `TickSystem` starts with `None`, and with no
//! map the boundary pass returns before it touches anything — so a benchmark
//! without one would measure the parts that were never the question.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use rfs_ship::loader::create_default_frigate;
use rfs_ship::ship::{Ship, ShipConfig};
use std::sync::Arc;

use rfs_damage::DamageSystem;
use rfs_sim::map::WorldMap;
use rfs_sim::tick::{Projectile, TickSystem};

const SPACING: f32 = 3000.0;

/// A world with obstacles in it.
///
/// The default world has none, and that matters here: `hold_ships_in_bounds` and
/// `check_collisions` used to `clone()` the whole map every tick (Bug №611). With
/// zero obstacles that clone is nearly free, so a benchmark on the default map
/// measures the defect as invisible. The obstacles are the payload.
fn bench_world(ships: usize, obstacles: usize) -> WorldMap {
    let radius = (ships as f32 * SPACING * 0.5 + 5_000.0).max(25_000.0);
    WorldMap::procedural("bench-open-sea", 1, radius, obstacles)
        .expect("the benchmark world failed its own validation")
}

fn sim_with(ships: usize) -> TickSystem {
    sim_with_world(ships, 0)
}

fn sim_with_world(ships: usize, obstacles: usize) -> TickSystem {
    let sim = TickSystem::new();
    let world = bench_world(ships, obstacles);
    sim.set_world(world)
        .expect("the benchmark world was refused");
    sim
}

fn add_fleet(sim: &TickSystem, ships: usize) {
    for i in 0..ships {
        let config: ShipConfig = create_default_frigate().into();
        let ship = Ship::new(
            config,
            rfs_core::entity::EntityId::new(10 + i as u64 * 10_000),
            Arc::new(DamageSystem::new()),
        );
        let mut state = ship.get_state();
        state.transform.position = rfs_core::math::Vec3f::new(i as f32 * SPACING, 0.0, 0.0);
        ship.apply_state(state);
        sim.add_ship(ship);
    }
}

/// Shells in flight, so the projectile passes is actually doing work.
///
/// Every tick used to `projectiles.read().clone()` the whole list twice (Bug
/// №611). With an empty list those clones are two allocations of nothing, and a
/// benchmark that measures them measures an accident of the fixture.
fn add_shells(sim: &TickSystem, count: u64) {
    for i in 0..count {
        let shell = Projectile {
            entity_id: rfs_core::entity::EntityId::new(5_000_000 + i),
            projectile_type: rfs_core::packet::ProjectileType::Cannonball,
            position: rfs_core::math::Vec3f::new((i as f32) * 7.0 - 400.0, 40.0, (i as f32) * 3.0),
            prev_position: rfs_core::math::Vec3f::new(
                (i as f32) * 7.0 - 410.0,
                40.0,
                (i as f32) * 3.0,
            ),
            velocity: rfs_core::math::Vec3f::new(400.0, 0.0, 0.0),
            spawn_tick: rfs_core::time::Tick(0),
            lifetime: 0.0,
            max_lifetime: 600.0,
            distance_traveled: 0.0,
            owner: rfs_core::entity::EntityId::new(999),
            weapon: rfs_core::entity::EntityId::new(2001),
            damage: 100.0,
            penetration: 50.0,
            explosion_radius: 0.0,
            ricochet_count: 0,
        };
        sim.fire_projectile(shell)
            .expect("the benchmark shell was accepted");
    }
}

/// The plain fleet sweep the plan asks for: 1, 16, 64 ships.
fn tick_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("tick");
    for ships in [1usize, 16, 64] {
        group.throughput(criterion::Throughput::Elements(ships as u64));
        group.bench_function(format!("{ships}_ships"), |b| {
            b.iter_batched(
                || {
                    let sim = sim_with(ships);
                    add_fleet(&sim, ships);
                    sim
                },
                |mut s| s.tick(),
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

/// The sweep that can actually see Bug №611: a world full of obstacles and a
/// sky full of shells, which are the two things the tick used to clone per tick.
fn loaded_tick_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("tick_loaded");
    for (ships, obstacles, shells) in [(16usize, 256usize, 256u64), (64, 256, 256)] {
        group.throughput(criterion::Throughput::Elements(ships as u64));
        group.bench_function(
            format!("{ships}_ships_{obstacles}_rocks_{shells}_shells"),
            |b| {
                b.iter_batched(
                    || {
                        let sim = sim_with_world(ships, obstacles);
                        add_fleet(&sim, ships);
                        add_shells(&sim, shells);
                        sim
                    },
                    |mut s| s.tick(),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

criterion_group!(benches, tick_benchmark, loaded_tick_benchmark);
criterion_main!(benches);
