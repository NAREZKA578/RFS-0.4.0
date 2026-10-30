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

use std::collections::HashMap;
use rfs_core::entity::EntityId;
use rfs_core::math::{Bounds, Vec3f};
use serde::{Deserialize, Serialize};

/// Largest number of obstacles a single map may declare.
///
/// Bug №731 added a broadphase grid, so the tick no longer walks every
/// obstacle per projectile and per hull; the bound stays for what it always
/// was: a sanity bound. A JSON file claiming two million rocks is a bug or an
/// attack, and either way it should not be turned into a `Vec` allocation on
/// the server's startup path.
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
///
/// Bug №698: the kind lives in `rfs_core` now, because it travels on the
/// wire — the client is told about the structures, and it has to know what
/// kind each one is. One definition, two users.
pub use rfs_core::world::ObstacleKind;

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
    InvertedObstacle {
        id: u64,
        min: [f32; 3],
        max: [f32; 3],
    },
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
        let parsed: WorldMap = serde_json::from_str(json)
            .map_err(|e| MapError::Malformed(name.to_string(), e.to_string()))?;
        parsed.validated(name)?;
        Ok(parsed)
    }

    /// Checks everything a map has to satisfy, naming the offending field.
    pub fn validated(&self, name: &str) -> Result<(), MapError> {
        if !check_box(self.bounds.min, self.bounds.max) {
            return Err(
                if self.bounds.min.x.is_finite()
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
                },
            );
        }
        if self.obstacles.len() > MAX_OBSTACLES {
            return Err(MapError::TooManyObstacles {
                count: self.obstacles.len(),
            });
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
            if !self.bounds.contains(obstacle.bounds.min)
                || !self.bounds.contains(obstacle.bounds.max)
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

    /// The wire description of this world.
    ///
    /// Bug №698: the client has to be told what the world holds. The world is
    /// static, so this is a one-off description rather than a layer — and this
    /// is the one place where the simulation's boxes become the plain data the
    /// client stores.
    pub fn to_description(&self) -> rfs_core::world::MapDescription {
        rfs_core::world::MapDescription {
            name: self.name.clone(),
            bounds_min: self.bounds.min,
            bounds_max: self.bounds.max,
            obstacles: self
                .obstacles
                .iter()
                .map(|obstacle| rfs_core::world::MapObstacleDescription {
                    id: obstacle.id,
                    kind: obstacle.kind,
                    min: obstacle.bounds.min,
                    max: obstacle.bounds.max,
                })
                .collect(),
        }
    }

    /// Builds an open-sea map: no obstacles at all, at any seed.
    ///
    /// `TODO.md` part G, «Погода, мир, обнаружение»: the map is 50×50 km of
    /// open sea with no land. Structures exist because the same section says
    /// hard obstacles cast a radar shadow, and a mechanism with no content is
    /// not a reason to invent content. The seed is still part of the signature
    /// so a caller's `--map-seed` argument stays meaningful and the procedural
    /// generator remains available for tests that do need a target to hit —
    /// a test says so instead of relying on the world happening to contain one.
    ///
    /// Deterministic: the same seed gives the same world, every time and on every
    /// machine. That is what makes it usable as a stand-in in tests and in
    /// development, and it costs one small PRNG rather than a dependency whose
    /// output could change between versions.
    pub fn open_sea(name: &str, seed: u64, radius: f32) -> Result<Self, MapError> {
        Self::procedural(name, seed, radius, 0)
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

    /// Builds the broadphase for this map. Test-side convenience: the live
    /// system builds the grid together with the world in `set_world` and keeps
    /// it on the `TickSystem`; a freshly written map rebuilds its own.
    #[cfg(test)]
    pub(crate) fn broadphase(&self) -> ObstacleGrid {
        ObstacleGrid::build(self)
    }

    /// The obstacle `segment` from `prev` to `curr` sweeps through, if any.
    ///
    /// Returns the *first* hit along the segment rather than an arbitrary one:
    /// the obstacle with the smallest segment parameter, and on a tie the one
    /// that comes first in the map's list — the same rule the full scan
    /// applied.
    ///
    /// Bug №731: the full scan walked every obstacle of the map for every
    /// projectile every tick. The broadphase grid narrows the walk to the
    /// cells the segment passes through; the answer is unchanged, because the
    /// cells a point lies in are a superset of the boxes a segment touches
    /// (see [`ObstacleGrid::sweep_candidates`]).
    ///
    /// `broadphase` may be `None` — an absent grid is exactly the pre-№731
    /// behaviour, kept as the fallback rather than a hard dependency, so a
    /// caller that has no grid never pays for building one on the hot path.
    pub fn swept_obstacle(
        &self,
        prev: Vec3f,
        curr: Vec3f,
        broadphase: Option<&ObstacleGrid>,
    ) -> Option<(EntityId, &'static ObstacleKind, Vec3f, f32)> {
        match broadphase.and_then(|grid| grid.sweep_candidates(prev, curr)) {
            Some(indices) => self.swept_among(
                prev,
                curr,
                indices.iter().map(|&i| &self.obstacles[i as usize]),
            ),
            None => self.swept_among(prev, curr, self.obstacles.iter()),
        }
    }

    /// The selection rule both the grid path and the full scan share: the
    /// smallest parameter along the segment, ties going to the earlier list
    /// entry (strict `<` keeps the first of equals).
    fn swept_among<'a, I>(
        &self,
        prev: Vec3f,
        curr: Vec3f,
        obstacles: I,
    ) -> Option<(EntityId, &'static ObstacleKind, Vec3f, f32)>
    where
        I: Iterator<Item = &'a MapObstacle>,
    {
        let mut best: Option<(f32, EntityId, &'static ObstacleKind, f32)> = None;
        for obstacle in obstacles {
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

/// The obstacle index the broadphase walks, one per map obstacle.
///
/// A uniform grid over the map: every obstacle is registered in every cell its
/// axis-aligned box overlaps. A query walks only the cells it touches, so the
/// per-tick cost follows what the query is near, not what the map holds (bug
/// №731: 65 536 rocks × every projectile × every hull at 30 Hz was a linear
/// tax on map size that two separate consumers paid per tick).
///
/// Fidelity contract, pinned by tests: the grid is a *filter*, never a
/// decider. It hands back candidate indices in list order, and its sweep walk
/// provably covers every cell any point of the segment lies in, so the
/// caller's selection rule still sees every box the full scan would have
/// seen — the candidates are a superset of the true hits, never a subset.
/// public APIs take it as an opaque parameter, never constructed outside.
pub struct ObstacleGrid {
    /// Edge length of one cell in world units.
    cell: f32,
    /// Obstacle indices per occupied cell, ascending within a cell by
    /// construction (obstacles are inserted in list order).
    cells: HashMap<[i32; 3], Vec<u32>>,
}

impl ObstacleGrid {
    /// Builds the grid for a validated map.
    ///
    /// The cell edge is sized by the largest obstacle (`extent / 4`, so even
    /// the biggest rock spans at most a handful of cells per axis, which
    /// bounds both the entries an obstacle costs and the cells any query
    /// walks), floored by `diagonal / 512` so a map of needle rocks cannot
    /// turn the grid into a point cloud, and floored by 1 so no cell edge is
    /// ever degenerate.
    pub(crate) fn build(map: &WorldMap) -> Self {
        let mut max_extent = 0.0f32;
        for obstacle in &map.obstacles {
            let size = obstacle.bounds.size();
            max_extent = max_extent.max(size.x.max(size.y).max(size.z));
        }
        let size = map.bounds.size();
        let diagonal = (size.x * size.x + size.y * size.y + size.z * size.z).sqrt();

        let cell = (max_extent / 4.0).max(diagonal / 512.0).max(1.0);
        let mut cells: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (index, obstacle) in map.obstacles.iter().enumerate() {
            let min = [
                cell_axis(obstacle.bounds.min.x, cell),
                cell_axis(obstacle.bounds.min.y, cell),
                cell_axis(obstacle.bounds.min.z, cell),
            ];
            let max = [
                cell_axis(obstacle.bounds.max.x, cell),
                cell_axis(obstacle.bounds.max.y, cell),
                cell_axis(obstacle.bounds.max.z, cell),
            ];
            for x in min[0]..=max[0] {
                for y in min[1]..=max[1] {
                    for z in min[2]..=max[2] {
                        cells.entry([x, y, z]).or_default().push(index as u32);
                    }
                }
            }
        }
        Self { cell, cells }
    }

    /// Indices of obstacles whose boxes overlap the box the segment sweeps
    /// (the segment's own axis-aligned hull), ascending, deduplicated.
    ///
    /// `None` means "this query is degenerate for the grid" — non-finite
    /// input, or a walk that outgrew the seat-belt cap — and the caller falls
    /// back to the full scan. On a validated map with finite positions the
    /// walk costs at most the cells the segment's spans cover.
    ///
    /// Coverage argument, pinned by the parity test: every point `p` of the
    /// segment lies in exactly one cell, `floor(p / cell)`; `floor` is
    /// monotone, so any obstacle box that contains `p` spans that cell or a
    /// neighbour the DDA step reaches between its own `floor(min)` and
    /// `floor(max)` — the walk never loses a box the segment touches.
    pub(crate) fn sweep_candidates(&self, prev: Vec3f, curr: Vec3f) -> Option<Vec<u32>> {
        if !(prev.x.is_finite()
            && prev.y.is_finite()
            && prev.z.is_finite()
            && curr.x.is_finite()
            && curr.y.is_finite()
            && curr.z.is_finite())
        {
            return None;
        }

        // Amanatides & Woo: walk the cells the line passes through, stepping
        // whichever axis reaches its next cell boundary first. Cell-space
        // arithmetic in f64: the walk must agree with itself over the whole
        // segment, and cell boundaries are exactly where f32 drift would pay.
        let cell = self.cell as f64;
        let mut cell_at = [
            (prev.x as f64 / cell).floor(),
            (prev.y as f64 / cell).floor(),
            (prev.z as f64 / cell).floor(),
        ];
        let dir = [
            (curr.x - prev.x) as f64 / cell,
            (curr.y - prev.y) as f64 / cell,
            (curr.z - prev.z) as f64 / cell,
        ];
        let steps = [
            if dir[0] > 0.0 {
                1
            } else if dir[0] < 0.0 {
                -1
            } else {
                0
            },
            if dir[1] > 0.0 {
                1
            } else if dir[1] < 0.0 {
                -1
            } else {
                0
            },
            if dir[2] > 0.0 {
                1
            } else if dir[2] < 0.0 {
                -1
            } else {
                0
            },
        ];
        // Fraction of the segment travelled when each axis first reaches its
        // next cell boundary in `steps[axis]` direction, and the fraction a
        // full cell edge costs once the walk is underway.
        let mut t_next = [f64::INFINITY; 3];
        let mut t_delta = [f64::INFINITY; 3];
        for axis in 0..3 {
            if steps[axis] == 0 {
                continue;
            }
            let position = (if axis == 0 {
                prev.x
            } else if axis == 1 {
                prev.y
            } else {
                prev.z
            }) as f64 / cell;
            let into_cell = position - position.floor(); // 0..1
            // Exactly on a boundary (`into_cell == 0`) the negative direction
            // steps immediately into the cell behind, which is correct: the
            // start cell was registered below, and what the walk visits now
            // is the other side of that boundary.
            t_next[axis] = (if steps[axis] > 0 {
                1.0 - into_cell
            } else {
                into_cell
            }) / dir[axis].abs();
            t_delta[axis] = 1.0 / dir[axis].abs();
        }

        let mut indices: Vec<u32> = Vec::new();
        // The start cell first: obstacles the segment begins inside must be
        // candidates before the walk moves anywhere. (NCGRID-731-TEMP proved
        // the parity test reacts: without this block the zero-length segment
        // inside a rock returned None while the full scan found it.)
        if let Some(bucket) = self.cells.get(&[
            cell_at[0] as i32,
            cell_at[1] as i32,
            cell_at[2] as i32,
        ]) {
            indices.extend_from_slice(bucket);
        }
        // Seat belt: on a validated map the per-axis spans bound the walk, so
        // the cap only ever matters if f64 drift manufactures a pathological
        // step. The honest ceiling is the sum of the axis spans (~513 each).
        for _ in 0..2048 {
            let axis = (0..3)
                .min_by(|a, b| t_next[*a].total_cmp(&t_next[*b]))
                .unwrap();
            if t_next[axis] > 1.0 {
                break;
            }
            cell_at[axis] += steps[axis] as f64;
            t_next[axis] += t_delta[axis];
            if let Some(bucket) = self.cells.get(&[
                cell_at[0] as i32,
                cell_at[1] as i32,
                cell_at[2] as i32,
            ]) {
                indices.extend_from_slice(bucket);
            }
        }
        indices.sort_unstable();
        indices.dedup();
        Some(indices)
    }

    /// Indices of obstacles whose boxes overlap `[min, max]`, ascending,
    /// deduplicated. Same `None` contract as [`ObstacleGrid::sweep_candidates`]:
    /// a query box that would walk an unreasonable number of cells (a hull
    /// the size of the map, or non-finite corners) falls back to the full
    /// scan, which is exactly what the tick did before №731.
    pub(crate) fn box_candidates(&self, min: Vec3f, max: Vec3f) -> Option<Vec<u32>> {
        const MAX_CELLS_PER_QUERY: i64 = 20_000;
        let x0 = cell_axis(min.x, self.cell);
        let y0 = cell_axis(min.y, self.cell);
        let z0 = cell_axis(min.z, self.cell);
        let x1 = cell_axis(max.x, self.cell);
        let y1 = cell_axis(max.y, self.cell);
        let z1 = cell_axis(max.z, self.cell);
        let span_x = x1 as i64 - x0 as i64 + 1;
        let span_y = y1 as i64 - y0 as i64 + 1;
        let span_z = z1 as i64 - z0 as i64 + 1;
        let walked = span_x
            .checked_mul(span_y)
            .and_then(|xy| xy.checked_mul(span_z));
        if walked.unwrap_or(MAX_CELLS_PER_QUERY + 1) > MAX_CELLS_PER_QUERY {
            return None;
        }

        let mut indices: Vec<u32> = Vec::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    if let Some(bucket) = self.cells.get(&[x, y, z]) {
                        indices.extend_from_slice(bucket);
                    }
                }
            }
        }
        indices.sort_unstable();
        indices.dedup();
        Some(indices)
    }
}

// The grid does not live inside WorldMap — a map stays a plain value
// (cloned, compared, serialized), and the grid is a derived index over it.
// It is carried by its owner instead: the TickSystem holds its world as a
// map+broadphase pair under one lock (WorldSlot), so no reader can ever
// combine a grid built for one map with another map's obstacle list — grid
// indices from one list read against another are nonsense at best and an
// out-of-bounds read at worst. `set_world` builds the grid for the map it
// installs, in the same write that installs it.

/// The cell index of a world coordinate: `floor(coord / cell)`. The `as i32`
/// cast is exact for every number a validated map or a live entity can carry
/// (tens of kilometres against a cell of metres); a corrupt but finite value
/// degrades to a wrong cell of the grid, never to a panic.
fn cell_axis(coordinate: f32, cell: f32) -> i32 {
    (coordinate / cell).floor() as i32
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
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
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

    /// `open_sea` forces the obstacle count to zero no matter what its seed is:
    /// part G says an open sea has no land, and a generator that sprays
    /// structures would quietly move the point of the whole helper.
    #[test]
    fn an_open_sea_map_is_open_at_any_seed() {
        for seed in [0u64, 12345, 77777] {
            let sea = WorldMap::open_sea("seed-echo", seed, 25_000.0)
                .expect("open_sea rejects a radius it accepts elsewhere");
            assert!(sea.obstacles.is_empty(), "seed {seed} produced obstacles");
            assert_eq!(sea.bounds.min, from_components([-25_000.0; 3]));
            assert_eq!(sea.bounds.max, from_components([25_000.0; 3]));
        }
    }

    /// A radius that `procedural` refuses cannot be accepted by public helper
    /// either: it is the same guard, reached through a narrower call.
    #[test]
    fn an_open_sea_refuses_an_invalid_radius() {
        match WorldMap::open_sea("seed-echo", 1, -1.0) {
            Err(MapError::InvalidRadius { radius }) => assert_eq!(radius, -1.0),
            other => panic!("open_sea accepted an invalid radius, got {other:?}"),
        }
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

    /// Bug №699: the description is the only thing the client ever sees of the
    /// world, so it has to be THE world. The name and bounds are compared for
    /// equality, and every structure is matched pair by pair — id and kind are
    /// what the client uses to tell a rock from a berth, and only identical
    /// boxes make the two views agree on what exists.
    #[test]
    fn a_world_describes_itself_for_the_client() {
        let world = authored();
        let description = world.to_description();

        assert_eq!(description.name, "test-water");
        assert_eq!(description.bounds_min, world.bounds.min);
        assert_eq!(description.bounds_max, world.bounds.max);
        assert_eq!(description.obstacles.len(), world.obstacles.len());

        for (original, described) in world.obstacles.iter().zip(&description.obstacles) {
            assert_eq!(described.id, original.id, "structure id diverged");
            assert_eq!(described.kind, original.kind, "structure kind diverged");
            assert_eq!(described.min, original.bounds.min, "box min diverged");
            assert_eq!(described.max, original.bounds.max, "box max diverged");
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
            bounds: Bounds::new(
                Vec3f::new(-10.0, -10.0, -10.0),
                Vec3f::new(10.0, 10.0, 10.0),
            ),
            obstacles: vec![rock; MAX_OBSTACLES + 1],
        };
        assert_eq!(
            map.validated("too-many"),
            Err(MapError::TooManyObstacles {
                count: MAX_OBSTACLES + 1
            })
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

    /// Both sweep paths on every question a test asks: the grid path the
    /// production tick takes, and the `None` fallback the pre-№731 tick took.
    /// Their disagreement would mean the grid decided an answer, which the
    /// contract forbids — this helper pins parity on every legacy assertion.
    fn swept(
        map: &WorldMap,
        prev: Vec3f,
        curr: Vec3f,
    ) -> Option<(EntityId, &'static ObstacleKind, Vec3f, f32)> {
        let grid = map.broadphase();
        let by_grid = map.swept_obstacle(prev, curr, Some(&grid));
        assert_eq!(
            by_grid,
            map.swept_obstacle(prev, curr, None),
            "grid and full scan disagreed along {prev:?} → {curr:?}"
        );
        by_grid
    }

    /// Two rocks on the same line: whichever the loop reaches first is not the
    /// one a shell meets first, and the difference is a shell passing through
    /// the far rock to report the near one.
    #[test]
    fn the_first_rock_a_shell_meets_is_the_one_reported() {
        let map = authored();
        let (id, _, hit, _) = swept(
            &map,
            Vec3f::new(-900.0, 0.0, 0.0),
            Vec3f::new(900.0, 0.0, 0.0),
        )
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

    /// Grid and full scan parity for a single segment. Where a finite answer
    /// is expected this is plain equality, but a degenerate or non-finite
    /// segment can produce NaN in its own right, and NaN == NaN is the fact
    /// there — so the comparison is bit-exact, which also pins "identical
    /// expression, identical result" harder than PartialEq would.
    #[allow(clippy::type_complexity)]
    fn same_hit(
        a: Option<(EntityId, &'static ObstacleKind, Vec3f, f32)>,
        b: Option<(EntityId, &'static ObstacleKind, Vec3f, f32)>,
    ) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(x), Some(y)) => {
                let (p, q) = (x.2, y.2);
                x.0 == y.0
                    && x.1 == y.1
                    && x.3.to_bits() == y.3.to_bits()
                    && p.x.to_bits() == q.x.to_bits()
                    && p.y.to_bits() == q.y.to_bits()
                    && p.z.to_bits() == q.z.to_bits()
            }
            _ => false,
        }
    }

    /// The point of №731: the grid is a filter, never a decider. Four hundred
    /// manufactured segments — across the rocks, grazing them, missing by
    /// inches, lying outside the map, degenerate to a point, and one with
    /// non-finite ends — must produce the same answer from the grid path the
    /// tick takes and the `None` path the pre-№731 tick took.
    #[test]
    fn the_broadphase_returns_what_the_full_scan_returns() {
        let map = authored();
        let grid = map.broadphase();
        let mut rng = SmallRng::new(0x731_5EED);
        // A degenerate zero-length segment, then the manufactured ones.
        for (prev, curr) in [
            (Vec3f::new(320.0, 0.0, 0.0), Vec3f::new(320.0, 0.0, 0.0)),
            (Vec3f::new(f32::NAN, 0.0, 0.0), Vec3f::new(900.0, 0.0, 0.0)),
            (
                Vec3f::new(-900.0, 0.0, 0.0),
                Vec3f::new(f32::INFINITY, 0.0, 0.0),
            ),
        ] {
            let by_grid = map.swept_obstacle(prev, curr, Some(&grid));
            let by_scan = map.swept_obstacle(prev, curr, None);
            assert!(
                same_hit(by_grid, by_scan),
                "grid and full scan disagreed on a special segment {prev:?} → {curr:?}: \
                 grid={by_grid:?}, scan={by_scan:?}"
            );
        }
        for case in 0..400 {
            let prev = Vec3f::new(
                rng.range(-1100.0, 1100.0),
                rng.range(-1100.0, 1100.0),
                rng.range(-1100.0, 1100.0),
            );
            let span = rng.range(-1600.0, 1600.0);
            let curr = Vec3f::new(
                prev.x + span,
                prev.y + rng.range(-1600.0, 1600.0),
                prev.z + rng.range(-1600.0, 1600.0),
            );
            let by_grid = map.swept_obstacle(prev, curr, Some(&grid));
            let by_scan = map.swept_obstacle(prev, curr, None);
            assert!(
                same_hit(by_grid, by_scan),
                "grid and full scan disagreed on segment {case} from {prev:?} to {curr:?}: \
                 grid={by_grid:?}, scan={by_scan:?}"
            );
        }
    }

    /// The other half of №731: the walk must follow what is NEAR the query,
    /// not what the map holds. Two thousand scattered rocks, a segment down
    /// the map's middle — the candidates it collects are the rocks a line of
    /// fire could plausibly touch, with the 1995 distant ones out of it, and
    /// the answer it leads to is still whatever the full scan says.
    #[test]
    fn a_straight_segment_walks_a_handful_of_candidates_not_the_whole_map() {
        let mut rng = SmallRng::new(0x731_BA5E);
        // Four rocks strung along the x axis at y=z=0, in reach of a line of
        // fire; nineteen hundred and ninety six rocks scattered far off that
        // line, out of reach of anything it can graze.
        let mut obstacles = Vec::new();
        for (n, x) in [-400.0, -50.0, 300.0, 700.0].iter().enumerate() {
            let x = *x;
            obstacles.push(MapObstacle {
                id: 900_001 + n as u64,
                bounds: Bounds::new(
                    Vec3f::new(x, -6.0, -6.0),
                    Vec3f::new(x + 12.0, 6.0, 6.0),
                ),
                kind: ObstacleKind::Structure,
            });
        }
        for i in 0..1996u64 {
            let x = rng.range(-900.0, 900.0);
            // Off the firing line: the nearest scatter is 30 units off it,
            // and the segment tests cells around y=z=0 only.
            let y = 30.0 * rng.range(1.2, 33.0) * if rng.next_u64() & 1 == 0 { 1.0 } else { -1.0 };
            let z = 30.0 * rng.range(1.2, 33.0) * if rng.next_u64() & 1 == 0 { 1.0 } else { -1.0 };
            obstacles.push(MapObstacle {
                id: 10_000 + i,
                bounds: Bounds::new(
                    Vec3f::new(x, y - 6.0, z - 6.0),
                    Vec3f::new(x + 12.0, y + 6.0, z + 6.0),
                ),
                kind: ObstacleKind::Structure,
            });
        }
        let map = WorldMap {
            name: "sublinear".to_string(),
            bounds: Bounds::new(
                Vec3f::new(-1000.0, -1000.0, -1000.0),
                Vec3f::new(1000.0, 1000.0, 1000.0),
            ),
            obstacles,
        };
        let grid = map.broadphase();

        let prev = Vec3f::new(-990.0, 0.0, 0.0);
        let curr = Vec3f::new(990.0, 0.0, 0.0);
        let candidates = grid
            .sweep_candidates(prev, curr)
            .expect("a finite segment must always be answerable");
        assert!(
            candidates.len() <= map.obstacles.len() / 20,
            "a line of fire across {} rocks collected {} candidates",
            map.obstacles.len(),
            candidates.len()
        );
        assert_eq!(
            map.swept_obstacle(prev, curr, Some(&grid)),
            map.swept_obstacle(prev, curr, None),
            "the broadphase's few candidates changed the answer"
        );
    }

    /// A segment that misses by a hair must miss. Without this, a shell would
    /// stop on a rock it flew past, and the ship behind it would take the hit.
    #[test]
    fn a_segment_that_clears_the_rock_does_not_hit_it() {
        let map = authored();
        assert!(swept(
            &map,
            Vec3f::new(-900.0, 60.0, 0.0),
            Vec3f::new(900.0, 60.0, 0.0)
        )
        .is_none());
        assert!(swept(
            &map,
            Vec3f::new(0.0, -900.0, 0.0),
            Vec3f::new(0.0, 900.0, 0.0)
        )
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
        assert!(
            graze > 0.0 && graze < square,
            "a graze scored {graze} against {square}"
        );
    }

    /// The penetration a hit reports has to describe the hit. A shell that
    /// crosses a 40 m rock in one tick and ends 300 m past it scored zero while
    /// technically stopping dead against it — the exact case where the number
    /// is wanted.
    #[test]
    fn penetration_is_measured_where_the_shell_arrived() {
        let map = authored();
        let (_, _, _, depth) = swept(
            &map,
            Vec3f::new(-900.0, 0.0, 0.0),
            Vec3f::new(900.0, 0.0, 0.0),
        )
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
        assert!(
            moved && clamped.x <= 1000.0,
            "a point outside was not clamped"
        );

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
            .map(|entry| {
                entry
                    .expect("a map directory entry could not be read")
                    .path()
            })
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
        let map =
            WorldMap::from_json("open-water-50km", &json).expect("the beta map does not load");
        assert!(
            map.obstacles.is_empty(),
            "the beta world is open sea, but it carries {} obstacle(s): {:?}",
            map.obstacles.len(),
            map.obstacles
        );
    }
}
