//! Cost of producing one layer's delta, as the fleet behind it grows.
//!
//! Analysis §7 asks for `build_snapshot` at 1/50/256 clients. The server's
//! per-client, per-layer hot path is `DeltaCompressor::create_delta`: it diffs a
//! base against a target and, on the first pass for a client, has no ring entry
//! at all and diffs against the whole base.
//!
//! The fleet size is the variable, not the client count, because a delta's cost
//! is set by how many entities differ between the two ticks. One client looking
//! at 64 ships is the expensive case; 64 clients looking at one ship each is the
//! cheap one, and the pair is what says whether the cost is per-client or
//! per-entity.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use rfs_core::entity::{EntityId, EntityType};
use rfs_core::math::{Quatf, Transform, Vec3f};
use rfs_core::time::Tick;
use rfs_net::snapshot::{DeltaCompressor, EntitySnapshot, Snapshot};

fn entities(count: usize, tick: Tick) -> Vec<EntitySnapshot> {
    (0..count)
        .map(|i| {
            // The transform carries the tick, so successive snapshots differ and
            // the diff has real work to do. An identical pair would measure the
            // early return instead.
            let drift = tick.0 as f32 * 0.01 * i as f32;
            EntitySnapshot {
                entity_id: EntityId::new(i as u64 + 1),
                entity_type: EntityType::Ship,
                transform: Transform::new(
                    Vec3f::new(i as f32 * 10.0 + drift, 0.0, 0.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                velocity: Vec3f::new(1.0, 0.0, 0.0),
                angular_velocity: Vec3f::ZERO,
                health: 1000.0,
                max_health: 1000.0,
                flags: Default::default(),
                ship_data: None,
                station_data: None,
                player_data: None,
            }
        })
        .collect()
}

fn snapshot(tick: u64) -> Snapshot {
    Snapshot {
        tick: Tick(tick),
        time: tick as f64 / 30.0,
        entities: entities(64, Tick(tick)),
        projectiles: Vec::new(),
        events: Vec::new(),
    }
}

fn delta_benchmark(c: &mut Criterion) {
    let base = snapshot(1);
    let target = snapshot(2);

    let mut group = c.benchmark_group("create_delta");
    // One client, growing fleet: the cost that §3.4.3 asks about.
    for entities in [1usize, 16, 64] {
        let mut base = base.clone();
        let mut target = target.clone();
        base.entities.truncate(entities);
        target.entities.truncate(entities);
        group.throughput(criterion::Throughput::Elements(entities as u64));
        group.bench_function(format!("1_client_{entities}_entities"), |b| {
            b.iter_batched(
                DeltaCompressor::new,
                |c| c.create_delta(1, 0, &base, &target),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();

    // Growing client count over a fixed fleet: whether anything here is per-client.
    let mut group = c.benchmark_group("create_delta_clients");
    for clients in [1u32, 50, 256] {
        group.throughput(criterion::Throughput::Elements(clients as u64));
        group.bench_function(format!("{clients}_clients_64_entities"), |b| {
            b.iter_batched(
                DeltaCompressor::new,
                |c| {
                    for client in 0..clients {
                        let _ = c.create_delta(client, 0, &base, &target);
                    }
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, delta_benchmark);
criterion_main!(benches);