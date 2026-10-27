// Integration tests for indexed graphics draws and depth-buffer attachment
// handling in the D3D11 backend.
//
// A single indexed draw submits a big far quad (red, z = 0.75) followed by a
// small near quad (green, z = 0.25) as separate triangles in one index buffer
// with no vertex buffers at all: the vertex shader pulls SV_VertexID. The
// depth attachment must keep the near quad visible where the two overlap and
// let the far quad paint the surrounding ring, proving `cmd_draw_indexed`,
// the depth-stencil wiring, and the no-input-layout path a signature-less
// vertex shader needs.

use rhi::backend::d3d11::compile::compile_hlsl;
use rhi::backend::d3d11::D3D11Backend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::pipeline::graphics::PrimitiveTopology;
use rhi::shader::module::{ShaderModule, ShaderModuleDesc, ShaderFormat};
use rhi::types::{ClearValue, Format, ShaderStage};
use rhi::{
    AttachmentDescription, AttachmentReference, Backend, ColorBlendAttachment, ColorBlendState,
    ColorComponentFlags, ColorTargetDesc, DepthStencilState, FramebufferAttachment,
    FramebufferDesc, GraphicsPipelineDesc, IndexType, InputAssemblyState, LoadOp,
    MultisampleState, PipelineBindPoint, PipelineShaderStage, RasterizerState, RenderPassDesc,
    SampleCount, StoreOp, SubpassDescription, TextureLayout, TextureUsage, BufferUsage,
};

const VS_SRC: &[u8] = b"struct VsOut {\
      float4 pos : SV_Position; \
      float4 color : COLOR; \
    };\
    static const float2 far_q[4] = {\
      float2(-0.8, -0.8), float2( 0.8, -0.8),\
      float2( 0.8,  0.8), float2(-0.8,  0.8)\
    };\
    static const float2 near_q[4] = {\
      float2(-0.3, -0.3), float2( 0.3, -0.3),\
      float2( 0.3,  0.3), float2(-0.3,  0.3)\
    };\
    VsOut main(uint vid : SV_VertexID) {\
      VsOut o;\
      if (vid < 4u) {\
        o.pos = float4(far_q[vid], 0.75, 1.0);\
        o.color = float4(1, 0, 0, 1);\
      } else {\
        o.pos = float4(near_q[vid - 4u], 0.25, 1.0);\
        o.color = float4(0, 1, 0, 1);\
      }\
      return o;\
    }";

const PS_SRC: &[u8] = b"struct PsIn {\
      float4 pos : SV_Position; \
      float4 color : COLOR; \
    };\
    float4 main(PsIn i) : SV_Target {\
      return i.color;\
    }";

fn open_d3d11_device() -> Device {
    let backend = D3D11Backend::new(&RhiConfig::default()).unwrap();
    let devices = backend.enumerate_physical_devices().unwrap();
    backend.create_device(&devices[0], &DeviceDesc::default()).unwrap()
}

/// Compiles HLSL source with the runtime compiler and hands the finished
/// container to the backend under `ShaderFormat::Dxil`, matching how the
/// D3D11 path is meant to receive shader bytes.
fn dx_module(device: &Device, code: &[u8], target: &str, name: &str) -> ShaderModule {
    let container = compile_hlsl(code, "main", target)
        .expect("the HLSL compile must succeed for the draw to mean anything");
    device
        .create_shader_module(&ShaderModuleDesc {
            code: container,
            format: ShaderFormat::Dxil,
            entry_point: Some("main".into()),
            name: Some(name.into()),
        })
        .expect("the shader module must be created for the draw to mean anything")
}

fn all_channels() -> ColorComponentFlags {
    ColorComponentFlags::R
        | ColorComponentFlags::G
        | ColorComponentFlags::B
        | ColorComponentFlags::A
}

#[test]
fn d3d11_offscreen_indexed_depth_roundtrip() {
    let device = open_d3d11_device();

    const EXTENT: u32 = 64;
    let color = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the colour attachment must be created");
    assert!(color.has_gpu_backing());
    let color_view = device
        .create_texture_view(&color, color.create_default_view().desc())
        .expect("color attachment view");

    let depth = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::D32_SFLOAT,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT,
            1,
        )
        .expect("the depth attachment must be created");
    assert!(depth.has_gpu_backing());
    let depth_view = device
        .create_texture_view(&depth, depth.create_default_view().desc())
        .expect("depth attachment view");

    let render_pass = device
        .create_gpu_render_pass(&RenderPassDesc {
            attachments: vec![
                AttachmentDescription {
                    format: Format::RGBA8_UNORM,
                    samples: SampleCount::X1,
                    load_op: LoadOp::Clear,
                    store_op: StoreOp::Store,
                    stencil_load_op: LoadOp::DontCare,
                    stencil_store_op: StoreOp::DontCare,
                    initial_layout: TextureLayout::Undefined,
                    final_layout: TextureLayout::ColorAttachmentOptimal,
                },
                AttachmentDescription {
                    format: Format::D32_SFLOAT,
                    samples: SampleCount::X1,
                    load_op: LoadOp::Clear,
                    store_op: StoreOp::DontCare,
                    stencil_load_op: LoadOp::DontCare,
                    stencil_store_op: StoreOp::DontCare,
                    initial_layout: TextureLayout::Undefined,
                    final_layout: TextureLayout::DepthAttachmentOptimal,
                },
            ],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: TextureLayout::ColorAttachmentOptimal,
                }],
                depth_stencil_attachment: Some(AttachmentReference {
                    attachment: 1,
                    layout: TextureLayout::DepthAttachmentOptimal,
                }),
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");
    assert!(render_pass.has_gpu_backing());

    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![
                FramebufferAttachment {
                    texture_view: color_view,
                    layer: 0,
                    mip_level: 0,
                },
                FramebufferAttachment {
                    texture_view: depth_view,
                    layer: 0,
                    mip_level: 0,
                },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");
    assert!(framebuffer.has_gpu_backing());

    let vs = dx_module(&device, VS_SRC, "vs_5_0", "indexed/depth vs");
    let ps = dx_module(&device, PS_SRC, "ps_5_0", "indexed/depth frag");
    assert!(vs.has_gpu_backing());
    assert!(ps.has_gpu_backing());

    let pipeline = device
        .create_gpu_graphics_pipeline(
            &GraphicsPipelineDesc {
                shader_stages: vec![
                    PipelineShaderStage {
                        stage: ShaderStage::VERTEX,
                        module: vs,
                        entry_point: "main".into(),
                    },
                    PipelineShaderStage {
                        stage: ShaderStage::FRAGMENT,
                        module: ps,
                        entry_point: "main".into(),
                    },
                ],
                input_assembly_state: InputAssemblyState {
                    topology: PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                rasterizer_state: Some(RasterizerState::double_sided()),
                multisample_state: MultisampleState {
                    sample_count: SampleCount::X1,
                    ..Default::default()
                },
                depth_stencil_state: Some(DepthStencilState::enabled()),
                color_blend_state: Some(ColorBlendState {
                    attachments: vec![ColorBlendAttachment {
                        color_write_mask: all_channels(),
                        ..ColorTargetDesc::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            &render_pass,
            &[],
        )
        .expect("graphics pipeline");
    assert!(pipeline.has_gpu_backing());

    let indices: [u16; 12] = [0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4];
    let mut index_bytes = Vec::with_capacity(indices.len() * 2);
    for index in &indices {
        index_bytes.extend_from_slice(&index.to_le_bytes());
    }
    let index_buffer = device
        .create_buffer(
            index_bytes.len() as u64,
            BufferUsage::INDEX | BufferUsage::TRANSFER_DST,
            true,
        )
        .expect("the index buffer must be created");
    device
        .upload_buffer(&index_buffer, &index_bytes)
        .expect("the index buffer must be uploaded before the indexed draw");

    device
        .draw_framebuffer_indexed(
            &framebuffer,
            &pipeline,
            &[],
            None,
            Some((&index_buffer, 0)),
            IndexType::U16,
            8,
            0,
            12,
            0,
            0,
            &[
                ClearValue::color(0.0, 0.0, 1.0, 1.0),
                ClearValue::DepthStencil { depth: 1.0, stencil: 0 },
            ],
        )
        .expect("indexed draw framebuffer");

    let bytes = device.download_texture(&color, 0, 0).expect("readback");
    assert_eq!(bytes.len(), (EXTENT * EXTENT * 4) as usize);

    let pixel = |x: u32, y: u32| -> [u8; 4] {
        let offset = ((y * EXTENT + x) * 4) as usize;
        [
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]
    };

    let center = pixel(EXTENT / 2, EXTENT / 2);
    assert!(
        center[1] > 200 && center[0] < 50 && center[2] < 50,
        "expected the near quad to win the depth test at the center, got {center:?}"
    );

    let corner = pixel(10, 32);
    assert!(
        corner[0] > 200 && corner[1] < 50 && corner[2] < 50,
        "expected the far quad to paint the surrounding ring, got {corner:?}"
    );
}
