//! Integration tests for the render::materials module.
//!
//! Covers: Material, MaterialBuilder, MaterialType, BlendMode, CullMode,
//! PbrMaterial, PbrMaterialBuilder, WaterMaterial, WaterMaterialBuilder,
//! MaterialLibrary.

use rfs_client::render::materials::library::MaterialLibrary;
use rfs_client::render::materials::material::MaterialBuilder;
use rfs_client::render::materials::pbr::PbrMaterialBuilder;
use rfs_client::render::materials::water::WaterMaterialBuilder;
use rfs_client::render::materials::{
    BlendMode, CullMode, Material, MaterialType, PbrMaterial, WaterMaterial,
};

#[test]
fn material_type_default_is_pbr() {
    assert_eq!(MaterialType::default(), MaterialType::Pbr);
}

#[test]
fn blend_and_cull_mode_defaults() {
    assert_eq!(BlendMode::default(), BlendMode::Opaque);
    assert_eq!(CullMode::default(), CullMode::Back);
}

#[test]
fn material_creation_defaults() {
    let material = Material::new("iron", MaterialType::Pbr);
    assert_eq!(material.name, "iron");
    assert_eq!(material.material_type, MaterialType::Pbr);
    assert_eq!(material.blend_mode, BlendMode::Opaque);
    assert_eq!(material.cull_mode, CullMode::Back);
    assert!(material.depth_test);
    assert!(material.depth_write);
    assert!(!material.two_sided);
    assert!(!material.wireframe);
}

#[test]
fn material_parameter_roundtrip() {
    let mut material = Material::new("tinted", MaterialType::Custom);
    material.set_float("intensity", 0.75);
    material.set_vector("offset", [1.0, 2.0, 3.0, 4.0]);
    material.set_color("tint", [0.1, 0.2, 0.3, 0.4]);

    assert_eq!(material.get_float("intensity"), Some(0.75));
    assert_eq!(material.get_vector("offset"), Some([1.0, 2.0, 3.0, 4.0]));
    assert_eq!(material.get_color("tint"), Some([0.1, 0.2, 0.3, 0.4]));
    assert_eq!(material.get_float("missing"), None);
}

#[test]
fn material_builder_builds_material() {
    let material = MaterialBuilder::new("builder", MaterialType::Pbr)
        .blend_mode(BlendMode::Additive)
        .cull_mode(CullMode::None)
        .depth_test(false)
        .depth_write(false)
        .two_sided(true)
        .wireframe(true)
        .float("roughness", 0.3)
        .color("tint", [1.0, 0.0, 0.0, 1.0])
        .build();

    assert_eq!(material.name, "builder");
    assert_eq!(material.blend_mode, BlendMode::Additive);
    assert_eq!(material.cull_mode, CullMode::None);
    assert!(!material.depth_test);
    assert!(!material.depth_write);
    assert!(material.two_sided);
    assert!(material.wireframe);
    assert_eq!(material.get_float("roughness"), Some(0.3));
    assert_eq!(material.get_color("tint"), Some([1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn pbr_material_defaults() {
    let material = PbrMaterial::new("steel");
    assert_eq!(material.base.name, "steel");
    assert_eq!(material.base.material_type, MaterialType::Pbr);
    assert_eq!(material.base.blend_mode, BlendMode::Opaque);
    assert_eq!(material.albedo, [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(material.roughness, 0.5);
    assert_eq!(material.metallic, 0.0);
    assert_eq!(material.ao, 1.0);
    assert_eq!(material.emissive, [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn pbr_material_setters_and_clamping() {
    let mut material = PbrMaterial::new("tuned");
    material.set_albedo([0.2, 0.3, 0.4, 1.0]);
    assert_eq!(material.albedo, [0.2, 0.3, 0.4, 1.0]);

    material.set_roughness(2.0);
    assert_eq!(material.roughness, 1.0, "roughness must clamp to [0, 1]");

    material.set_metallic(-1.0);
    assert_eq!(material.metallic, 0.0, "metallic must clamp to [0, 1]");

    material.set_blend_mode(BlendMode::Additive);
    assert_eq!(material.base.blend_mode, BlendMode::Additive);

    material.set_cull_mode(CullMode::Front);
    assert_eq!(material.base.cull_mode, CullMode::Front);
}

#[test]
fn pbr_material_presets() {
    let metal = PbrMaterial::metal("metal_gold", [1.0, 0.76, 0.33, 1.0]);
    assert_eq!(metal.metallic, 1.0);
    assert_eq!(metal.roughness, 0.1);

    let plastic = PbrMaterial::plastic("plastic_red", [0.8, 0.1, 0.1, 1.0]);
    assert_eq!(plastic.metallic, 0.0);
    assert_eq!(plastic.roughness, 0.3);

    let wood = PbrMaterial::wood("wood_oak", [0.6, 0.4, 0.2, 1.0]);
    assert_eq!(wood.roughness, 0.7);

    let glass = PbrMaterial::glass("glass_clear");
    assert_eq!(glass.base.blend_mode, BlendMode::Alpha);
    assert_eq!(glass.base.cull_mode, CullMode::None);

    let water = PbrMaterial::water("pbr_water");
    assert_eq!(water.base.blend_mode, BlendMode::Alpha);
}

#[test]
fn pbr_material_builder() {
    let material = PbrMaterialBuilder::new("brownstone")
        .albedo([0.5, 0.35, 0.2, 1.0])
        .roughness(0.8)
        .metallic(0.2)
        .ao(0.9)
        .emissive([0.0, 0.0, 0.0, 1.0])
        .blend_mode(BlendMode::Opaque)
        .cull_mode(CullMode::Back)
        .build();

    assert_eq!(material.albedo, [0.5, 0.35, 0.2, 1.0]);
    assert_eq!(material.roughness, 0.8);
    assert_eq!(material.metallic, 0.2);
    assert_eq!(material.ao, 0.9);
}

#[test]
fn water_material_builder() {
    let water = WaterMaterialBuilder::new("bay")
        .color([0.1, 0.4, 0.6, 1.0])
        .deep_color([0.0, 0.3, 0.5, 1.0])
        .shallow_color([0.2, 0.6, 0.7, 1.0])
        .depth(10.0)
        .clarity(1.0)
        .fresnel(0.5, 0.2, 5.0)
        .build();
    assert_eq!(water.base.name, "bay");
    assert_eq!(water.base.material_type, MaterialType::Water);
}

#[test]
fn water_material_presets_construct() {
    let ocean = WaterMaterial::ocean("water_ocean");
    let lake = WaterMaterial::lake("water_lake");
    let river = WaterMaterial::river("water_river");
    assert_eq!(ocean.base.name, "water_ocean");
    assert_eq!(lake.base.name, "water_lake");
    assert_eq!(river.base.name, "water_river");
}

#[test]
fn material_library_crud() {
    let mut library = MaterialLibrary::new();
    assert_eq!(library.material_count(), 0);

    library.add(Material::new("base", MaterialType::Unlit));
    library.add_pbr(PbrMaterial::new("pbr"));
    library.add_water(WaterMaterial::new("water"));
    assert_eq!(library.material_count(), 3);

    assert_eq!(library.get("base").unwrap().name, "base");
    assert_eq!(library.get_pbr("pbr").unwrap().base.name, "pbr");
    assert_eq!(library.get_water("water").unwrap().base.name, "water");
    assert!(library.get("missing").is_none());

    let names = library.material_names();
    assert!(names.contains(&"base".to_string()));
    assert!(names.contains(&"pbr".to_string()));
    assert!(names.contains(&"water".to_string()));

    assert!(library.remove("base").is_some());
    assert!(library.remove_pbr("pbr").is_some());
    assert!(library.remove_water("water").is_some());
    assert_eq!(library.material_count(), 0);
}

#[test]
fn material_library_get_mut() {
    let mut library = MaterialLibrary::new();
    library.add(Material::new("editable", MaterialType::Pbr));
    // The library keys materials by name at insert time; mutating the
    // returned material's .name field does NOT re-key the internal map.
    {
        let m = library.get_mut("editable").unwrap();
        m.two_sided = true;
        m.wireframe = true;
        m.name = "renamed".to_string();
    }
    assert!(library.get("editable").unwrap().two_sided);
    assert!(library.get("editable").unwrap().wireframe);
    assert!(library.get("renamed").is_none());
}

#[test]
fn material_library_default_creators() {
    let mut library = MaterialLibrary::new();
    library.create_default_pbr("default_pbr");
    library.create_default_water("default_water");
    assert_eq!(library.material_count(), 2);
    assert!(library.get_pbr("default_pbr").is_some());
    assert!(library.get_water("default_water").is_some());
}

#[test]
fn material_library_create_presets() {
    let mut library = MaterialLibrary::new();
    library.create_presets();
    assert_eq!(library.material_count(), 12);
    let names = library.material_names();
    assert!(names.contains(&"metal_steel".to_string()));
    assert!(names.contains(&"plastic_white".to_string()));
    assert!(names.contains(&"glass_clear".to_string()));
    assert!(names.contains(&"water_ocean".to_string()));
}