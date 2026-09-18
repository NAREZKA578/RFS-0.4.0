// Integration tests for the rhi::shader module.
//
// Covers: ShaderFormat, ShaderModule, ShaderLibrary, stage constants.

use rhi::shader::stage;
use rhi::shader::library::{ShaderLibrary, ShaderLibraryDesc};
use rhi::shader::module::{ShaderFormat, ShaderModule, ShaderModuleDesc};

#[test]
fn shader_format_variants() {
    assert_eq!(ShaderFormat::default(), ShaderFormat::SpirV);
    let _ = ShaderFormat::Dxil;
    let _ = ShaderFormat::Msl;
    let _ = ShaderFormat::Wgsl;
    let _ = ShaderFormat::Glsl;
    let _ = ShaderFormat::GlslEs;
}

#[test]
fn shader_module_desc_creation() {
    let desc = ShaderModuleDesc {
        code: vec![0x03, 0x02, 0x23, 0x07],
        format: ShaderFormat::SpirV,
        entry_point: Some("main".to_string()),
        name: Some("my_shader".to_string()),
    };
    assert_eq!(desc.format, ShaderFormat::SpirV);
    assert_eq!(desc.entry_point.as_deref(), Some("main"));
}

#[test]
fn shader_module_new_and_desc() {
    let desc = ShaderModuleDesc {
        code: vec![1, 2, 3],
        format: ShaderFormat::SpirV,
        entry_point: Some("main".to_string()),
        name: Some("vs_main".to_string()),
    };
    let module = ShaderModule::new(desc);
    assert_eq!(module.desc().name.as_deref(), Some("vs_main"));
    assert_eq!(module.desc().code.len(), 3);
}

#[test]
fn shader_module_default() {
    let module = ShaderModule::default();
    assert!(module.desc().code.is_empty());
    assert!(module.desc().name.is_none());
}

#[test]
fn shader_library_new_get_none() {
    let lib = ShaderLibrary::new(ShaderLibraryDesc::default());
    assert!(lib.get_module("missing").is_none());
}

#[test]
fn shader_library_add_and_get() {
    let mut lib = ShaderLibrary::new(ShaderLibraryDesc::default());
    let module = ShaderModule::new(ShaderModuleDesc {
        code: vec![9],
        format: ShaderFormat::SpirV,
        entry_point: Some("main".to_string()),
        name: Some("vs".to_string()),
    });
    lib.add_module(module);

    let found = lib.get_module("vs");
    assert!(found.is_some());
    assert_eq!(found.unwrap().desc().format, ShaderFormat::SpirV);
    assert!(lib.get_module("fs").is_none());
}

#[test]
fn shader_library_prepopulated_desc() {
    let desc = ShaderLibraryDesc {
        modules: vec![
            ShaderModuleDesc {
                code: vec![],
                format: ShaderFormat::Glsl,
                entry_point: Some("main".to_string()),
                name: Some("pre".to_string()),
            },
        ],
        name: "lib".to_string(),
    };
    let lib = ShaderLibrary::new(desc);
    // Prepopulated modules are NOT imported into get_module lookup (separate vec).
    assert!(lib.get_module("pre").is_none());
}

#[test]
fn shader_stage_constants_are_type() {
    let v = stage::VERTEX_SHADER;
    let f = stage::FRAGMENT_SHADER;
    assert!(v.contains(rhi::ShaderStage::VERTEX));
    assert!(f.contains(rhi::ShaderStage::FRAGMENT));
    let _ = stage::COMPUTE_SHADER;
    let _ = stage::RAY_GEN_SHADER;
    let _ = stage::ANY_HIT_SHADER;
    let _ = stage::CLOSEST_HIT_SHADER;
    let _ = stage::MISS_SHADER;
    let _ = stage::INTERSECTION_SHADER;
}