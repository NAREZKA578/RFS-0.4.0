use glam::{Quat, Vec3};

/// Complete configuration for a game map.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MapConfig {
    /// Internal map identifier.
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Map description text.
    pub description: String,
    /// Map size category (e.g. "large", "medium").
    pub size: String,
    /// Path to the loading screen image.
    pub loading_screen_path: String,
    /// Path to the minimap texture.
    pub minimap_path: String,
    /// Path to the map preview image.
    pub preview_image_path: String,
    /// Terrain configuration (heightmap, splatmap, physics).
    pub terrain: TerrainConfig,
    /// All spawn points on this map.
    pub spawn_points: Vec<SpawnPointConfig>,
    /// All capture points on this map.
    pub capture_points: Vec<CapturePointConfig>,
    /// All vehicle spawn locations.
    pub vehicle_spawns: Vec<VehicleSpawnConfig>,
    /// Zones where FOBs can be placed.
    pub fob_placement_zones: Vec<FobPlacementZone>,
    /// Map boundary configuration.
    pub boundaries: BoundaryConfig,
    /// Weather settings for this map.
    pub weather: WeatherConfig,
    /// Water areas on the map.
    #[serde(default)]
    pub water_areas: Vec<WaterArea>,
}

impl MapConfig {
    /// Returns the dimensions of the map boundaries as a Vec3.
    pub fn dimensions(&self) -> Vec3 {
        self.boundaries.max - self.boundaries.min
    }

    /// Returns `true` if the given point is within the map boundaries.
    pub fn contains_point(&self, point: Vec3) -> bool {
        point.x >= self.boundaries.min.x
            && point.x <= self.boundaries.max.x
            && point.y >= self.boundaries.min.y
            && point.y <= self.boundaries.max.y
            && point.z >= self.boundaries.min.z
            && point.z <= self.boundaries.max.z
    }

    /// Returns `true` if the point is inside the kill zone (above or below the map).
    pub fn is_in_kill_zone(&self, point: Vec3) -> bool {
        point.y < self.boundaries.kill_below || point.y > self.boundaries.kill_above
    }

    /// Returns `true` if the point is inside the soft boundary warning zone.
    pub fn is_in_soft_boundary(&self, point: Vec3) -> bool {
        let soft = self.boundaries.soft_boundary_distance;
        // Clamp soft distance to no more than half the map dimension, so the inner
        // rectangle never inverts (min > max) and falsely reports every point.
        let half_x = (self.boundaries.max.x - self.boundaries.min.x) * 0.5;
        let half_z = (self.boundaries.max.z - self.boundaries.min.z) * 0.5;
        let soft_x = soft.min(half_x);
        let soft_z = soft.min(half_z);
        let min = self.boundaries.min + Vec3::new(soft_x, 0.0, soft_z);
        let max = self.boundaries.max - Vec3::new(soft_x, 0.0, soft_z);
        point.x < min.x || point.x > max.x || point.z < min.z || point.z > max.z
    }
}

/// Configuration for a spawn point.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpawnPointConfig {
    /// Unique identifier for this spawn point.
    pub id: u32,
    /// World-space position.
    pub position: Vec3,
    /// Spawn orientation.
    pub rotation: Quat,
    /// Faction this spawn belongs to, or `None` for neutral.
    pub faction: Option<String>,
    /// Type of spawn (e.g. "main", "habs", "radio").
    pub spawn_type: String,
    /// Whether only squad members can use this spawn.
    pub squad_only: bool,
    /// Whether this spawn point is currently active.
    pub is_active: bool,
    /// Spawn radius in meters.
    pub radius: f32,
    /// Display name of the spawn point.
    pub name: String,
    /// ID of a linked capture point that enables this spawn.
    pub linked_capture_point: Option<u32>,
}

/// Configuration for a capture point on the map.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CapturePointConfig {
    /// Unique identifier.
    pub id: u32,
    /// Display name.
    pub name: String,
    /// World-space center position.
    pub position: Vec3,
    /// Capture radius in meters.
    pub radius: f32,
    /// IDs of linked capture points.
    pub linked_points: Vec<u32>,
    /// Faction that owns this point at match start.
    pub initial_owner: Option<String>,
    /// Capture rate per second.
    pub capture_rate: f32,
    /// Neutral decay rate per second.
    pub neutral_rate: f32,
    /// Ticket value awarded on capture.
    pub ticket_value: u32,
}

/// Terrain data configuration for the map.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TerrainConfig {
    /// Path to the heightmap texture.
    pub heightmap_path: String,
    /// Path to the splatmap (terrain layer painting).
    pub splatmap_path: String,
    /// Path to the physics collision mesh.
    pub physics_mesh_path: String,
    /// Path to the navigation mesh for AI pathfinding.
    pub navmesh_path: String,
    /// Water level height in world units.
    pub water_level: f32,
    /// Default surface friction coefficient.
    pub default_friction: f32,
    /// Default surface bounciness coefficient.
    pub default_bounciness: f32,
    /// Terrain texture/material layers.
    pub layers: Vec<TerrainLayer>,
}

/// A single terrain texture/material layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TerrainLayer {
    /// Layer name identifier.
    pub name: String,
    /// Path to the albedo/diffuse texture.
    pub texture_path: String,
    /// Path to the normal map texture.
    pub normal_path: String,
    /// Surface roughness value.
    pub roughness: f32,
    /// Surface metalness value.
    pub metalness: f32,
    /// Texture tiling scale.
    pub tiling: f32,
    /// Impact type for bullet holes and effects.
    pub impact_type: String,
}

/// Configuration for a vehicle spawn location.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VehicleSpawnConfig {
    /// Unique identifier for this spawn.
    pub id: u32,
    /// World-space position.
    pub position: Vec3,
    /// Spawn orientation.
    pub rotation: Quat,
    /// Faction this spawn belongs to, or `None` for neutral.
    pub faction: Option<String>,
    /// Type of vehicle that spawns here.
    pub vehicle_type: String,
    /// Time in seconds before the vehicle respawns after being destroyed.
    pub respawn_time: f32,
    /// Maximum number of vehicles at this spawn.
    pub max_vehicles: u32,
    /// Whether this spawn is currently active.
    pub is_active: bool,
    /// Display name of the spawn.
    pub name: String,
}

/// A zone where forward operating bases can be placed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FobPlacementZone {
    /// Unique identifier for this zone.
    pub id: u32,
    /// Minimum corner of the placement volume.
    pub min_position: Vec3,
    /// Maximum corner of the placement volume.
    pub max_position: Vec3,
    /// Faction this zone belongs to, or `None` for neutral.
    pub faction: Option<String>,
    /// Minimum distance from other friendly FOBs.
    pub min_distance_from_other_fob: f32,
    /// Minimum distance from enemy FOBs.
    pub min_distance_from_enemy_fob: f32,
    /// Whether this zone is currently active.
    pub is_active: bool,
}

/// Map boundary configuration controlling the playable area.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoundaryConfig {
    /// Minimum corner of the map boundary.
    pub min: Vec3,
    /// Maximum corner of the map boundary.
    pub max: Vec3,
    /// Y-coordinate below which players die instantly.
    pub kill_below: f32,
    /// Y-coordinate above which players die instantly.
    pub kill_above: f32,
    /// Distance from the edge where the soft boundary warning activates.
    pub soft_boundary_distance: f32,
    /// Damage per second applied inside the soft boundary.
    pub damage_per_second: f32,
}

/// Weather configuration for the map.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WeatherConfig {
    /// Weather type identifier (e.g. "clear", "rain", "fog").
    pub weather_type: String,
    /// Wind speed in meters per second.
    pub wind_speed: f32,
    /// Wind direction in degrees (0-360).
    pub wind_direction: f32,
    /// Cloud coverage from 0.0 (clear) to 1.0 (overcast).
    pub cloud_coverage: f32,
    /// Precipitation intensity from 0.0 to 1.0.
    pub precipitation_intensity: f32,
    /// Visibility distance in meters.
    pub visibility: f32,
    /// Temperature in degrees Celsius.
    pub temperature: f32,
    /// Fog density from 0.0 to 1.0.
    pub fog_density: f32,
    /// Fog color as [R, G, B] in linear space.
    pub fog_color: [f32; 3],
    /// Time of day as hours (0.0 - 24.0).
    pub time_of_day: f32,
}

/// A water area on the map.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WaterArea {
    /// Unique identifier for this water area.
    pub id: u32,
    /// World-space center position.
    pub position: Vec3,
    /// Dimensions (width, height, depth) of the water volume.
    pub size: Vec3,
    /// Water depth in meters.
    pub depth: f32,
    /// Water flow speed in meters per second.
    pub flow_speed: f32,
    /// Water flow direction in degrees.
    pub flow_direction: f32,
    /// Whether players can swim in this area.
    pub is_swimmable: bool,
    /// Whether entering this water deals damage.
    pub damage_on_entry: bool,
}
