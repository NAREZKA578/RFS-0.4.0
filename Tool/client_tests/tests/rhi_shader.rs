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

#[test]
fn shader_reflection_new_and_empty() {
    use rhi::shader::reflection::{ShaderReflection, ScalarType, ShaderVariable, ShaderVariableType};
    let empty = ShaderReflection::default();
    assert!(empty.is_empty());
    assert_eq!(empty.push_constant_size(), 0);
    assert!(empty.find_resource(0, 0).is_none());

    let reflection = ShaderReflection::new(
        vec![ShaderVariable {
            name: "in_pos".into(),
            location: 0,
            ty: ShaderVariableType::Vector(ScalarType::Float, 3),
            size: 12,
            offset: 0,
        }],
        vec![],
        vec![],
        vec![],
    );
    assert!(!reflection.is_empty());
    assert_eq!(reflection.inputs.len(), 1);
    assert_eq!(reflection.inputs[0].size, 12);
}

#[test]
fn shader_reflection_find_resource() {
    use rhi::descriptor::layout::DescriptorType;
    use rhi::shader::reflection::{
        ShaderReflection, ShaderResource, ShaderResourceAccess, ShaderResourceDimensions,
    };
    let reflection = ShaderReflection::new(
        vec![],
        vec![],
        vec![ShaderResource {
            name: "ubo".into(),
            binding: 1,
            set: 0,
            ty: DescriptorType::UniformBuffer,
            access: ShaderResourceAccess::Read,
            dimensions: ShaderResourceDimensions::Buffer,
            format: None,
        }],
        vec![],
    );
    assert!(reflection.find_resource(0, 1).is_some());
    assert!(reflection.find_resource(0, 2).is_none());
    assert!(reflection.find_resource(1, 1).is_none());
}

#[test]
fn shader_reflection_push_constants() {
    use rhi::shader::reflection::{ShaderPushConstant, ShaderReflection};
    let reflection = ShaderReflection::new(
        vec![],
        vec![],
        vec![],
        vec![
            ShaderPushConstant {
                name: "a".into(),
                offset: 0,
                size: 16,
                stages: rhi::ShaderStage::VERTEX,
            },
            ShaderPushConstant {
                name: "b".into(),
                offset: 16,
                size: 16,
                stages: rhi::ShaderStage::FRAGMENT,
            },
        ],
    );
    assert_eq!(reflection.push_constant_size(), 32);
}

#[test]
fn stub_compiler_compiles_and_roundtrips() {
    use rhi::shader::compiler::{ShaderCompiler, StubShaderCompiler};
    let compiler = StubShaderCompiler;
    let desc = ShaderModuleDesc {
        code: vec![1, 2, 3],
        format: ShaderFormat::SpirV,
        entry_point: Some("main".into()),
        name: Some("vs".into()),
    };
    let module = compiler.compile(&desc).unwrap();
    assert_eq!(module.desc().code, vec![1, 2, 3]);

    let spirv = compiler
        .compile_to_spirv("void main(){}", rhi::ShaderStage::VERTEX)
        .unwrap();
    assert_eq!(spirv, b"void main(){}");

    let glsl = compiler
        .compile_to_glsl(&spirv, rhi::ShaderStage::VERTEX)
        .unwrap();
    assert_eq!(glsl, "void main(){}");

    let reflection = compiler.reflect(&[], ShaderFormat::SpirV).unwrap();
    assert!(reflection.is_empty());
}

#[test]
fn compile_options_defaults() {
    use rhi::shader::compiler::{CompileOptions, OptimizationLevel, ShaderTargetEnv};
    let opts = CompileOptions::default();
    assert_eq!(opts.optimization_level, OptimizationLevel::None);
    assert_eq!(opts.target_env, ShaderTargetEnv::Vulkan);
    assert!(!opts.generate_debug_info);
}