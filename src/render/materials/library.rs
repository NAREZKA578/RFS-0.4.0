//! Material Library
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Manages loading, storing, and retrieving materials.

use super::{Material, PbrMaterial, WaterMaterial};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Material library for managing materials
pub struct MaterialLibrary {
    /// Map of material names to materials
    materials: HashMap<String, Material>,
    /// Map of material names to PBR materials
    pbr_materials: HashMap<String, PbrMaterial>,
    /// Map of material names to water materials
    water_materials: HashMap<String, WaterMaterial>,
    /// Device reference
    device: Option<Arc<crate::rhi::Device>>,
}

impl MaterialLibrary {
    /// Create a new material library
    pub fn new() -> Self {
        Self {
            materials: HashMap::new(),
            pbr_materials: HashMap::new(),
            water_materials: HashMap::new(),
            device: None,
        }
    }

    /// Set the device (needed for creating pipelines)
    pub fn set_device(&mut self, device: Arc<crate::rhi::Device>) {
        self.device = Some(device);
    }

    /// Add a material to the library
    pub fn add(&mut self, material: Material) {
        self.materials.insert(material.name.clone(), material);
    }

    /// Add a PBR material to the library
    pub fn add_pbr(&mut self, material: PbrMaterial) {
        self.pbr_materials
            .insert(material.base.name.clone(), material);
    }

    /// Add a water material to the library
    pub fn add_water(&mut self, material: WaterMaterial) {
        self.water_materials
            .insert(material.base.name.clone(), material);
    }

    /// Get a material by name
    pub fn get(&self, name: &str) -> Option<&Material> {
        self.materials.get(name)
    }

    /// Get a mutable material by name
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Material> {
        self.materials.get_mut(name)
    }

    /// Get a PBR material by name
    pub fn get_pbr(&self, name: &str) -> Option<&PbrMaterial> {
        self.pbr_materials.get(name)
    }

    /// Get a mutable PBR material by name
    pub fn get_pbr_mut(&mut self, name: &str) -> Option<&mut PbrMaterial> {
        self.pbr_materials.get_mut(name)
    }

    /// Get a water material by name
    pub fn get_water(&self, name: &str) -> Option<&WaterMaterial> {
        self.water_materials.get(name)
    }

    /// Get a mutable water material by name
    pub fn get_water_mut(&mut self, name: &str) -> Option<&mut WaterMaterial> {
        self.water_materials.get_mut(name)
    }

    /// Remove a material by name
    pub fn remove(&mut self, name: &str) -> Option<Material> {
        self.materials.remove(name)
    }

    /// Remove a PBR material by name
    pub fn remove_pbr(&mut self, name: &str) -> Option<PbrMaterial> {
        self.pbr_materials.remove(name)
    }

    /// Remove a water material by name
    pub fn remove_water(&mut self, name: &str) -> Option<WaterMaterial> {
        self.water_materials.remove(name)
    }

    /// Create pipelines for all materials
    pub fn create_pipelines(&mut self) {
        if let Some(device) = &self.device {
            for material in self.materials.values_mut() {
                material.create_pipeline(device);
            }
            for material in self.pbr_materials.values_mut() {
                material.create_pipeline(device);
            }
            for material in self.water_materials.values_mut() {
                material.create_pipeline(device);
            }
        }
    }

    /// Load materials from a directory
    pub fn load_from_directory(&mut self, _path: &Path) {
        // In the actual implementation, this would:
        // 1. Scan the directory for material definition files
        // 2. Parse the files (JSON, YAML, etc.)
        // 3. Load textures referenced by the materials
        // 4. Create the materials

        // For now, this is a placeholder
    }

    /// Create a default PBR material
    pub fn create_default_pbr(&mut self, name: &str) -> &PbrMaterial {
        let material = PbrMaterial::new(name);
        self.add_pbr(material);
        self.pbr_materials.get(name).unwrap()
    }

    /// Create a default water material
    pub fn create_default_water(&mut self, name: &str) -> &WaterMaterial {
        let material = WaterMaterial::new(name);
        self.add_water(material);
        self.water_materials.get(name).unwrap()
    }

    /// Create common material presets
    pub fn create_presets(&mut self) {
        // Metal materials
        self.add_pbr(PbrMaterial::metal("metal_gold", [1.0, 0.76, 0.33, 1.0]));
        self.add_pbr(PbrMaterial::metal("metal_iron", [0.56, 0.57, 0.58, 1.0]));
        self.add_pbr(PbrMaterial::metal("metal_steel", [0.7, 0.75, 0.8, 1.0]));

        // Plastic materials
        self.add_pbr(PbrMaterial::plastic("plastic_red", [0.8, 0.1, 0.1, 1.0]));
        self.add_pbr(PbrMaterial::plastic("plastic_blue", [0.1, 0.1, 0.8, 1.0]));
        self.add_pbr(PbrMaterial::plastic("plastic_white", [0.9, 0.9, 0.9, 1.0]));

        // Wood materials
        self.add_pbr(PbrMaterial::wood("wood_oak", [0.6, 0.4, 0.2, 1.0]));
        self.add_pbr(PbrMaterial::wood("wood_pine", [0.7, 0.5, 0.3, 1.0]));

        // Glass
        self.add_pbr(PbrMaterial::glass("glass_clear"));

        // Water materials
        self.add_water(WaterMaterial::ocean("water_ocean"));
        self.add_water(WaterMaterial::lake("water_lake"));
        self.add_water(WaterMaterial::river("water_river"));
    }

    /// Clean up all materials
    pub fn cleanup(&mut self) {
        for material in self.materials.values_mut() {
            material.cleanup();
        }
        self.materials.clear();
        self.pbr_materials.clear();
        self.water_materials.clear();
    }

    /// Get material count
    pub fn material_count(&self) -> usize {
        self.materials.len() + self.pbr_materials.len() + self.water_materials.len()
    }

    /// Get all material names
    pub fn material_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.materials.keys().cloned().collect();
        names.extend(self.pbr_materials.keys().cloned());
        names.extend(self.water_materials.keys().cloned());
        names
    }
}

impl Default for MaterialLibrary {
    fn default() -> Self {
        Self::new()
    }
}
