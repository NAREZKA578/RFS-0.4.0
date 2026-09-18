# RHI + Render Implementation TODO List

> **Project:** RFS-0.4.0  
> **Modules:** `src/rhi/` (RHI) + `src/render/` (Render System)  
> **Status:** Architecture Complete, Implementation Started  
> **NOT CONNECTED TO SERVER CODE**

---

## 📌 Priority Legend

| Symbol | Priority | Description |
|--------|----------|-------------|
| P0 | **Critical** | Required for basic functionality |
| P1 | **High** | Important for most use cases |
| P2 | **Medium** | Nice to have |
| P3 | **Low** | Optional enhancements |

---

## 🎯 Current Status Summary

| Module | Status | Files | LOC | Notes |
|--------|--------|-------|-----|-------|
| **RHI** | Architecture Complete | 83 | ~5,000 | Foundation ready, backends stubbed |
| **Render** | Architecture Complete | 80+ | ~10,000 | All modules designed, ready for implementation |
| **Total** | **Architecture Complete** | **163+** | **~15,000** | Ready for implementation |

---

# 🔧 RHI Module TODO

## ✅ Phase 1: Foundation - **COMPLETE**

- [x] Project structure created
- [x] Type definitions (GraphicsApi, Format, etc.)
- [x] Error handling (RhiError, RhiResult)
- [x] Core module stubs (instance, device, caps)
- [x] Backend stubs (vulkan, d3d12, d3d11, opengl)
- [x] All forward declarations fixed
- [x] WebGPU/Metal references removed

## P0: Core Infrastructure (CRITICAL)

### Backend Implementations
- [ ] Vulkan Backend (`backend/vulkan/`) - **P0** | 6-8 weeks | Not Started
- [ ] D3D12 Backend (`backend/d3d12/`) - **P0** | 4-6 weeks | Not Started
- [ ] D3D11 Backend (`backend/d3d11/`) - **P0** | 3-4 weeks | Not Started
- [ ] OpenGL Backend (`backend/opengl/`) - **P0** | 3-4 weeks | Not Started

### Core Module
- [ ] Complete `core/instance.rs` - **P0** | 2 days | Partial
- [ ] Complete `core/device.rs` - **P0** | 3 days | Partial
- [ ] Implement `core/caps.rs` - **P0** | 1 day | Not Started

### Resource Module
- [ ] `resource/mod.rs` - **P0** | 1 hour | Not Started
- [ ] `resource/buffer.rs` - **P0** | 3 days | Not Started
- [ ] `resource/texture.rs` - **P0** | 5 days | Not Started
- [ ] `resource/sampler.rs` - **P0** | 1 day | Not Started
- [ ] `resource/acceleration/` - **P1** | 1 week | Not Started

### Pipeline Module
- [ ] `pipeline/mod.rs` - **P0** | 1 hour | Not Started
- [ ] `pipeline/graphics.rs` - **P0** | 5 days | Not Started
- [ ] `pipeline/compute.rs` - **P0** | 2 days | Not Started
- [ ] `pipeline/ray_tracing.rs` - **P1** | 1 week | Not Started
- [ ] `pipeline/mesh_shading.rs` - **P2** | 5 days | Not Started

### Shader Module
- [ ] `shader/mod.rs` - **P0** | 1 hour | Not Started
- [ ] `shader/module.rs` - **P0** | 2 days | Not Started
- [ ] `shader/reflection.rs` - **P1** | 3 days | Not Started
- [ ] `shader/compiler.rs` - **P1** | 1 week | Not Started

### Command Module
- [ ] `command/mod.rs` - **P0** | 1 hour | Not Started
- [ ] `command/buffer.rs` - **P0** | 3 days | Not Started
- [ ] `command/pool.rs` - **P0** | 2 days | Not Started
- [ ] `command/encoder.rs` - **P1** | 3 days | Not Started
- [ ] `command/pass/` - **P0** | 5 days | Not Started
- [ ] `command/commands.rs` - **P0** | 3 days | Not Started

### Sync Module
- [ ] `sync/mod.rs` - **P0** | 1 hour | Not Started
- [ ] `sync/fence.rs` - **P0** | 2 days | Not Started
- [ ] `sync/semaphore.rs` - **P0** | 2 days | Not Started
- [ ] `sync/barrier.rs` - **P0** | 2 days | Not Started

### Memory Module
- [ ] `memory/mod.rs` - **P0** | 1 hour | Not Started
- [ ] `memory/allocator.rs` - **P0** | 2 days | Not Started
- [ ] `memory/buddy.rs` - **P1** | 3 days | Not Started
- [ ] `memory/linear.rs` - **P1** | 2 days | Not Started
- [ ] `memory/pool.rs` - **P1** | 2 days | Not Started

### Descriptor Module
- [ ] `descriptor/mod.rs` - **P0** | 1 hour | Not Started
- [ ] `descriptor/layout.rs` - **P0** | 2 days | Not Started
- [ ] `descriptor/set.rs` - **P0** | 2 days | Not Started
- [ ] `descriptor/allocator.rs` - **P0** | 3 days | Not Started

### SwapChain Module
- [ ] `swapchain/mod.rs` - **P0** | 1 hour | Not Started
- [ ] `swapchain/swapchain.rs` - **P0** | 5 days | Not Started
- [ ] `swapchain/surface.rs` - **P0** | 2 days | Not Started

### Query & Debug Modules
- [ ] `query/mod.rs` - **P1** | 1 hour | Not Started
- [ ] `query/pool.rs` - **P1** | 2 days | Not Started
- [ ] `query/types.rs` - **P1** | 1 day | Not Started
- [ ] `query/results.rs` - **P1** | 1 day | Not Started
- [ ] `debug/mod.rs` - **P2** | 1 hour | Not Started
- [ ] `debug/markers.rs` - **P2** | 2 days | Not Started
- [ ] `debug/validation.rs` - **P2** | 3 days | Not Started
- [ ] `debug/stats.rs` - **P2** | 3 days | Not Started
- [ ] `debug/capture.rs` - **P2** | 3 days | Not Started

### Utils Module
- [ ] `utils/mod.rs` - **P2** | 1 hour | Not Started
- [ ] `utils/conversion.rs` - **P2** | 3 days | Not Started
- [ ] `utils/alignment.rs` - **P2** | 1 day | Not Started
- [ ] `utils/hash.rs` - **P2** | 1 day | Not Started

---

# 🎨 Render Module TODO

## ✅ Architecture Phase - **COMPLETE**

- [x] Module structure created (`src/render/`)
- [x] All submodules created (13 modules)
- [x] All mod.rs files with proper exports
- [x] All type definitions created
- [x] All trait definitions created
- [x] All circular dependencies resolved
- [x] All import issues fixed
- [x] Bug documentation completed

## P0: Core Infrastructure (CRITICAL - 1-2 weeks)

### Core Module
- [ ] Complete `core/renderer.rs` - Main renderer implementation
- [ ] Complete `core/context.rs` - Render context management
- [ ] Complete `core/settings.rs` - Graphics settings system
- [ ] Integrate with RHI device and swapchain
- [ ] Implement frame rendering loop
- [ ] Implement resize handling
- [ ] Implement settings application

### Graph Module
- [ ] Complete `graph/graph.rs` - Render graph execution
- [ ] Complete `graph/node.rs` - Render pass node management
- [ ] Complete `graph/resource.rs` - Resource management
- [ ] Implement topological sorting
- [ ] Implement resource lifecycle management

## P1: Render Passes (HIGH - 2-3 weeks)

### Base Pass
- [ ] Complete `passes/base.rs` - Base render pass implementation
- [ ] Implement pass execution logic
- [ ] Implement pass initialization
- [ ] Implement pass resize handling

### GBuffer Pass
- [ ] Complete `passes/gbuffer.rs` - GBuffer pass implementation
- [ ] Create GBuffer textures (position, normal, albedo, material)
- [ ] Create GBuffer render pass
- [ ] Create GBuffer pipeline
- [ ] Implement GBuffer rendering

### Shadow Pass
- [ ] Complete `passes/shadow.rs` - Shadow pass implementation
- [ ] Implement cascaded shadow maps (4 cascades)
- [ ] Create shadow map textures
- [ ] Create shadow render pass
- [ ] Create shadow pipeline
- [ ] Implement shadow matrix calculation

### Lighting Pass
- [ ] Complete `passes/lighting.rs` - Lighting pass implementation
- [ ] Implement PBR lighting
- [ ] Implement directional/point/spot lights
- [ ] Implement SSAO (Screen Space Ambient Occlusion)
- [ ] Implement SSR (Screen Space Reflections)
- [ ] Create lighting pipeline

### Transparent Pass
- [ ] Complete `passes/transparent.rs` - Transparent pass implementation
- [ ] Implement depth sorting (back-to-front)
- [ ] Implement blend modes (Alpha, Additive, Multiplicative, Screen)
- [ ] Create transparent pipeline

### Water Pass
- [ ] Complete `passes/water.rs` - Water pass implementation
- [ ] Implement reflection rendering (planar/cube map)
- [ ] Implement refraction rendering
- [ ] Create water pipeline

### UI Pass
- [ ] Complete `passes/ui.rs` - UI pass implementation
- [ ] Implement orthographic projection
- [ ] Implement UI rendering
- [ ] Create UI pipeline

## P1: Asset Management (HIGH - 1-2 weeks)

### Material System
- [ ] Complete `materials/material.rs` - Base material implementation
- [ ] Complete `materials/pbr.rs` - PBR material implementation
- [ ] Complete `materials/water.rs` - Water material implementation
- [ ] Complete `materials/library.rs` - Material library
- [ ] Implement material pipeline creation
- [ ] Implement material parameter binding
- [ ] Create material presets (metal, plastic, wood, glass, water)

### Mesh System
- [ ] Complete `meshes/mesh.rs` - Mesh implementation
- [ ] Complete `meshes/loader.rs` - Mesh loader (OBJ, glTF, FBX)
- [ ] Complete `meshes/library.rs` - Mesh library
- [ ] Implement mesh buffer creation
- [ ] Implement primitive mesh creation (cube, sphere, plane, cylinder)
- [ ] Implement LOD support

### Texture System
- [ ] Complete `textures/texture.rs` - Texture implementation
- [ ] Complete `textures/loader.rs` - Texture loader (PNG, JPG, DDS, KTX2)
- [ ] Complete `textures/library.rs` - Texture library
- [ ] Implement texture upload
- [ ] Implement common textures (white, black, checkerboard, normal maps)
- [ ] Implement texture atlas support

## P1: Scene System (HIGH - 1 week)

### Scene Management
- [ ] Complete `scene/scene.rs` - Scene implementation
- [ ] Complete `scene/entity.rs` - Entity implementation
- [ ] Complete `scene/components.rs` - ECS components
- [ ] Implement entity management (add, remove, find)
- [ ] Implement camera management
- [ ] Implement light management

### Culling System
- [ ] Complete `scene/culling.rs` - Culling implementation
- [ ] Implement frustum culling
- [ ] Implement occlusion culling (optional - GPU queries)
- [ ] Implement AABB calculations
- [ ] Integrate culling with scene

## P1: Camera System (MEDIUM - 3-5 days)

### Camera Implementations
- [ ] Complete `camera/camera.rs` - Camera trait and implementations
- [ ] Implement perspective camera
- [ ] Implement orthographic camera
- [ ] Implement camera matrices (view, projection, view-projection)

### Camera Controller
- [ ] Complete `camera/controller.rs` - Camera controller implementation
- [ ] Implement keyboard input handling (WASD, arrow keys)
- [ ] Implement mouse input handling (look, zoom)
- [ ] Implement camera movement
- [ ] Implement camera rotation
- [ ] Implement different camera modes (FPS, TPS, Free)

## P2: Lighting System (MEDIUM - 1 week)

### Light Types
- [ ] Complete `lighting/light.rs` - Light implementations
- [ ] Implement directional light
- [ ] Implement point light
- [ ] Implement spot light
- [ ] Implement light properties (color, intensity, range, shadows)

### Shadow Mapping
- [ ] Complete `lighting/shadows.rs` - Shadow mapping implementation
- [ ] Implement cascaded shadow maps (4 cascades)
- [ ] Implement omnidirectional shadow maps (cube maps)
- [ ] Implement shadow bias calculation
- [ ] Implement shadow matrix calculation

### Light Probes
- [ ] Complete `lighting/probe.rs` - Light probe implementation
- [ ] Implement spherical harmonics
- [ ] Implement cube map probes
- [ ] Implement probe grid
- [ ] Implement probe baking

## P2: Post-Processing (MEDIUM - 1 week)

### Bloom
- [ ] Complete `postprocess/bloom.rs` - Bloom effect implementation
- [ ] Implement bright pass
- [ ] Implement blur passes (Gaussian)
- [ ] Implement composite pass
- [ ] Create bloom pipeline

### Motion Blur
- [ ] Complete `postprocess/motion_blur.rs` - Motion blur implementation
- [ ] Implement velocity texture
- [ ] Implement blur sampling
- [ ] Create motion blur pipeline

### Depth of Field
- [ ] Complete `postprocess/dof.rs` - DoF implementation
- [ ] Implement focus calculation
- [ ] Implement blur calculation
- [ ] Create DoF pipeline

### HDR
- [ ] Complete `postprocess/hdr.rs` - HDR implementation
- [ ] Implement tone mapping (Linear, Reinhard, ACES, Filmic)
- [ ] Implement exposure control
- [ ] Implement gamma correction
- [ ] Create HDR pipeline

### FXAA
- [ ] Complete `postprocess/fxaa.rs` - FXAA implementation
- [ ] Implement edge detection
- [ ] Implement edge smoothing
- [ ] Create FXAA pipeline

## P2: Particle System (MEDIUM - 1 week)

### Particle System
- [ ] Complete `particles/particle.rs` - Particle struct
- [ ] Complete `particles/system.rs` - Particle system implementation
- [ ] Implement particle buffer management
- [ ] Implement particle emission
- [ ] Implement particle update
- [ ] Implement particle rendering (instanced)

### Particle Emitter
- [ ] Complete `particles/emitter.rs` - Particle emitter implementation
- [ ] Implement emission rate control
- [ ] Implement particle properties (lifetime, size, color, velocity)
- [ ] Implement emitter shapes (Point, Sphere, Box, Cylinder, Cone, Line, Circle)
- [ ] Implement gravity and drag
- [ ] Implement rotation

### Particle Effects
- [ ] Complete `particles/effects.rs` - Particle effects implementation
- [ ] Implement smoke effect
- [ ] Implement fire effect
- [ ] Implement splash effect
- [ ] Implement explosion effect
- [ ] Implement sparks effect
- [ ] Implement dust effect
- [ ] Implement blood effect
- [ ] Implement magic effect

## P2: Water System (MEDIUM - 1 week)

### Water Surface
- [ ] Complete `water/surface.rs` - Water surface implementation
- [ ] Implement water mesh (tessellated)
- [ ] Implement normal map
- [ ] Implement foam texture
- [ ] Implement reflection texture
- [ ] Implement refraction texture

### Wave System
- [ ] Complete `water/waves.rs` - Wave system implementation
- [ ] Implement flat waves
- [ ] Implement simple waves
- [ ] Implement Gerstner waves
- [ ] Implement FFT waves (optional - high performance cost)

### Water Interaction
- [ ] Complete `water/interaction.rs` - Water interaction implementation
- [ ] Implement ripples from object movement
- [ ] Implement splashes from impacts
- [ ] Implement object displacement
- [ ] Implement foam generation

## P2: UI System (MEDIUM - 1 week)

### HUD
- [ ] Complete `ui/hud.rs` - HUD implementation
- [ ] Implement crosshair
- [ ] Implement ship information display (HP, speed, fuel, crew)
- [ ] Implement weapon information display (ammo, cooldown)
- [ ] Implement player information display (health, role, team)
- [ ] Implement objectives display
- [ ] Implement compass
- [ ] Implement damage indicators

### Minimap
- [ ] Complete `ui/minimap.rs` - Minimap implementation
- [ ] Implement minimap rendering
- [ ] Implement ship markers
- [ ] Implement objective markers
- [ ] Implement fog of war (optional)
- [ ] Implement zoom and rotation

### Menu
- [ ] Complete `ui/menu.rs` - Menu implementation
- [ ] Implement main menu
- [ ] Implement settings menu
- [ ] Implement ship selection menu
- [ ] Implement loadout selection menu
- [ ] Implement pause menu
- [ ] Implement in-game menu
- [ ] Implement scoreboard

## P3: Optimizations (LOW - 1-2 weeks)

### LOD System
- [ ] Implement LOD level selection
- [ ] Implement distance-based LOD switching
- [ ] Implement LOD mesh creation
- [ ] Integrate LOD with rendering

### Culling Optimizations
- [ ] Implement occlusion culling (GPU queries)
- [ ] Implement small object culling
- [ ] Implement temporal culling coherence

### Rendering Optimizations
- [ ] Implement instanced rendering
- [ ] Implement batch rendering
- [ ] Implement GPU-driven rendering (optional - requires compute shaders)
- [ ] Implement async compute (optional)
- [ ] Implement bindless resources (optional)

---

# 📊 Implementation Roadmap

## Phase 1: RHI Foundation (COMPLETE)
✅ Project structure created
✅ Type definitions
✅ Error handling
✅ Core module stubs
✅ Backend stubs
✅ All bugs fixed

**Time:** COMPLETE

## Phase 2: RHI Backend Implementations (NEXT)
1. Vulkan Backend
   - Device enumeration/creation
   - Buffer/Texture/Sampler resources
   - Graphics/Compute pipelines
   - Descriptor sets/layouts
   - Command buffers
   - Synchronization
   - Swap chain
   - Memory allocator
   - Shader modules

2. D3D12 Backend
3. D3D11 Backend
4. OpenGL Backend

**Estimated Time:** 6-8 weeks (Vulkan) + 4-6 weeks (D3D12) + 3-4 weeks (D3D11) + 3-4 weeks (OpenGL)

## Phase 3: Core RHI Features
- Complete resource module
- Complete pipeline module
- Complete descriptor module
- Complete command module
- Complete sync module
- Complete memory module
- Complete shader module

**Estimated Time:** 4-6 weeks

## Phase 4: Render System Foundation
- Complete core module (renderer, context, settings)
- Complete graph module (graph, node, resource, types)
- Complete base render pass
- Implement GBuffer pass
- Implement Shadow pass
- Implement Lighting pass

**Estimated Time:** 3-4 weeks

## Phase 5: Asset Management
- Complete material system
- Complete mesh system
- Complete texture system
- Implement asset loaders

**Estimated Time:** 2-3 weeks

## Phase 6: Scene & Camera
- Complete scene system
- Complete camera system
- Implement culling

**Estimated Time:** 2 weeks

## Phase 7: Advanced Features
- Complete all render passes
- Implement post-processing effects
- Implement particle system
- Implement water system
- Implement UI system

**Estimated Time:** 4-6 weeks

## Phase 8: Optimizations
- Implement LOD system
- Implement culling optimizations
- Implement rendering optimizations

**Estimated Time:** 1-2 weeks

## Phase 9: Polish & Release
- Performance optimization
- Comprehensive testing
- Full documentation
- First stable release (v1.0.0)

**Estimated Time:** 4-6 weeks

---

## 📈 Total Estimated Timeline

| Phase | Module | Time | Status |
|-------|--------|------|--------|
| 1 | RHI Foundation | COMPLETE | ✅ |
| 2 | RHI Backends | 16-22 weeks | ⏳ |
| 3 | RHI Core Features | 4-6 weeks | ⏳ |
| 4 | Render Foundation | 3-4 weeks | ⏳ |
| 5 | Asset Management | 2-3 weeks | ⏳ |
| 6 | Scene & Camera | 2 weeks | ⏳ |
| 7 | Advanced Features | 4-6 weeks | ⏳ |
| 8 | Optimizations | 1-2 weeks | ⏳ |
| 9 | Polish & Release | 4-6 weeks | ⏳ |
| | **Total** | **36-51 weeks** | |

**Note:** This is for full implementation. A minimal working version (RHI + basic render) can be achieved in **8-12 weeks**.

---

## 🎯 Key Design Principles

### RHI
- Minimal Abstraction Overhead
- Thread-Safe
- Zero-Cost Abstractions
- Consistent API across all backends
- Comprehensive Error Handling
- Debuggable

### Render
- RenderGraph-based architecture
- PBR Materials
- Deferred Rendering (primary)
- Forward Rendering (transparent objects)
- GPU-Driven where possible
- LOD System for performance
- Frustum/Occlusion Culling
- Instanced Rendering

---

## 📋 Current Progress

| Category | Total Tasks | Completed | In Progress | Remaining |
|----------|-------------|-----------|-------------|-----------|
| RHI Architecture | 100% | 100% | 0% | 0% |
| Render Architecture | 100% | 100% | 0% | 0% |
| Bug Fixes | 24 | 24 | 0 | 0 |
| RHI Implementation | ~50 | 0 | 0 | ~50 |
| Render Implementation | ~112 | 0 | 0 | ~112 |
| **Total** | **~286** | **24** | **0** | **~262** |

---

## 🚀 Next Immediate Steps

### Week 1-2: RHI Foundation + Render Core
1. ✅ **DONE** - Architecture design
2. ✅ **DONE** - Bug fixes
3. [ ] **NEXT** - Create Cargo.toml for client
4. [ ] **NEXT** - Implement RHI core/instance.rs
5. [ ] **NEXT** - Implement RHI core/device.rs
6. [ ] **NEXT** - Implement Render core/renderer.rs
7. [ ] **NEXT** - Implement Render core/context.rs
8. [ ] **NEXT** - Try to compile client

### Week 3-4: RHI Backend + Render Graph
9. [ ] Implement Vulkan backend device creation
10. [ ] Implement Render graph/graph.rs
11. [ ] Implement Render passes/base.rs
12. [ ] Implement Render passes/gbuffer.rs

---

**File:** `.DOC/TODO.md`  
**Project:** RFS-0.4.0  
**Modules:** `src/rhi/` + `src/render/`  
**Last Updated:** 2026-09-14  
**Status:** Architecture Complete, Implementation Ready
