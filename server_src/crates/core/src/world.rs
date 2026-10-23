//! The world's static description, and the types that carry it on the wire.
//!
//! Bug №698: the server could load a map, but the client never heard of it.
//! The replication layers carry ships, players, stations, compartments and
//! projectiles — and no structure, so a player watched a shell vanish at a
//! rock that was not in their world at all.
//!
//! The world is STATIC (the owner's decision: open sea, structures are
//! fixed), so it is not a layer and not a stream: it is one description,
//! sent once, when the client connects. Everything here is therefore plain
//! data with no behaviour: the same description is what the server loaded,
//! what the client stores, and what a renderer would draw.

use crate::math::Vec3f;
use serde::{Deserialize, Serialize};

/// What a solid thing is.
///
/// Kept here, in the crate every other one can see, because the kind travels
/// on the wire: the client has to know whether the thing it was told about is
/// a structure, and the collision event carries the same kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ObstacleKind {
    /// Built structure — the only kind an open-sea map is expected to hold.
    #[default]
    Structure,
}

/// One solid volume, in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MapObstacleDescription {
    /// Stable identity, so a client can be told about the same volume twice
    /// and still recognise it.
    pub id: u64,
    pub kind: ObstacleKind,
    pub min: Vec3f,
    pub max: Vec3f,
}

impl MapObstacleDescription {
    /// The box this volume occupies.
    pub fn bounds(&self) -> crate::math::Bounds {
        crate::math::Bounds::new(self.min, self.max)
    }
}

/// The world as the client is told about it: the volume to stay inside and
/// what is inside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapDescription {
    pub name: String,
    pub bounds_min: Vec3f,
    pub bounds_max: Vec3f,
    pub obstacles: Vec<MapObstacleDescription>,
}

impl MapDescription {
    /// The playable volume.
    pub fn bounds(&self) -> crate::math::Bounds {
        crate::math::Bounds::new(self.bounds_min, self.bounds_max)
    }
}
