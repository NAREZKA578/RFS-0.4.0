//! The world map: the volume ships fly in and the solid things inside it.
//!
//! # Why this exists
//!
//! The simulation had no world. `check_collisions` compared projectiles against
//! ships and nothing else, `CollisionSystem` was never called from anywhere, and
//! a ship that pointed at `+X` left the universe and kept its velocity. The
//! server worked because it placed every ship on a line with nothing around
//! them, which is a scenario, not a world.
//!
//! # Scope, deliberately
//!
//! Obstacles are axis-aligned boxes. That is not a simplification to save work:
//! the tick already does swept point-vs-AABB (`segment_intersects_bounds`) for
//! ships, and a sphere would need its own swept test plus its own regression
//! tests before it could be trusted. Boxes are what the code can already prove
//! correct.
//!
//! Rotation is deliberately absent for the same reason. A rotated box needs an
//! inverse transform per candidate, which turns the per-tick cost from "iterate
//! the obstacles near this shell" into a broadphase problem.
//!
//! # Loading
//!
//! Two paths, and both are validated:
//!
//!  - [`WorldMap::from_json`] for a real map, and
//!  - [`WorldMap::procedural`] for development and tests, from a seed.
//!
//! Validation rejects rather than repairs. A map whose rocks stick out through
//! its own boundary is broken authoring data; quietly clamping it hides the
//! mistake until someone wonders why the asteroid is the wrong shape.

use rfs_core::entity::EntityId;
use rfs_core::math::{Bounds, Vec3f};
use serde::{Deserialize, Serialize};

/// Largest number of obstacles a single map may declare.
///
/// Not a performance limit — the tick walks every obstacle per projectile. It is
/// a sanity bound: a JSON file claiming two million rocks is a bug or an attack,
/// and either way it should not be turned into a `Vec` allocation on the server's
/// startup path.
pub const MAX_OBSTACLES: usize = 65_536;

/// What a solid thing is, for replication and for log lines.
///
/// The kind is carried on the collision event and reaches the caller as
/// `CollisionTarget::Obstacle`. **Nothing branches on it yet**: a hit spends the
/// shell and reports the impact, and that is the whole of the behaviour today.
/// Damage to the world is not modelled — see the registry entry — because a
/// destructible rock needs health that outlives a tick, plus a way to tell a
/// client the rock it knows about is gone. Both are design decisions, not
/// details, so the promise "takes damage like anything else" is deliberately not
/// written down here.
/// What a solid thing is.
///
/// RFS is a naval game on **open sea without land**, so there is no drifting
/// rock here and there never should be: the first version of this enum had an
/// `Asteroid` arm, invented because a world generator that emits nothing looked
/// unfinished. It was green, tested and reachable, and wrong - a second class of
/// defect next to "written but not wired": written, wired, and not part of the
/// project. See `.DOC/AGENT_CHAT.md`, 2026-10-14.
///
/// Kept as an enum because the two roles are genuinely different to a client:
/// `Structure` is what the design note means by "твёрдые препятствия создают
/// радиолокационную тень", and it is the only kind an open-sea map is ever
/// expected to contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ObstacleKind {
    /// Built structure. The only kind an open-sea map is expected to contain.
    #[default]
    Structure,
}

/// One solid volume in the world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MapObstacle {
    /// Stable identity, so a client can be told about the same rock twice.
    pub id: u64,
    pub bounds: Bounds,
    #[serde(default)]
    pub kind: ObstacleKind,
}

impl MapObstacle {
    /// How far a point is inside the box, as a 0..=1 fraction of the box size on
    /// the shallowest axis.
    ///
    /// Used to scale impact damage: a shell that clips the edge of a rock does
    /// less than one that buries itself in the middle. Returns 1.0 for a
    /// degenerate box, which has no interior to be shallow in.
    pub fn penetration_depth(&self, point: Vec3f) -> f32 {
        let size = components(self.bounds.size());
        let centre = components(self.bounds.center());
        let p = components(point);
        let mut depth: Option<f32> = None;
        for axis in 0..3 {
            let extent = size[axis];
            // `!(x > 0.0)` rather than `x <= 0.0`: the comparison is here to
            // reject NaN, and clippy's suggestion would hide that behind a
            // `partial_cmp` returning `None`.
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if !(extent > 0.0) {
                return 1.0;
            }
            let offset = (p[axis] - centre[axis]).abs() / (extent * 0.5);
            let along = (1.0f32 - offset).max(0.0);
            depth = Some(match depth {
                Some(current) => current.min(along),
                None => along,
            });
        }
        depth.unwrap_or(1.0)
    }
}

/// The world: a volume to stay inside, and what is inside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldMap {
    pub name: String,
    /// The playable volume. Ships are held inside it.
    pub bounds: Bounds,
    #[serde(default)]
    pub obstacles: Vec<MapObstacle>,
}

/// Why a map was refused. Every variant names the field, so the message points
/// at the line in the JSON that has to change.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MapError {
    #[error("map `{0}` is not valid JSON: {1}")]
    Malformed(String, String),
    #[error("map bounds are not finite (min={min:?}, max={max:?})")]
    NonFiniteBounds { min: [f32; 3], max: [f32; 3] },
    #[error("map bounds are inverted on at least one axis (min={min:?}, max={max:?})")]
    InvertedBounds { min: [f32; 3], max: [f32; 3] },
    #[error("map declares {count} obstacles, the limit is {MAX_OBSTACLES}")]
    TooManyObstacles { count: usize },
    #[error("map radius {radius:?} is not a positive finite number")]
    InvalidRadius { radius: f32 },
    #[error("obstacle {id} is not finite")]
    NonFiniteObstacle { id: u64 },
    #[error("obstacle {id} has an inverted box (min={min:?}, max={max:?})")]
    InvertedObstacle { id: u64, min: [f32; 3], max: [f32; 3] },
    #[error("obstacle {id} sticks out of the map (it spans {min:?}..{max:?}, map is {map:?}..{map_max:?})")]
    ObstacleOutsideMap {
        id: u64,
        min: [f32; 3],
        max: [f32; 3],
        map: [f32; 3],
        map_max: [f32; 3],
    },
    #[error("obstacle id {id} appears more than once")]
    DuplicateObstacle { id: u64 },
}

fn triplet(v: Vec3f) -> [f32; 3] {
    components(v)
}

/// `Vec3f` has no indexing, and this file wants to walk axes.
fn components(v: Vec3f) -> [f32; 3] {
    [v.x, v.y, v.z]
}

fn from_components(c: [f32; 3]) -> Vec3f {
    Vec3f::new(c[0], c[1], c[2])
}

/// Checks a box is usable: finite on every corner and non-empty on every axis.
///
/// Empty is rejected rather than treated as "no collision". A zero-thickness box
/// is the shape you get from a typo in an authoring tool, and letting it through
/// produces an obstacle a shell passes straight through — indistinguishable from
/// an asteroid that is not there.
fn check_box(min: Vec3f, max: Vec3f) -> bool {
    let (min, max) = (components(min), components(max));
    let corners = [min[0], min[1], min[2], max[0], max[1], max[2]];
    if corners.iter().any(|c| !c.is_finite()) {
        return false;
    }
    (0..3).all(|axis| max[axis] > min[axis])
}

/// Clamps `point` into `bounds` — the working half of [`WorldMap::clamp_point`].
///
/// Bug №611: the tick's boundary pass cloned the entire `WorldMap` — every
/// obstacle in it — only to call `clamp_point`, and then immediately built an
/// inset map with `obstacles: Vec::new()`, throwing the cloned obstacles away
/// again. `clamp_point` only ever reads `bounds`, which is two `Vec3f`. Taking
/// the bounds directly makes the expensive half of that copy impossible.
pub fn clamp_point_into(bounds: &Bounds, point: Vec3f) -> (Vec3f, bool) {
    let mut clamped = components(point);
    let min = components(bounds.min);
    let max = components(bounds.max);
    let centre = components(bounds.center());
    let mut moved = false;
    for axis in 0..3 {
        if !clamped[axis].is_finite() {
            // A non-finite coordinate has no position to preserve. Centring
            // is the only choice that keeps the ship inside and finite.
            clamped[axis] = centre[axis];
            moved = true;
        } else if clamped[axis] < min[axis] {
            clamped[axis] = min[axis];
            moved = true;
        } else if clamped[axis] > max[axis] {
            clamped[axis] = max[axis];
            moved = true;
        }
    }
    (from_components(clamped), moved)
}

impl WorldMap {
    /// Parses and validates a map from JSON.
    pub fn from_json(name: &str, json: &str) -> Result<Self, MapError> {
        let parsed: WorldMap =
            serde_json::from_str(json).map_err(|e| MapError::Malformed(name.to_string(), e.to_string()))?;
        parsed.validated(name)?;
        Ok(parsed)
    }

    /// Checks everything a map has to satisfy, naming the offending field.
    pub fn validated(&self, name: &str) -> Result<(), MapError> {
        if !check_box(self.bounds.min, self.bounds.max) {
            return Err(if self.bounds.min.x.is_finite()
                && self.bounds.min.y.is_finite()
                && self.bounds.min.z.is_finite()
                && self.bounds.max.x.is_finite()
                && self.bounds.max.y.is_finite()
                && self.bounds.max.z.is_finite()
            {
                MapError::InvertedBounds {
                    min: triplet(self.bounds.min),
                    max: triplet(self.bounds.max),
                }
            } else {
                MapError::NonFiniteBounds {
                    min: triplet(self.bounds.min),
                    max: triplet(self.bounds.max),
                }
            });
        }
        if self.obstacles.len() > MAX_OBSTACLES {
            return Err(MapError::TooManyObstacles { count: self.obstacles.len() });
        }

        let mut seen = std::collections::HashSet::with_capacity(self.obstacles.len());
        for obstacle in &self.obstacles {
            if !seen.insert(obstacle.id) {
                return Err(MapError::DuplicateObstacle { id: obstacle.id });
            }
            if !check_box(obstacle.bounds.min, obstacle.bounds.max) {
                let finite = check_box_finite(obstacle.bounds.min, obstacle.bounds.max);
                return Err(if finite {
                    MapError::InvertedObstacle {
                        id: obstacle.id,
                        min: triplet(obstacle.bounds.min),
                        max: triplet(obstacle.bounds.max),
                    }
                } else {
                    MapError::NonFiniteObstacle { id: obstacle.id }
                });
            }
            if !self.bounds.contains(obstacle.bounds.min) || !self.bounds.contains(obstacle.bounds.max)
            {
                return Err(MapError::ObstacleOutsideMap {
                    id: obstacle.id,
                    min: triplet(obstacle.bounds.min),
                    max: triplet(obstacle.bounds.max),
                    map: triplet(self.bounds.min),
                    map_max: triplet(self.bounds.max),
                });
            }
        }
        let _ = name;
        Ok(())
    }

    /// Whether `point` is inside the playable volume.
    pub fn contains(&self, point: Vec3f) -> bool {
        self.bounds.contains(point)
    }

    /// Clamps `point` into the playable volume.
    ///
    /// Used for spawn placement and for the boundary test. Returns the clamped
    /// point and whether anything was clamped, so a caller that cares about a
    /// ship starting outside its map can say so instead of silently moving it.
    pub fn clamp_point(&self, point: Vec3f) -> (Vec3f, bool) {
        clamp_point_into(&self.bounds, point)
    }

    /// Builds an open-sea map, with `count` structures scattered through it.
    ///
    /// **The default is `count = 0`, and that is the only correct default for
    /// this project.** `TODO.md` part G, «Погода, мир, обнаружение»: the map is
    /// 50×50 km of open sea with no land. Structures exist because the same
    /// section says hard obstacles cast a radar shadow, and a mechanism with no
    /// content is not a reason to invent content.
    ///
    /// Deterministic: the same seed gives the same world, every time and on every
    /// machine. That is what makes it usable as a stand-in in tests and in
    /// development, and it costs one small PRNG rather than a dependency whose
    /// output could change between versions.
    pub fn open_sea(name: &str, radius: f32) -> Result<Self, MapError> {
        Self::procedural(name, 0, radius, 0)
    }

    /// Builds a map with `count` structures scattered through it.
    ///
    /// Callers on the server path want [`Self::open_sea`]. This exists for tests
    /// and for loading fixtures, and the difference is deliberate: a test that
    /// needs a target to hit says so, rather than relying on the world happening
    /// to contain one.
    pub fn procedural(name: &str, seed: u64, radius: f32, count: usize) -> Result<Self, MapError> {
        // Reported as a radius problem, not as inverted bounds. A NaN radius
        // used to come back as "bounds are inverted on at least one axis", which
        // names a field the caller never passed.
        if !radius.is_finite() || radius <= 0.0 {
            return Err(MapError::InvalidRadius { radius });
        }
        let count = count.min(MAX_OBSTACLES);
        let half = from_components([radius; 3]);
        let bounds = Bounds::new(-half, half);

        let mut rng = SmallRng::new(seed);
        let mut obstacles = Vec::with_capacity(count);
        for index in 0..count {
            let size_value = rng.range(4.0, 40.0);
            let size = from_components([size_value; 3]);
            // Shrink the placement box by the size so every rock is inside the map
            // by construction: validation would otherwise reject a map this
            // function produced, which would be a self-inflicted wound.
            let span = radius - size.x.max(size.y).max(size.z);
            // NaN-safe on purpose, as above: a rock larger than the world is a
            // reason to stop placing, not a reason to place one outside it.
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if !(span > 0.0) {
                break;
            }
            let centre = Vec3f::new(
                rng.range(-span, span),
                rng.range(-span, span),
                rng.range(-span, span),
            );
            obstacles.push(MapObstacle {
                // Ids live in their own band, clear of the ranges the server
                // hands out: ships from 10, projectiles from 5_000_000, players
// from 1_000_000. A collision target has to be unambiguous, or
                  // "what did I hit" resolves to a ship that happens to share a
                  // number with a structure.
                  id: 100_000 + index as u64,
                bounds: Bounds::new(centre - size * 0.5, centre + size * 0.5),
kind: ObstacleKind::Structure,
            });
        }

        let map = WorldMap {
            name: name.to_string(),
            bounds,
            obstacles,
        };
        map.validated(name)?;
        Ok(map)
    }

    /// The obstacle `segment` from `prev` to `curr` sweeps through, if any.
    ///
    /// Returns the *first* hit along the segment rather than an arbitrary one:
    /// with two rocks on the same line, whichever the loop reaches first is not
    /// the one a shell would meet first.
    pub fn swept_obstacle(
        &self,
        prev: Vec3f,
        curr: Vec3f,
    ) -> Option<(EntityId, &'static ObstacleKind, Vec3f, f32)> {
        let mut best: Option<(f32, EntityId, &'static ObstacleKind, f32)> = None;
        for obstacle in &self.obstacles {
            if !crate::tick::segment_intersects_bounds(prev, curr, &obstacle.bounds) {
                continue;
            }
            // Parameter along the segment, clamped, so ordering is comparable.
            let total = curr - prev;
            let length_sq = total.dot(total);
            let t = if length_sq > 0.0 {
                ((obstacle.bounds.center() - prev).dot(total) / length_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let kind: &'static ObstacleKind = &STRUCTURE_KIND;
            let better = best.is_none_or(|(best_t, _, _, _)| t < best_t);
            if better {
                // Depth is measured where the shell arrived, not where the tick
                // ended. A shell that crossed the whole box in one tick ends
                // outside it, and scoring that position gives zero for every
                // hit that mattered and non-zero for none of them.
                let impact = prev + (curr - prev) * t;
                best = Some((
                    t,
                    EntityId::new(obstacle.id),
                    kind,
                    obstacle.penetration_depth(impact),
                ));
            }
        }
        best.map(|(t, id, kind, depth)| (id, kind, prev + (curr - prev) * t, depth))
    }
}

/// `swept_obstacle` has to hand back a `&'static ObstacleKind` without borrowing
/// the map, because the caller holds the map immutably while mutating the tick
/// result.
static STRUCTURE_KIND: ObstacleKind = ObstacleKind::Structure;

fn check_box_finite(min: Vec3f, max: Vec3f) -> bool {
    [min.x, min.y, min.z, max.x, max.y, max.z]
        .iter()
        .all(|c| c.is_finite())
}

/// A tiny deterministic PRNG: SplitMix64.
///
/// Written out rather than pulled in so the world a seed produces cannot change
/// because a dependency bumped a version. The whole generator is about twenty
/// lines, and reproducibility across builds is worth more here than the feature
/// set of a general-purpose RNG.
struct SmallRng {
    state: u64,
}

impl SmallRng {
    fn new(seed: u64) -> Self {
        // Avoid the zero state, which is a fixed point for this generator.
        Self { state: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[low, high)`.
    fn range(&mut self, low: f32, high: f32) -> f32 {
        // 24 bits of mantissa is plenty and keeps the multiply in f32 exact
        // enough that the result cannot round above `high`.
        let unit = (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32;
        low + (high - low) * unit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-written map, so the tests exercise the parse path and not the
    /// generator. Numbers are chosen so that every rock is inside the volume —
    /// a map that fails its own validation is a different test.
    const AUTHORED: &str = r#"{
        "name": "test-water",
        "bounds": {
            "min": { "x": -1000.0, "y": -1000.0, "z": -1000.0 },
            "max": { "x":  1000.0, "y":  1000.0, "z":  1000.0 }
        },
        "obstacles": [
            {
                "id": 100001,
                "bounds": {
                    "min": { "x": 300.0, "y": -20.0, "z": -20.0 },
                    "max": { "x": 340.0, "y":  20.0, "z":  20.0 }
                },
                "kind": "Structure"
            },
            {
                "id": 100002,
                "bounds": {
                    "min": { "x": -500.0, "y": -5.0, "z": -5.0 },
                    "max": { "x": -460.0, "y":  5.0, "z":  5.0 }
                },
                "kind": "Structure"
            }
        ]
    }"#;

    fn authored() -> WorldMap {
        WorldMap::from_json("test-water", AUTHORED).expect("the fixture map is not valid")
    }

    /// A rock that pokes through the map face is broken authoring data. The
    /// point of this test is that it is *refused*: a clamp would produce a rock
    /// of a different shape than the file describes, and nobody would find out.
    #[test]
    fn a_rock_outside_the_world_is_refused_rather_than_trimmed() {
        let json = AUTHORED.replace(
            r#""min": { "x": 300.0, "y": -20.0, "z": -20.0 }"#,
            r#""min": { "x": 300.0, "y": -20.0, "z": -1400.0 }"#,
        );
        match WorldMap::from_json("test-water", &json) {
            Err(MapError::ObstacleOutsideMap { id, .. }) => assert_eq!(id, 100_001),
            other => panic!("an outside rock was accepted or misreported: {other:?}"),
        }
    }

    #[test]
    fn an_inverted_rock_is_refused() {
        let json = AUTHORED.replace(
            r#""min": { "x": 300.0, "y": -20.0, "z": -20.0 }"#,
            r#""min": { "x": 340.0, "y": -20.0, "z": -20.0 }"#,
        );
        assert!(matches!(
            WorldMap::from_json("test-water", &json),
            Err(MapError::InvertedObstacle { id: 100_001, .. })
        ));
    }

    #[test]
    fn an_inverted_world_is_refused() {
        let json = AUTHORED.replace(r#""max": { "x":  1000.0"#, r#""max": { "x": -2000.0"#);
        assert!(matches!(
            WorldMap::from_json("test-water", &json),
            Err(MapError::InvertedBounds { .. })
        ));
    }

    /// JSON has no spelling for NaN, so this goes through the generator: a NaN
    /// radius has to be named as a radius problem. Reporting it as inverted
    /// bounds points at a field the caller never passed.
    #[test]
    fn a_broken_radius_is_reported_as_a_radius() {
        // Matched rather than compared: a NaN radius never equals itself, so
        // `assert_eq!` on the error would fail for the wrong reason.
        for radius in [f32::NAN, f32::INFINITY, 0.0, -5.0] {
            match WorldMap::procedural("t", 1, radius, 4) {
                Err(MapError::InvalidRadius { .. }) => {}
                other => panic!("radius {radius} was not refused by name: {other:?}"),
            }
        }
    }

    #[test]
    fn two_rocks_with_one_id_are_refused() {
        let map = authored();
        let first = map.obstacles[0];
        let cloned = MapObstacle {
            bounds: Bounds::new(Vec3f::new(-60.0, -5.0, -5.0), Vec3f::new(-20.0, 5.0, 5.0)),
            ..first
        };
        let duplicated = WorldMap {
            obstacles: vec![first, cloned],
            ..map
        };
        assert_eq!(
            duplicated.validated("test-water"),
            Err(MapError::DuplicateObstacle { id: 100_001 })
        );
    }

    /// The cap exists because the tick walks every rock per shell; a file
    /// claiming two million of them is a bug or an attack, and both deserve a
    /// refusal at load time rather than an allocation.
    #[test]
    fn the_obstacle_cap_is_enforced() {
        let rock = MapObstacle {
            id: 1,
            bounds: Bounds::new(Vec3f::new(-1.0, -1.0, -1.0), Vec3f::new(1.0, 1.0, 1.0)),
            kind: ObstacleKind::Structure,
        };
        let map = WorldMap {
            name: "too-many".into(),
            bounds: Bounds::new(Vec3f::new(-10.0, -10.0, -10.0), Vec3f::new(10.0, 10.0, 10.0)),
            obstacles: vec![rock; MAX_OBSTACLES + 1],
        };
        assert_eq!(
            map.validated("too-many"),
            Err(MapError::TooManyObstacles { count: MAX_OBSTACLES + 1 })
        );
    }

    /// Reproducibility is the whole reason the generator is hand-written: a
    /// test that changes shape every build is a test nobody trusts.
    #[test]
    fn the_same_seed_gives_the_same_world() {
        let a = WorldMap::procedural("t", 7, 5000.0, 64).expect("generation failed");
        let b = WorldMap::procedural("t", 7, 5000.0, 64).expect("generation failed");
        assert_eq!(a, b, "the same seed produced two different worlds");

        let other = WorldMap::procedural("t", 8, 5000.0, 64).expect("generation failed");
        assert_ne!(
            a.obstacles, other.obstacles,
            "two different seeds produced the same field"
        );
    }

    /// The generator must not produce a map its own validator rejects — that
    /// would be a self-inflicted wound at every startup.
    #[test]
    fn every_generated_rock_is_inside_its_own_world() {
        for seed in 0..8u64 {
            let map = WorldMap::procedural("t", seed, 25_000.0, 512).expect("generation failed");
            assert_eq!(map.obstacles.len(), 512, "seed {seed} dropped rocks");
            for rock in &map.obstacles {
                assert!(
                    map.bounds.contains(rock.bounds.min) && map.bounds.contains(rock.bounds.max),
                    "seed {seed} put rock {} outside the world",
                    rock.id
                );
            }
        }
    }

    /// Two rocks on the same line: whichever the loop reaches first is not the
    /// one a shell meets first, and the difference is a shell passing through
    /// the far rock to report the near one.
    #[test]
    fn the_first_rock_a_shell_meets_is_the_one_reported() {
        let map = authored();
        let (id, _, hit, _) = map
            .swept_obstacle(Vec3f::new(-900.0, 0.0, 0.0), Vec3f::new(900.0, 0.0, 0.0))
            .expect("a shell straight through both rocks hit neither");
        assert_eq!(id, EntityId::new(100_002), "the far rock was reported");
        // The reported point must be the face of the near rock, not somewhere
        // past it: a hit position past the second rock would place the impact
        // on the wrong side of the first.
        assert!(
            hit.x < -400.0,
            "impact reported at {hit:?}, which is beyond the near rock"
        );
    }

    /// A segment that misses by a hair must miss. Without this, a shell would
    /// stop on a rock it flew past, and the ship behind it would take the hit.
    #[test]
    fn a_segment_that_clears_the_rock_does_not_hit_it() {
        let map = authored();
        assert!(map
            .swept_obstacle(Vec3f::new(-900.0, 60.0, 0.0), Vec3f::new(900.0, 60.0, 0.0))
            .is_none());
        assert!(map
            .swept_obstacle(Vec3f::new(0.0, -900.0, 0.0), Vec3f::new(0.0, 900.0, 0.0))
            .is_none());
    }

    #[test]
    fn a_graze_is_shallower_than_a_square_hit() {
        let map = authored();
        let rock = map
            .obstacles
            .iter()
            .find(|o| o.id == 100_002)
            .expect("fixture rock is missing");
        let centre = rock.bounds.center();
        let square = rock.penetration_depth(centre);
        let graze = rock.penetration_depth(Vec3f::new(centre.x - 19.0, centre.y, centre.z));
        assert!((square - 1.0).abs() < 1e-6, "a centre hit scored {square}");
        assert!(graze > 0.0 && graze < square, "a graze scored {graze} against {square}");
    }

    /// The penetration a hit reports has to describe the hit. A shell that
    /// crosses a 40 m rock in one tick and ends 300 m past it scored zero while
    /// technically stopping dead against it — the exact case where the number
    /// is wanted.
    #[test]
    fn penetration_is_measured_where_the_shell_arrived() {
        let map = authored();
        let (_, _, _, depth) = map
            .swept_obstacle(Vec3f::new(-900.0, 0.0, 0.0), Vec3f::new(900.0, 0.0, 0.0))
            .expect("a shell through the rock hit nothing");
        assert!(
            depth > 0.0 && depth <= 1.0,
            "a shell that stopped on the rock reported depth {depth}"
        );
    }

    /// Clamping reports whether it moved anything, because the caller needs to
    /// say "this ship started outside the world" rather than silently relocating
    /// it and letting the tick's own boundary clamp take the blame.
    #[test]
    fn clamping_says_so_and_centres_a_broken_coordinate() {
        let map = authored();
        let inside = Vec3f::new(10.0, 20.0, 30.0);
        let (kept, moved) = map.clamp_point(inside);
        assert_eq!(kept, inside);
        assert!(!moved, "a point inside the world was reported as moved");

        let outside = Vec3f::new(5000.0, 0.0, 0.0);
        let (clamped, moved) = map.clamp_point(outside);
        assert!(moved && clamped.x <= 1000.0, "a point outside was not clamped");

        let broken = Vec3f::new(f32::NAN, 0.0, 0.0);
        let (repaired, moved) = map.clamp_point(broken);
        assert!(moved);
        assert!(
            repaired.is_finite() && map.contains(repaired),
            "a non-finite coordinate became {repaired:?}"
        );
    }

    /// Bug №611: `clamp_point` was split so the tick can clamp against an inset
    /// volume without cloning the map. A split that drifts is two answers to
    /// "where is the edge", so the two are pinned to each other — including the
    /// non-finite case, which is the branch most likely to be re-implemented
    /// differently on one side only.
    #[test]
    fn clamping_by_bounds_agrees_with_clamping_the_map() {
        let map = authored();
        for point in [
            Vec3f::new(10.0, 20.0, 30.0),
            Vec3f::new(5000.0, 0.0, 0.0),
            Vec3f::new(-5000.0, 900.0, 0.0),
            Vec3f::new(f32::NAN, 0.0, 0.0),
            Vec3f::new(f32::INFINITY, f32::NEG_INFINITY, 0.0),
        ] {
            assert_eq!(
                clamp_point_into(&map.bounds, point),
                map.clamp_point(point),
                "{point:?} is clamped differently by the two paths"
            );
        }
    }

    /// Path of the map directory that ships with the server. Resolved from the
    /// crate root so the test does not depend on the working directory.
    fn shipped_maps() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps")
    }

    /// Every map on disk has to load.
    ///
    /// This test exists because of a specific failure: the shipped map named an
    /// `ObstacleKind` variant that had been deleted from the enum. Nothing
    /// failed to compile, nothing failed to build, and every test in the tree
    /// stayed green — because no test ever opened the file. Only running the
    /// server with `--map` found it, and by then the enum had been changed and
    /// the map had not.
    ///
    /// The obstacle *content* is covered by [`AUTHORED`] above. What is guarded
    /// here is a different thing: the file as it sits on disk.
    #[test]
    fn every_shipped_map_loads() {
        let dir = shipped_maps();
        let entries: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("shipped maps are unreadable at {}: {e}", dir.display()))
            .map(|entry| entry.expect("a map directory entry could not be read").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();

        // Not an `if let` with a pass: a map directory that lost its contents is
        // exactly the case that made this test necessary.
        assert!(
            !entries.is_empty(),
            "no map was found in {}, so this test would pass without checking anything",
            dir.display()
        );

        for path in entries {
            let json = std::fs::read_to_string(&path).expect("a shipped map could not be read");
            let name = path
                .file_stem()
                .expect("a shipped map has no file name")
                .to_string_lossy()
                .into_owned();
            if let Err(e) = WorldMap::from_json(&name, &json) {
                panic!("{} does not load: {e}", path.display());
            }
        }
    }

    /// The beta world is open sea: nothing on it.
    ///
    /// This is an owner's decision, not a derived one, and it is the thing most
    /// likely to be undone by someone who finds an empty map unfinished and
    /// fills it. That has already happened once on this project: the generator
    /// produced 256 rocks because an obstacle-free world looked like a missing
    /// feature, and rocks belong to a different game entirely.
    #[test]
    fn the_beta_map_is_open_sea() {
        let path = shipped_maps().join("open-water-50km.json");
        let json = std::fs::read_to_string(&path).expect("the beta map is missing");
        let map = WorldMap::from_json("open-water-50km", &json).expect("the beta map does not load");
        assert!(
            map.obstacles.is_empty(),
            "the beta world is open sea, but it carries {} obstacle(s): {:?}",
            map.obstacles.len(),
            map.obstacles
        );
    }
}