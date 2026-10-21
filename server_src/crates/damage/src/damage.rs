use crate::graph::{BulkheadEdge, CompartmentNode, DamageGraph};
use parking_lot::Mutex;
use rfs_core::entity::EntityId;
use rfs_core::math::{Bounds, Vec3f};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamageConfig {
    pub hull_damage_multiplier: f32,
    pub direct_damage_multiplier: f32,
    pub falloff_distance: f32,
    pub falloff_power: f32,
    pub max_damage_radius: f32,
}

impl Default for DamageConfig {
    fn default() -> Self {
        Self {
            hull_damage_multiplier: 1.0,
            direct_damage_multiplier: 1.0,
            falloff_distance: 3.0,
            falloff_power: 1.5,
            // How far a blast still reaches, measured to the nearest point of a
            // compartment box. This is NOT free to pick: it has to be at least
            // the distance from the shell-collision box of the hull to the
            // compartments inside it, or a shell that strikes the hull lands
            // outside every falloff radius and does nothing at all.
            //
            // For the default frigate that distance is worst-case 111.8 m,
            // measured by `every_part_of_the_default_hull_can_be_damaged` in
            // sim/src/tick.rs, which walks the hull's surface and finds the
            // point furthest from any compartment:
            //   hull collision box  x +-50, y +-30, z +-150   (ship/src/ship.rs)
            //   compartments        x +-12, y -5..+6, z -70..+50
            // At 12.0 the whole hull - all 486 sampled points - was outside the
            // reach, so every shell in the game did nothing.
            //
            // The upper bound comes from a test that must keep passing:
            // `Tool/harness/tests/damage.rs` fires from (100,100,100) at a
            // +-5 m box, which is 164.5 m away, and requires zero damage.
            //
            // So the usable window is roughly 112..164 m. 140 sits inside it.
            // The hull test fails if either side of this coupling moves.
            max_damage_radius: 140.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentHit {
    pub entity_id: EntityId,
    pub name: String,
    pub bounds: Bounds,
    pub max_water_level: f32,
    pub pump_capacity: f32,
}

/// A damageable station inside a compartment (Bug №194).
///
/// Stations are deliberately not `CompartmentHit`: they hold no water and have
/// no bulkhead edges, so feeding them to the flooding graph would invent
/// capacity and edges for something that is only a gun mount.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationHit {
    pub entity_id: EntityId,
    pub name: String,
    pub bounds: Bounds,
}

/// What one blast did to a ship (Bug №607).
///
/// `breached` is what the caller needs to mirror a fresh breach into the ship's
/// own state (Bug №151). `inflicted` is new: every compartment that absorbed a
/// non-zero share of the blast, with the share dealt. An explosive hit uses it
/// to light the compartment the blast actually reached — the hull collision box
/// is far larger than any compartment, so the shell's impact point alone names
/// no compartment at all.
#[derive(Debug, Default, Clone)]
pub struct DamageReport {
    pub breached: Vec<EntityId>,
    pub inflicted: Vec<(EntityId, f32)>,
}

pub trait DamageTarget {
    fn entity_id(&self) -> EntityId;
    fn compartment_hits(&self) -> Vec<CompartmentHit>;
    fn apply_health_damage(&self, amount: f32);
    fn apply_compartment_damage(&self, compartment_id: EntityId, damage: f32, position: Vec3f);

    /// Stations that a blast can reach.
    ///
    /// Bug №194: `Station::apply_damage` had no caller anywhere in the server,
    /// so a station's `is_operational = false` and `occupant = None` branches
    /// were unreachable, the early exit in `Station::update` was dead, and
    /// guns could never be destroyed. Defaulted so existing targets keep
    /// compiling.
    fn station_hits(&self) -> Vec<StationHit> {
        Vec::new()
    }

    /// Applies blast damage to one station. Defaulted to a no-op for the same
    /// reason.
    fn apply_station_damage(&self, _station_id: EntityId, _damage: f32, _position: Vec3f) {}
}

pub struct DamageSystem {
    config: DamageConfig,
    graph: Mutex<DamageGraph>,
}

impl DamageSystem {
    pub fn new() -> Self {
        Self {
            config: DamageConfig::default(),
            graph: Mutex::new(DamageGraph::new()),
        }
    }

    pub fn with_config(config: DamageConfig) -> Self {
        Self {
            config,
            graph: Mutex::new(DamageGraph::new()),
        }
    }

    pub fn config(&self) -> &DamageConfig {
        &self.config
    }

    /// Apply damage and report which compartments breached for the first time
    /// (Bug №151: the caller mirrors the breach into the ship's own state so
    /// battle damage actually floods it) and every compartment that took a
    /// share of the blast (Bug №607: an explosive hit lights the compartment it
    /// reached).
    pub fn apply_damage_to_ship(
        &self,
        target: &impl DamageTarget,
        position: Vec3f,
        damage: f32,
    ) -> DamageReport {
        // Bug №65: NaN/negative "damage" must never reach the hull or the
        // compartments — a negative blast used to heal the ship.
        // Spelled out rather than `!(damage > 0.0)`: the NaN case has to stay
        // rejected, and `<=` alone would let NaN through.
        if damage.is_nan() || damage <= 0.0 {
            return DamageReport::default();
        }

        let hits = target.compartment_hits();
        let mut inflicted = Vec::with_capacity(hits.len());

        // Bug №65: the falloff distance is measured to the NEAREST point of
        // each compartment box, not its center — an edge hit of a large box
        // used to under-damage that box and over-damage its neighbours.
        for hit in &hits {
            let distance = distance_to_bounds(position, &hit.bounds);
            let factor = self.falloff(distance);
            let damage_dealt = damage * factor * self.config.direct_damage_multiplier;

            if damage_dealt <= 0.0 {
                continue;
            }

            target.apply_compartment_damage(hit.entity_id, damage_dealt, position);
            inflicted.push((hit.entity_id, damage_dealt));
        }

        let mut newly_breached: Vec<EntityId> = Vec::new();
        {
            let mut graph = self.graph.lock();
            for (entity_id, amount) in &inflicted {
                if graph.get_compartment(*entity_id).is_none() {
                    if let Some(hit) = hits.iter().find(|h| h.entity_id == *entity_id) {
                        graph.add_compartment(CompartmentNode::new(
                            hit.entity_id,
                            hit.name.clone(),
                            hit.max_water_level,
                            hit.pump_capacity,
                        ));

                        // Bug №65: a node created here used to arrive with ZERO
                        // edges, so water flooding it could never flow anywhere.
                        // Best-effort wire it to every ship box it touches so the
                        // replay/spoofed path still propagates.
                        for other in &hits {
                            if other.entity_id == hit.entity_id {
                                continue;
                            }
                            let touching = hit.bounds.expand(2.0).intersects(&other.bounds);
                            if !touching {
                                continue;
                            }
                            if graph
                                .get_bulkhead_between(hit.entity_id, other.entity_id)
                                .is_some()
                            {
                                continue;
                            }
                            // Synthetic deterministic id outside ship regions.
                            // Guard: hash may collide with a real compartment or
                            // bulkhead id — never reuse such an id.
                            let edge_id = EntityId::new(
                                hit.entity_id.0.wrapping_mul(0x9E37_79B9)
                                    ^ other.entity_id.0.wrapping_add(0x517C_C1B7),
                            );
                            if graph.get_bulkhead(edge_id).is_none()
                                && graph.get_compartment(edge_id).is_none()
                            {
                                // Bug №241: this used to be `BulkheadEdge::new`,
                                // which sets `is_sealed: true`, and
                                // `is_passable()` is `!is_sealed || is_destroyed`
                                // — so every synthetic edge was permanently
                                // impassable and the water never flowed, which
                                // is the exact opposite of what the comment
                                // above this block claims. A synthetic edge
                                // exists to provide a route, so it is built
                                // open.
                                graph.add_bulkhead(BulkheadEdge::open(
                                    edge_id,
                                    hit.entity_id,
                                    other.entity_id,
                                    100.0,
                                ));
                            }
                        }
                    }
                }
                let was_breached = graph
                    .get_compartment(*entity_id)
                    .map(|c| c.is_breached)
                    .unwrap_or(false);
                graph.apply_damage(*entity_id, *amount);
                // Bug №151: only the edge TRUE is reported — an already-sunk
                // compartment must not re-fire the breach.
                if !was_breached
                    && graph
                        .get_compartment(*entity_id)
                        .map(|c| c.is_breached)
                        .unwrap_or(false)
                {
                    newly_breached.push(*entity_id);
                }
            }
        }

        // Bug №194: stations used to be invisible to the blast. They are kept
        // out of the flooding graph on purpose — no water capacity, no edges —
        // so they get their own pass with the same falloff curve. Without this
        // a station could never be destroyed and its gun fired forever.
        for station in target.station_hits() {
            let distance = distance_to_bounds(position, &station.bounds);
            let damage_dealt =
                damage * self.falloff(distance) * self.config.direct_damage_multiplier;
            if damage_dealt <= 0.0 {
                continue;
            }
            target.apply_station_damage(station.entity_id, damage_dealt, position);
        }

        // Bug №65: the hull took the FULL blast regardless of falloff — a
        // far-edge hit dealt 0 to every compartment and 100 to the hull.
        // Hull damage now scales with the best factor seen by any compartment.
        let mut max_factor: f32 = 0.0;
        for hit in &hits {
            let factor = self.falloff(distance_to_bounds(position, &hit.bounds));
            max_factor = max_factor.max(factor);
        }
        let hull_factor = if hits.is_empty() { 1.0 } else { max_factor };
        target.apply_health_damage(damage * self.config.hull_damage_multiplier * hull_factor);
        DamageReport {
            breached: newly_breached,
            inflicted,
        }
    }

    /// Register the ship's compartment/bulkhead topology so the production
    /// graph has real edges (Bug №65's lazy nodes) and water can flow.
    pub fn register_topology(
        &self,
        nodes: &[(EntityId, String, f32, f32)],
        edges: &[(EntityId, EntityId, EntityId, f32)],
    ) {
        let mut graph = self.graph.lock();
        for (id, name, capacity, pump) in nodes {
            if let Some(node) = graph.get_compartment_mut(*id) {
                // Bug №64: registering a duplicate used to add an orphan node.
                node.max_capacity = *capacity;
                node.pump_capacity = *pump;
            } else {
                graph.add_compartment(CompartmentNode::new(*id, name.clone(), *capacity, *pump));
            }
        }
        for (id, a, b, seal_strength) in edges {
            if graph.get_bulkhead(*id).is_none() {
                graph.add_bulkhead(BulkheadEdge::new(*id, *a, *b, *seal_strength));
            }
        }
    }

    fn falloff(&self, distance: f32) -> f32 {
        // Validated falloff: non-finite/inverted configs must not invert damage
        // (negative power = farther hurts more) or NaN-poison.
        if !distance.is_finite() || distance < 0.0 {
            return 1.0;
        }
        let falloff =
            if self.config.falloff_distance.is_finite() && self.config.falloff_distance >= 0.0 {
                self.config.falloff_distance
            } else {
                0.0
            };
        let max_r = if self.config.max_damage_radius.is_finite()
            && self.config.max_damage_radius > falloff
        {
            self.config.max_damage_radius
        } else {
            falloff + 1.0
        };
        if distance <= falloff {
            1.0
        } else if distance >= max_r {
            0.0
        } else {
            let range = (max_r - falloff).max(0.01);
            let t = (1.0 - (distance - falloff) / range).clamp(0.0, 1.0);
            let power = if self.config.falloff_power.is_finite() && self.config.falloff_power >= 0.0
            {
                self.config.falloff_power
            } else {
                1.0
            };
            t.powf(power).clamp(0.0, 1.0)
        }
    }
}

/// Distance from a point to the nearest point of an axis-aligned box
/// (Bug №65). If the point is inside the box the distance is 0.
fn distance_to_bounds(point: Vec3f, bounds: &Bounds) -> f32 {
    // Bug №199: an inverted axis used to clamp to nothing and so contribute a
    // distance of zero, which made the blast distance collapse to 0 and
    // `falloff(0.0)` to 1.0 — full damage from any point on the ship.
    //
    // `Bounds::new` orders the axes for every construction site, but `Bounds`
    // is `Serialize`/`Deserialize` with public fields, so a box can still
    // arrive from the wire. Re-order here as well: the sink does not assume
    // the source normalized.
    let bounds = bounds.normalized();
    // Bug №65: inverted bounds must not panic. Lives in `finite` so every call
    // site shares one implementation.
    use crate::finite::safe_clamp;
    let clamped = Vec3f::new(
        safe_clamp(point.x, bounds.min.x, bounds.max.x),
        safe_clamp(point.y, bounds.min.y, bounds.max.y),
        safe_clamp(point.z, bounds.min.z, bounds.max.z),
    );
    clamped.distance(point)
}

impl Default for DamageSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records what the blast actually did, so the assertions are made on dealt
    /// damage rather than on the falloff helper in isolation.
    struct RecordingShip {
        hull_damage: RefCell<f32>,
        compartment_damage: RefCell<Vec<(EntityId, f32)>>,
        bounds: Bounds,
    }

    impl RecordingShip {
        fn new(bounds: Bounds) -> Self {
            Self {
                hull_damage: RefCell::new(0.0),
                compartment_damage: RefCell::new(Vec::new()),
                bounds,
            }
        }

        fn hull(&self) -> f32 {
            *self.hull_damage.borrow()
        }

        fn compartment_total(&self) -> f32 {
            self.compartment_damage
                .borrow()
                .iter()
                .map(|(_, d)| *d)
                .sum()
        }
    }

    impl DamageTarget for RecordingShip {
        fn entity_id(&self) -> EntityId {
            EntityId::new(1)
        }

        fn compartment_hits(&self) -> Vec<CompartmentHit> {
            vec![CompartmentHit {
                entity_id: EntityId::new(10),
                name: "engine room".into(),
                bounds: self.bounds,
                max_water_level: 100.0,
                pump_capacity: 5.0,
            }]
        }

        fn apply_health_damage(&self, amount: f32) {
            *self.hull_damage.borrow_mut() += amount;
        }

        fn apply_compartment_damage(&self, compartment_id: EntityId, damage: f32, _pos: Vec3f) {
            // The trait takes `&self`, so the log sits behind a RefCell.
            self.compartment_damage
                .borrow_mut()
                .push((compartment_id, damage));
        }
    }

    /// A compartment box spanning x/y 0..10 and sitting at z 0..2.
    fn compartment_bounds() -> Bounds {
        Bounds::new(Vec3f::new(0.0, 0.0, 0.0), Vec3f::new(10.0, 10.0, 2.0))
    }

    /// The blast position that actually exposes Bug №199: it lies **inside** the
    /// compartment on x and y but far away on z. With z inverted the clamp on z
    /// does nothing, so z contributes a distance of zero, x and y contribute
    /// zero, and the measured distance collapses to exactly 0 — which
    /// `falloff(0.0)` scores as 1.0, full damage.
    ///
    /// A blast far away on *all* axes does not expose it: the two sound axes
    /// then supply a large real distance on their own. That is why the obvious
    /// "distant blast" version of this test passed even with the bug present.
    fn blast_along_the_inverted_axis() -> Vec3f {
        Vec3f::new(5.0, 5.0, 500.0)
    }

    /// A box with z corners swapped, built as a struct literal so `Bounds::new`
    /// does not silently repair it — this stands in for data that arrived
    /// malformed from the wire or from a direct field write.
    fn inverted_bounds() -> Bounds {
        let b = Bounds {
            min: Vec3f::new(0.0, 0.0, 2.0),
            max: Vec3f::new(10.0, 10.0, 0.0),
        };
        assert!(b.is_inverted(), "the fixture is not actually inverted");
        b
    }

    /// Bug №199: an inverted axis collapsed the measured distance to 0, so a
    /// blast from 500 units away dealt full hull damage. Asserted end to end
    /// through the real `DamageTarget`, not on the helper alone.
    #[test]
    fn an_inverted_compartment_axis_does_not_make_every_blast_full_damage() {
        let sys = DamageSystem::new();

        // The well-formed box: this blast is far outside it, scaled away to none.
        let good = RecordingShip::new(compartment_bounds());
        sys.apply_damage_to_ship(&good, blast_along_the_inverted_axis(), 100.0);
        let good_hull = good.hull();
        assert_eq!(
            good_hull, 0.0,
            "a far blast reached a sound compartment box"
        );

        // The malformed box must behave identically.
        let bad = RecordingShip::new(inverted_bounds());
        sys.apply_damage_to_ship(&bad, blast_along_the_inverted_axis(), 100.0);
        assert_eq!(
            bad.hull(),
            good_hull,
            "an inverted axis turned a far blast into full hull damage: {}",
            bad.hull()
        );
        assert_eq!(
            bad.compartment_total(),
            0.0,
            "an inverted axis dealt compartment damage from 500 units away"
        );
    }

    /// The complementary case: a blast genuinely inside the compartment must
    /// still deal full damage. Guards against over-correcting into "malformed
    /// boxes are simply immune".
    #[test]
    fn a_blast_inside_the_compartment_still_hits_full() {
        let sys = DamageSystem::new();
        let ship = RecordingShip::new(inverted_bounds());

        sys.apply_damage_to_ship(&ship, Vec3f::new(5.0, 5.0, 1.0), 100.0);

        assert!(
            ship.hull() > 99.0,
            "a blast at the compartment centre dealt only {}",
            ship.hull()
        );
    }

    /// A well-formed box is untouched by the normalization: the fix must not
    /// quietly change the distance a sound compartment reports.
    #[test]
    fn a_well_formed_compartment_box_is_unaffected() {
        let bounds = compartment_bounds();
        assert!(!bounds.is_inverted());
        assert_eq!(
            distance_to_bounds(Vec3f::new(5.0, 5.0, 1.0), &bounds),
            0.0,
            "a point inside the box is not at distance 0"
        );
        let d = distance_to_bounds(Vec3f::new(13.0, 5.0, 1.0), &bounds);
        assert!((d - 3.0).abs() < 1e-4, "expected 3.0, got {d}");
    }
}
