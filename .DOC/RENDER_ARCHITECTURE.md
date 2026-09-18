# Render Module Architecture

> **Project:** RFS-0.4.0  
> **Module:** `src/render/` (Client-Side Only)  
> **Status:** Architecture Complete, Ready for Implementation  
> **NOT CONNECTED TO SERVER CODE**

---

## 📁 Module Structure

```
src/render/
├── mod.rs                          # Main module exports
├── core/                           # Core rendering system
│   ├── mod.rs
│   ├── renderer.rs                 # Main renderer
│   ├── context.rs                  # Render context
│   └── settings.rs                 # Graphics settings
│
├── graph/                          # Render Graph system
│   ├── mod.rs
│   ├── graph.rs                    # Render graph manager
│   ├── node.rs                     # Render pass nodes
│   ├── resource.rs                 # Graph resources
│   └── types.rs                    # Shared types (ResourceUsage)
│
├── passes/                         # Render passes
│   ├── mod.rs
│   ├── base.rs                     # Base render pass trait
│   ├── gbuffer.rs                  # GBuffer pass (Deferred)
│   ├── shadow.rs                   # Shadow mapping
│   ├── lighting.rs                 # PBR lighting
│   ├── transparent.rs              # Transparent objects
│   ├── water.rs                    # Water rendering
│   └── ui.rs                       # UI rendering
│
├── materials/                      # Material system
│   ├── mod.rs
│   ├── material.rs                 # Base material
│   ├── pbr.rs                      # PBR material
│   ├── water.rs                    # Water material
│   └── library.rs                  # Material library
│
├── meshes/                         # Mesh system
│   ├── mod.rs
│   ├── mesh.rs                     # Mesh definition
│   ├── loader.rs                   # Mesh loader
│   └── library.rs                  # Mesh library
│
├── textures/                       # Texture system
│   ├── mod.rs
│   ├── texture.rs                  # Texture definition
│   ├── loader.rs                   # Texture loader
│   └── library.rs                  # Texture library
│
├── scene/                          # Scene management
│   ├── mod.rs
│   ├── scene.rs                    # Scene
│   ├── entity.rs                   # Entity
│   ├── components.rs               # ECS components
│   └── culling.rs                  # Frustum/Occlusion culling
│
├── camera/                         # Camera system
│   ├── mod.rs
│   ├── camera.rs                   # Camera trait + implementations
│   └── controller.rs               # Camera controller
│
├── lighting/                       # Lighting system
│   ├── mod.rs
│   ├── light.rs                    # Light types
│   ├── shadows.rs                  # Shadow mapping
│   └── probe.rs                    # Light probes (IBL)
│
├── postprocess/                    # Post-processing effects
│   ├── mod.rs
│   ├── bloom.rs                    # Bloom effect
│   ├── motion_blur.rs              # Motion blur
│   ├── dof.rs                      # Depth of Field
│   ├── hdr.rs                      # HDR + Tone Mapping
│   └── fxaa.rs                     # FXAA anti-aliasing
│
├── particles/                      # Particle system
│   ├── mod.rs
│   ├── particle.rs                 # Particle struct
│   ├── system.rs                   # Particle system
│   ├── emitter.rs                  # Particle emitter
│   └── effects/                    # Pre-configured effects
│       ├── mod.rs
│       └── effects.rs              # Smoke, Fire, Splash, etc.
│
├── water/                          # Water rendering
│   ├── mod.rs
│   ├── surface.rs                  # Water surface
│   ├── waves.rs                    # Wave simulation
│   └── interaction.rs              # Object interaction
│
└── ui/                             # UI system
    ├── mod.rs
    ├── hud.rs                      # HUD elements
    ├── minimap.rs                  # Minimap
    └── menu.rs                     # Menu system
```

---

## 🎯 Architecture Overview

### Rendering Pipeline

The render module uses a **RenderGraph** approach with the following passes:

```
Scene Setup
    ↓
[GBuffer Pass] → Position, Normal, Albedo, Material buffers
    ↓
[Shadow Pass] → Cascaded shadow maps
    ↓
[Lighting Pass] → PBR lighting + SSAO + SSR
    ↓
[Water Pass] → Water surface with reflections/refractions
    ↓
[Transparent Pass] → Transparent objects (sorted)
    ↓
[PostProcess Pass] → Bloom, HDR, FXAA, Motion Blur, DoF
    ↓
[UI Pass] → 2D interface (HUD, Menu)
    ↓
Swapchain Present
```

### Key Features

1. **Deferred Rendering** - Main rendering approach with GBuffer
2. **PBR Materials** - Physically Based Rendering support
3. **Cascaded Shadow Maps** - High-quality shadows for directional lights
4. **GPU-Driven Particles** - Efficient particle system
5. **Dynamic Water** - Waves, reflections, refractions, foam
6. **Screen Space Effects** - SSAO, SSR
7. **LOD System** - Level of Detail for performance
8. **Frustum/Occlusion Culling** - Optimized rendering
9. **Instanced Rendering** - Efficient batch rendering
10. **Ray Tracing Support** - Optional (if hardware supports)

---

## 🏗 Core Components

### Renderer (`core/renderer.rs`)

Main renderer class that:
- Manages the RHI device and swapchain
- Initializes render passes
- Executes the render graph
- Handles resizing
- Manages settings

```rust
pub struct Renderer {
    device: Arc<Device>,
    swapchain: Swapchain,
    render_graph: RenderGraph,
    context: RenderContext,
    settings: RendererSettings,
    // ...
}
```

### RenderContext (`core/context.rs`)

Contains all RHI resources needed for rendering:
- Device and queues
- Swapchain
- Depth texture
- Current resolution
- Frame timing
- Settings

### RendererSettings (`core/settings.rs`)

Runtime-configurable graphics settings:
- Quality presets (Low/Medium/High/Ultra/Custom)
- Shadow quality
- Anti-aliasing mode
- Ray tracing settings
- Post-processing settings
- Water settings
- Particle settings
- LOD settings

---

## 📊 RenderGraph System

### Graph Structure

The render graph manages:
- **Nodes** - Render passes with dependencies
- **Resources** - Textures, buffers used by passes
- **Execution Order** - Topologically sorted

### Node Types

- `RenderPassNode` - Wrapper around a render pass
- `PassDependency` - Dependency between passes
- `GraphResource` - Resource managed by the graph

### Resource Types

- `Texture` - 2D, 3D, Cube, Array
- `Buffer` - Vertex, Index, Uniform, Storage
- `Sampler` - Texture sampler

### ResourceUsage

- `RenderTarget` - Written to
- `ColorAttachment` - Color buffer
- `DepthStencil` - Depth/stencil buffer
- `Sampled` - Read from (texture sampling)
- `Storage` - Read/write (compute shaders)

---

## 🎨 Render Passes

### GBuffer Pass (`passes/gbuffer.rs`)

Renders scene geometry to multiple render targets:
- **Position** (RGBA16Float) - XYZ + depth
- **Normal** (RGBA16Float) - XYZ normal
- **Albedo** (RGBA8) - RGB color + roughness (A)
- **Material** (RGBA8) - Metallic (R), AO (G), Emissive (B)

### Shadow Pass (`passes/shadow.rs`)

Renders scene from light's perspective:
- **Directional Lights** - Cascaded shadow maps (1-4 cascades)
- **Point Lights** - Omnidirectional shadow maps (cube maps)
- **Spot Lights** - Perspective shadow maps

Configuration:
- Shadow resolution (512, 1024, 2048, 4096)
- Cascade distances
- Shadow bias

### Lighting Pass (`passes/lighting.rs`)

Computes lighting using GBuffer data:
- **PBR Lighting** - Physically Based Rendering
- **Multiple Light Types** - Directional, Point, Spot
- **IBL** - Image-Based Lighting (optional)
- **SSAO** - Screen Space Ambient Occlusion (optional)
- **SSR** - Screen Space Reflections (optional)

### Transparent Pass (`passes/transparent.rs`)

Renders transparent objects with proper blending:
- **Depth Sorting** - Back-to-front or front-to-back
- **Blend Modes** - Alpha, Additive, Multiplicative, Screen
- **Transparent Objects** - Water, Smoke, Fire, Glass

### Water Pass (`passes/water.rs`)

Specialized pass for water rendering:
- **Reflections** - Planar or cube map
- **Refractions** - Underwater view
- **Waves** - Gerstner or FFT-based
- **Foam** - Based on depth and velocity
- **Fresnel** - Edge reflection intensity

### UI Pass (`passes/ui.rs`)

Renders 2D interface:
- **Orthographic Projection** - Screen-space coordinates
- **HUD** - Crosshair, ship info, weapon info
- **Menu** - Main menu, settings, etc.
- **Minimap** - Battlefield overview

---

## 🎭 Material System

### Base Material (`materials/material.rs`)

```rust
pub struct Material {
    name: String,
    material_type: MaterialType,
    shader: Option<Arc<ShaderModule>>,
    pipeline: Option<Pipeline>,
    parameters: MaterialParameters,
    blend_mode: BlendMode,
    cull_mode: CullMode,
    // ...
}
```

### PBR Material (`materials/pbr.rs`)

Physically Based Rendering material:
- **Albedo** - Base color
- **Normal Map** - Surface normals
- **Roughness Map** - Surface roughness
- **Metallic Map** - Metallic values
- **AO Map** - Ambient occlusion
- **Emissive Map** - Emissive color

### Water Material (`materials/water.rs`)

Specialized water material:
- **Wave Parameters** - Speed, scale, height
- **Color Parameters** - Base, deep, shallow colors
- **Foam Parameters** - Threshold, intensity
- **Fresnel Parameters** - Factor, bias, power
- **Textures** - Normal map, foam texture

### Material Library (`materials/library.rs`)

Manages material loading and caching:
- **Material Creation** - From definitions
- **Texture Loading** - Automatic texture loading
- **Presets** - Common material presets (metal, plastic, wood, glass, water)

---

## 🔧 Mesh System

### Mesh (`meshes/mesh.rs`)

```rust
pub struct Mesh {
    name: String,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    vertex_buffer: Option<Arc<Buffer>>,
    index_buffer: Option<Arc<Buffer>>,
    bounding_box: Aabb,
    // ...
}
```

### Vertex Structure

```rust
pub struct Vertex {
    position: Vec3,
    normal: Vec3,
    tangent: Vec4,
    tex_coord: Vec2,
    color: Vec4,
}
```

### Mesh Library (`meshes/library.rs`)

Manages mesh loading and caching:
- **Primitive Meshes** - Cube, Sphere, Plane, Cylinder
- **File Loading** - OBJ, glTF, FBX
- **LOD Support** - Multiple LOD levels

### Mesh Loader (`meshes/loader.rs`)

Loads meshes from files:
- **OBJ** - Wavefront format
- **glTF** - GL Transmission Format
- **FBX** - Autodesk format

---

## 🖼 Texture System

### Texture (`textures/texture.rs`)

```rust
pub struct Texture {
    name: String,
    texture_type: TextureType,
    format: TextureFormat,
    width: u32,
    height: u32,
    rhi_texture: Arc<RhiTexture>,
    default_view: Arc<RhiTextureView>,
    // ...
}
```

### Texture Types

- `Texture2D` - Standard 2D texture
- `Texture3D` - 3D volume texture
- `TextureCube` - Cube map (6 faces)
- `TextureArray` - Array of textures
- `DepthTexture` - Depth buffer
- `RenderTarget` - Render target

### Texture Formats

- `RGBA8/16/32` - Color formats
- `RGB8/16/32` - RGB formats
- `RG8/16` - Two-channel formats
- `R8/16/32` - Single-channel formats
- `Depth16/24/32` - Depth formats

### Texture Library (`textures/library.rs`)

Manages texture loading and caching:
- **File Loading** - PNG, JPG, DDS, KTX2
- **Common Textures** - White, Black, Checkerboard, Normal maps
- **Texture Atlases** - Combined textures

---

## 🌍 Scene System

### Scene (`scene/scene.rs`)

Manages all renderable entities:
- **Entity Management** - Add, remove, find entities
- **Camera Management** - Active camera, view/projection matrices
- **Culling** - Frustum and occlusion culling
- **Lights** - Light management
- **Bounding Box** - Scene AABB

### Entity (`scene/entity.rs`)

Game object with components:
- **Transform** - Position, rotation, scale
- **Renderable** - Render flags
- **Mesh** - Geometry
- **Material** - Appearance
- **Light** - Light source
- **Camera** - Camera

### Components (`scene/components.rs`)

ECS-style components:
- **Transform** - Position, rotation, scale
- **Renderable** - Opaque/transparent, shadows, LOD
- **MeshComponent** - Mesh reference + LODs
- **MaterialComponent** - Material reference
- **LightComponent** - Light properties
- **CameraComponent** - Camera reference

### Culling (`scene/culling.rs`)

Optimization techniques:
- **Frustum Culling** - Remove objects outside view frustum
- **Occlusion Culling** - Remove objects blocked by others
- **AABB** - Axis-Aligned Bounding Box

---

## 📸 Camera System

### Camera Trait (`camera/camera.rs`)

```rust
pub trait Camera {
    fn view_matrix(&self) -> Mat4;
    fn projection_matrix(&self) -> Mat4;
    fn view_projection_matrix(&self) -> Mat4;
    fn position(&self) -> Vec3;
    fn forward(&self) -> Vec3;
    fn up(&self) -> Vec3;
    // ...
}
```

### Perspective Camera

3D camera with perspective projection:
- **FOV** - Field of view
- **Aspect Ratio** - Width/height ratio
- **Near/Far Planes** - Clipping planes
- **Viewport** - Screen area

### Orthographic Camera

2D camera with orthographic projection:
- **Left/Right/Top/Bottom** - View volume
- **Near/Far Planes** - Clipping planes
- **Viewport** - Screen area

### Camera Controller (`camera/controller.rs`)

Handles camera movement:
- **Keyboard Input** - WASD, arrow keys
- **Mouse Input** - Look, zoom
- **Movement Modes** - First-person, third-person, free
- **Settings** - Move speed, rotate speed, mouse sensitivity

---

## 💡 Lighting System

### Light Types (`lighting/light.rs`)

- **Directional Light** - Sun, moon (infinite distance)
- **Point Light** - Light bulb, explosion (spherical)
- **Spot Light** - Flashlight, searchlight (conical)
- **Area Light** - Area light source (rectangular)

### Light Configuration

```rust
pub struct Light {
    name: String,
    light_type: LightType,
    color: Vec3,
    intensity: f32,
    enabled: bool,
    casts_shadows: bool,
    shadow_resolution: u32,
}
```

### Shadow Mapping (`lighting/shadows.rs`)

- **Cascaded Shadow Maps** - Multiple cascades for directional lights
- **Omnidirectional Shadows** - Cube maps for point lights
- **Shadow Config** - Resolution, bias, quality

### Light Probes (`lighting/probe.rs`)

Precomputed lighting for static objects:
- **Spherical Harmonics** - Diffuse lighting
- **Cube Maps** - Reflection probes
- **Probe Grid** - Multiple probes for large environments

---

## ✨ Post-Processing

### Effect Pipeline

Post-processing effects are applied in sequence:
1. **Bloom** - Bright area extraction and blur
2. **Motion Blur** - Velocity-based blur
3. **Depth of Field** - Focus-based blur
4. **HDR** - High Dynamic Range + Tone Mapping
5. **FXAA** - Fast Approximate Anti-Aliasing

### Bloom (`postprocess/bloom.rs`)

- **Bright Pass** - Extract pixels above threshold
- **Blur Passes** - Multiple Gaussian blur passes
- **Composite** - Combine with original
- **Configuration** - Threshold, intensity, blur radius

### Motion Blur (`postprocess/motion_blur.rs`)

- **Velocity Texture** - Stores object velocities
- **Sampling** - Sample along velocity vector
- **Configuration** - Intensity, max velocity, sample count

### Depth of Field (`postprocess/dof.rs`)

- **Focus Distance** - Distance in focus
- **Focus Range** - Range of acceptable focus
- **Blur Radius** - Amount of blur
- **Configuration** - Focus distance, range, blur radius

### HDR (`postprocess/hdr.rs`)

- **Tone Mapping** - Convert HDR to LDR
- **Exposure** - Brightness control
- **Gamma** - Color correction
- **Tone Mapping Methods** - Linear, Reinhard, ACES, Filmic

### FXAA (`postprocess/fxaa.rs`)

Fast anti-aliasing:
- **Edge Detection** - Find edges in image
- **Edge Smoothing** - Smooth edges
- **Configuration** - Enabled/disabled

---

## 💥 Particle System

### Particle System (`particles/system.rs`)

GPU-based particle system:
- **Particle Buffer** - GPU storage for particles
- **Emitter Management** - Multiple particle emitters
- **Rendering** - Instanced rendering
- **Configuration** - Max particles, GPU particles, compute shader

### Particle (`particles/particle.rs`)

```rust
pub struct Particle {
    position: Vec3,
    velocity: Vec3,
    size: Vec2,
    color: Vec4,
    lifetime: f32,
    max_lifetime: f32,
    rotation: f32,
    rotation_speed: f32,
    texture_index: u32,
    emitter_id: u32,
}
```

### Particle Emitter (`particles/emitter.rs`)

Emits particles with configurable properties:
- **Emission Rate** - Particles per second
- **Particle Lifetime** - How long particles live
- **Particle Size** - Size and variation
- **Particle Color** - Color and variation
- **Particle Velocity** - Initial velocity and variation
- **Emitter Shape** - Point, Sphere, Box, Cylinder, Cone, Line, Circle
- **Gravity** - Gravity effect
- **Drag** - Air resistance
- **Rotation** - Particle rotation

### Particle Effects (`particles/effects.rs`)

Pre-configured particle effects:
- **Smoke** - Slow-moving, fading particles
- **Fire** - Fast-moving, additive particles
- **Splash** - Water splash effect
- **Explosion** - Radial burst of particles
- **Sparks** - Small, fast-moving particles
- **Dust** - Slow-moving, ground-hugging particles
- **Blood** - Red particles with gravity
- **Magic** - Glowing, additive particles

---

## 🌊 Water System

### Water Renderer (`water/mod.rs`)

Main water rendering class:
- **Water Surface** - Mesh and rendering
- **Wave System** - Wave simulation
- **Water Interaction** - Object interaction (ships, projectiles)

### Water Surface (`water/surface.rs`)

- **Mesh** - Tessellated water mesh
- **Normal Map** - Wave normals
- **Foam Texture** - Foam around objects
- **Reflection Texture** - Reflected scene
- **Refraction Texture** - Refracted underwater scene

### Wave System (`water/waves.rs`)

Wave simulation methods:
- **Flat** - No waves (flat water)
- **Simple** - Basic sine waves
- **Gerstner** - More realistic waves
- **FFT** - Fast Fourier Transform (most realistic)

### Water Interaction (`water/interaction.rs`)

Handles interaction with objects:
- **Ripples** - Small waves from object movement
- **Splashes** - Water splashes from impacts
- **Object Displacement** - Water displacement around objects
- **Foam Generation** - Foam from object movement

---

## 🎮 UI System

### HUD (`ui/hud.rs`)

Head-Up Display elements:
- **Crosshair** - Aiming reticle
- **Ship Information** - HP, speed, fuel, crew
- **Weapon Information** - Ammo, cooldown
- **Player Information** - Health, role, team
- **Objectives** - Mission objectives
- **Compass** - Direction indicator
- **Damage Indicators** - Hit indicators

### Minimap (`ui/minimap.rs`)

Top-down view of battlefield:
- **Ship Positions** - All ships on map
- **Objectives** - Mission objectives
- **Fog of War** - Unexplored areas
- **Markers** - Custom markers
- **Zoom/Rotation** - Adjustable view

### Menu (`ui/menu.rs`)

Menu system:
- **Menu Types** - Main, Settings, Ship Selection, Loadout, Pause, In-Game, Scoreboard
- **Menu Items** - Buttons, Labels, Sliders, Dropdowns, Checkboxes, Text Input
- **Navigation** - Keyboard/mouse navigation
- **Selection** - Highlighted items

---

## 🔧 Integration with RHI

All rendering is built on top of the RHI (Rendering Hardware Interface):

```rust
// Example: Creating a texture
let texture = device.create_texture(
    width,
    height,
    1,
    Format::RGBA8Unorm,
    TextureUsage::COLOR_ATTACHMENT | TextureUsage::SAMPLED,
    1,
);

// Example: Creating a buffer
let buffer = device.create_buffer(
    size,
    BufferUsage::VERTEX | BufferUsage::TRANSFER_DST,
    false,
);

// Example: Creating a pipeline
let pipeline = device.create_graphics_pipeline(&pipeline_desc);

// Example: Rendering
let mut encoder = device.create_command_encoder();
encoder.begin_render_pass(render_pass, framebuffer, rect, clear_colors, clear_depth, clear_stencil);
encoder.bind_pipeline(&pipeline);
encoder.bind_vertex_buffer(&vertex_buffer);
encoder.draw(vertex_count, instance_count, first_vertex, first_instance);
encoder.end_render_pass();
device.submit(encoder);
```

---

## ⚡ Performance Optimizations

### LOD System

- **LOD Levels** - Multiple detail levels for meshes
- **Distance-Based** - Switch LOD based on camera distance
- **Automatic** - Automatic LOD selection

### Culling

- **Frustum Culling** - Remove objects outside view frustum
- **Occlusion Culling** - Remove objects blocked by others (GPU queries)
- **Small Object Culling** - Remove small objects at distance

### Instanced Rendering

- **Same Mesh** - Multiple instances of same mesh
- **Different Transforms** - Each instance has its own transform
- **Efficient** - Single draw call for many objects

### Batch Rendering

- **Small Objects** - Combine small objects into batches
- **Static Batching** - Pre-combine static objects
- **Dynamic Batching** - Combine objects at runtime

### GPU-Driven Rendering

- **Indirect Draw** - GPU decides what to draw
- **Compute Shaders** - GPU-based culling and sorting
- **Bindless Resources** - Access resources without binding

### Async Compute

- **Parallel Execution** - Run compute and graphics in parallel
- **Load Balancing** - Balance workload between queues

---

## 📊 Quality Presets

| Preset | Shadow Quality | AA | Post-Process | Particles | LOD |
|--------|----------------|----|--------------|-----------|-----|
| Low | 512, 1 cascade | None | None | 10K | Aggressive |
| Medium | 1024, 2 cascades | FXAA | Bloom, HDR | 50K | Balanced |
| High | 2048, 4 cascades | FXAA | All | 100K | Conservative |
| Ultra | 4096, 4 cascades | TAA | All + DoF | 200K | Minimal |

---

## 🎯 Implementation Roadmap

### P0: Core Infrastructure (1-2 weeks)
- [ ] Renderer initialization
- [ ] Render context management
- [ ] Settings system
- [ ] RHI integration

### P1: Render Passes (2-3 weeks)
- [ ] GBuffer pass
- [ ] Shadow pass
- [ ] Lighting pass
- [ ] Transparent pass
- [ ] Water pass
- [ ] UI pass

### P2: Asset Management (1-2 weeks)
- [ ] Material library
- [ ] Mesh library
- [ ] Texture library
- [ ] Asset loading

### P3: Scene System (1 week)
- [ ] Entity-Component system
- [ ] Scene management
- [ ] Culling system
- [ ] Camera system

### P4: Lighting (1 week)
- [ ] Light types
- [ ] Shadow mapping
- [ ] Light probes
- [ ] IBL

### P5: Post-Processing (1 week)
- [ ] Bloom
- [ ] Motion blur
- [ ] Depth of field
- [ ] HDR
- [ ] FXAA

### P6: Special Effects (1-2 weeks)
- [ ] Particle system
- [ ] Water system
- [ ] Decals
- [ ] Screen space effects

### P7: UI System (1 week)
- [ ] HUD
- [ ] Minimap
- [ ] Menu

### P8: Optimizations (1-2 weeks)
- [ ] LOD system
- [ ] Culling
- [ ] Instanced rendering
- [ ] Batch rendering
- [ ] GPU-driven rendering

---

## 📝 Notes

1. **Ray Tracing** - Optional feature, enabled if hardware supports
2. **VR Support** - Not implemented yet, but architecture allows for it
3. **Platform Support** - Windows (D3D12, D3D11), Linux (Vulkan), OpenGL 4.6 as fallback
4. **Feature Detection** - Automatic detection of supported features
5. **Fallbacks** - Graceful degradation if feature not supported

---

**File:** `.DOC/RENDER_ARCHITECTURE.md`  
**Project:** RFS-0.4.0  
**Module:** `src/render/`  
**Last Updated:** 2026-09-14  
**Status:** Architecture Complete
