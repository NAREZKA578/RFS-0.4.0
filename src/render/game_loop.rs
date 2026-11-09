//! Scene assembly for the main game loop.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The main loop binary holds the window and the frame pump; the pieces this
//! module owns are the ones tests can exercise without a window: how the
//! initial scene is built (model asset or cube fallback, a camera entity that
//! `Scene::cull` and the renderer's LOD context need), how the controller's
//! camera is mirrored into the scene each frame, and how winit keys map to
//! camera movement.
//!
//! Design note on the camera: the `CameraController` owns its camera as
//! `Box<dyn Camera>` and the scene wants `Arc<dyn Camera>`; an `Arc` cannot be
//! mutated through shared references, so the scene mirror is a small fresh
//! camera rebuilt from the controller's state each frame. The controller is
//! the single writer; the mirror exists so that culling and the renderer's
//! camera-position context always agree with the camera the player moves.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::{EulerRot, Quat, Vec3};

use super::camera::controller::CameraKey;
use super::camera::{Camera, CameraController, PerspectiveCamera};
use super::meshes::loader::MeshLoader;
use super::meshes::Mesh;
use super::scene::components::{CameraComponent, MeshComponent};
use super::scene::entity::Entity;
use super::scene::{Scene, Transform};
use super::water::ocean::{Ocean, OceanMesh};

// ------------------------------------------------------------------ camera

/// Field of view shared by the controller camera and its scene mirror.
pub const CAMERA_FOV: f32 = std::f32::consts::FRAC_PI_3;
/// Near plane shared by the controller camera and its scene mirror.
pub const CAMERA_NEAR: f32 = 0.1;
/// Far plane shared by the controller camera and its scene mirror.
pub const CAMERA_FAR: f32 = 1000.0;
/// Every loaded model is scaled so its largest extent equals this (metres).
pub const MODEL_TARGET_SIZE: f32 = 3.0;

/// Where the camera starts: a little above the origin, and along -Z.
///
/// `PerspectiveCamera` builds its view as `rotation * Vec3::Z`, so at
/// identity rotation the camera looks down +Z — which means a camera parked
/// on -Z looks straight at an object centred on the origin.
pub fn camera_start() -> Vec3 {
    Vec3::new(0.0, 2.5, -6.0)
}

/// A fresh perspective camera for the given controller state and aspect.
///
/// Rebuilt rather than mutated because the scene holds its camera behind
/// `Arc`: the values are cheap, and one construction site means the fov and
/// the near/far planes cannot drift apart between the controller and the
/// scene mirror.
pub fn mirror_camera(position: Vec3, yaw: f32, pitch: f32, aspect: f32) -> PerspectiveCamera {
    PerspectiveCamera::new(position, CAMERA_FOV, aspect.clamp(0.1, 10.0), CAMERA_NEAR, CAMERA_FAR)
        .with_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0))
}

// -------------------------------------------------------------------- fit

/// How a mesh is placed on the origin at a predictable size.
pub struct ModelFit {
    /// The AABB centre, in the mesh's own coordinates. The entity transform
    /// undoes it (`position = -center * scale`), so the drawn object is
    /// centred on the origin regardless of where its author left it.
    pub center: Vec3,
    /// The uniform scale that brings the largest AABB extent to the target
    /// size. A degenerate mesh (a point, or an empty one) fits at scale 1.
    pub scale: f32,
}

/// Compute the placement of `mesh` so its largest dimension becomes
/// `target_size` metres and its centre sits on the origin.
///
/// The whole point: assets arrive at any size and any origin (the owner's
/// probe asset is nothing like human-scale), and the demo scene needs both to
/// be predictable, or the object is either invisible or swallowed whole.
pub fn model_fit(mesh: &Mesh, target_size: f32) -> ModelFit {
    if mesh.vertices.is_empty() {
        return ModelFit {
            center: Vec3::ZERO,
            scale: 1.0,
        };
    }

    let mut min = mesh.vertices[0].position;
    let mut max = mesh.vertices[0].position;
    for vertex in &mesh.vertices[1..] {
        min = min.min(vertex.position);
        max = max.max(vertex.position);
    }
    let center = (min + max) * 0.5;
    let extent = (max - min).max_element();
    // A degenerate AABB (all vertices in one spot, or a zero-sized mesh) must
    // not divide by zero: it fits at scale 1 instead of NaN-poisoning the
    // model matrix, which would make the object vanish.
    let scale = if extent > 1e-6 { target_size / extent } else { 1.0 };

    ModelFit { center, scale }
}

// ------------------------------------------------------------------ scene

/// The scene the main loop owns, plus the bookkeeping the loop needs.
pub struct SceneRig {
    pub scene: Scene,
    /// The entity id of the camera, for the per-frame mirror.
    pub camera_id: usize,
    /// The entity id of the drawn object, if a scene was assembled at all.
    pub ship_id: Option<usize>,
    /// Where the ship's transform started. Buoyancy moves the ship relative to
    /// this, not relative to its previous frame — adding a bob to the current
    /// position every frame is a drift, not a wave.
    pub ship_base_position: Vec3,
    /// The ship's authored rotation, before the water tilts it.
    pub ship_base_rotation: Quat,
    /// The entity id of the ocean plane, when the water was built.
    pub water_id: Option<usize>,
    /// The wave field and its drawable mesh. `None` only when the GPU buffers
    /// could not be created; the rest of the scene still draws.
    pub ocean: Option<OceanMesh>,
    /// The model path that actually loaded, or `None` for the cube fallback.
    pub used_model_path: Option<PathBuf>,
    /// Everything the assembly decided along the way, for the run log.
    pub notes: Vec<String>,
}

/// Assemble the main loop's scene: one drawn object and one camera.
///
/// `model_path` is tried through `MeshLoader`; anything that fails (missing
/// file, unsupported asset, loader refusal) falls back to a cube with a note,
/// because the loop must start with *something* drawn. The meshes get their
/// GPU buffers here: `items_for_scene` refuses to produce an item "when the
/// mesh has no GPU buffers", so a mesh without them quietly draws nothing.
pub fn build_scene(device: &Arc<crate::rhi::Device>, model_path: Option<&Path>) -> SceneRig {
    let mut notes = Vec::new();
    let mut used_model_path = None;

    let mut mesh = match model_path {
        Some(path) if path.exists() => {
            let mut loader = MeshLoader::new();
            loader.set_device(device.clone());
            match loader.load(path) {
                Some(loaded) => {
                    notes.push(format!(
                        "model loaded from {}: {} vertices, {} indices",
                        path.display(),
                        loaded.vertex_count(),
                        loaded.index_count()
                    ));
                    used_model_path = Some(path.to_path_buf());
                    loaded
                }
                None => {
                    notes.push(format!(
                        "model {} did not load: falling back to the cube",
                        path.display()
                    ));
                    Mesh::cube("fallback cube", 1.0)
                }
            }
        }
        Some(path) => {
            notes.push(format!(
                "model {} is not on disk: falling back to the cube",
                path.display()
            ));
            Mesh::cube("fallback cube", 1.0)
        }
        None => {
            notes.push("no model path given: using the cube".to_string());
            Mesh::cube("fallback cube", 1.0)
        }
    };

    if let Err(e) = mesh.create_buffers(device) {
        notes.push(format!("GPU buffers for the mesh failed: {e}"));
    }
    let fit = model_fit(&mesh, MODEL_TARGET_SIZE);
    notes.push(format!(
        "model fit: centre {:?}, scale {}",
        fit.center, fit.scale
    ));
    let mesh = Arc::new(mesh);

    let mut scene = Scene::new("main loop");

    // The drawn object, centred on the origin at the target size.
    let ship_base_position = -fit.center * fit.scale;
    let ship_base_rotation = Quat::IDENTITY;
    let mut ship = Entity::new("ship");
    let mut ship_transform = Transform::new();
    ship_transform.set_position(ship_base_position);
    ship_transform.set_rotation(ship_base_rotation);
    ship_transform.set_scale(Vec3::splat(fit.scale));
    ship.add_component(ship_transform);
    ship.add_component(MeshComponent::new(mesh));
    let ship_id = scene.add_entity(ship);

    // The sea. Its vertices are already in world space, so the entity's
    // transform stays identity and `height_at` answers in the same space the
    // ship's transform uses.
    let (ocean, water_id) = match OceanMesh::new(device, Ocean::default()) {
        Ok(ocean) => {
            let mesh = ocean.mesh();
            notes.push(format!(
                "ocean: {} vertices, {} indices, level {}",
                mesh.vertex_count(),
                mesh.index_count(),
                ocean.ocean().level()
            ));
            let mut water = Entity::new("ocean");
            water.add_component(Transform::new());
            water.add_component(MeshComponent::new(mesh));
            let id = scene.add_entity(water);
            (Some(ocean), Some(id))
        }
        Err(e) => {
            notes.push(format!("the ocean could not be built: {e}"));
            (None, None)
        }
    };

    // The camera. The mirror starts as a copy of this camera; every frame
    // the controller rewrites it (see `sync_camera_mirror`).
    let start = camera_start();
    let mut camera_entity = Entity::new("camera");
    camera_entity.add_component(Transform::with_position(start));
    camera_entity.add_component(CameraComponent::new(Arc::new(mirror_camera(
        start,
        0.0,
        0.0,
        16.0 / 9.0,
    ))));
    let camera_id = scene.add_entity(camera_entity);
    // `Scene::cull` skips frustum testing without an active camera and
    // `items_for_scene` runs that cull, so the camera must be active before
    // the items are built — a silent make with no camera mistake otherwise.
    scene.set_active_camera(camera_id);

    SceneRig {
        scene,
        camera_id,
        ship_id: Some(ship_id),
        ship_base_position,
        ship_base_rotation,
        water_id,
        ocean,
        used_model_path,
        notes,
    }
}

// ------------------------------------------------------------------ mirror

/// Rewrite the scene's camera from the controller's state.
///
/// `CameraController::camera()` is the single source of truth for the
/// player's camera; the scene holds a `CameraComponent` whose `Arc<dyn
/// Camera>` cannot be mutated in place (the controller would need shared
/// access, and the camera the controller itself moves is a different value).
/// So each frame the mirror hands the scene a genuinely fresh camera built
/// from the same state, and moves the camera entity's transform to the same
/// position — `Scene::camera_position` reads it for the renderer's LOD and
/// distance-culling context.
pub fn sync_camera_mirror(
    scene: &mut Scene,
    camera_id: usize,
    controller: &CameraController,
    aspect: f32,
) {
    let position = controller.camera().position();
    let mirror: Arc<dyn Camera> = Arc::new(mirror_camera(
        position,
        controller.yaw,
        controller.pitch,
        aspect,
    ));

    let mut mirrored = 0usize;
    for entity in scene.entities_mut() {
        if entity.id() != camera_id {
            continue;
        }
        if let Some(transform) = entity.transform_mut() {
            transform.set_position(position);
        }
        if let Some(component) = entity.get_component_mut::<CameraComponent>() {
            component.camera = mirror.clone();
            mirrored += 1;
        }
    }
    // A wrong camera id would leave the scene's camera silently stale —
    // culling, the frustum and LOD would all judge from a dead camera while
    // the picture looks perfectly alive.
    debug_assert_eq!(
        mirrored, 1,
        "the camera mirror must touch exactly one camera entity, touched {mirrored}"
    );
}

// -------------------------------------------------------------------- keys

/// Map a winit physical key to a camera action.
///
/// `PhysicalKey::Code` is what a keyboard delivers on Windows; the logical
/// layer would rebind WASD under non-English layouts — exactly the layout
/// this project's team uses. WASD and Q/E walk, the arrows rotate.
pub fn map_winit_key(key: winit::keyboard::PhysicalKey) -> Option<CameraKey> {
    use winit::keyboard::{KeyCode, PhysicalKey};

    let PhysicalKey::Code(code) = key else {
        return None;
    };
    match code {
        KeyCode::KeyW => Some(CameraKey::Forward),
        KeyCode::KeyS => Some(CameraKey::Backward),
        KeyCode::KeyA => Some(CameraKey::Left),
        KeyCode::KeyD => Some(CameraKey::Right),
        KeyCode::KeyE => Some(CameraKey::Up),
        KeyCode::KeyQ => Some(CameraKey::Down),
        KeyCode::ArrowLeft => Some(CameraKey::RotateLeft),
        KeyCode::ArrowRight => Some(CameraKey::RotateRight),
        KeyCode::ArrowUp => Some(CameraKey::RotateUp),
        KeyCode::ArrowDown => Some(CameraKey::RotateDown),
        _ => None,
    }
}

// ------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::meshes::mesh::{MeshBuilder, Vertex};
    use super::*;
    use winit::keyboard::{KeyCode, NativeKeyCode, PhysicalKey};

    #[test]
    fn a_symmetric_mesh_fits_centered_in_place() {
        let mesh = Mesh::cube("fit", 2.0); // vertices span [-1, 1] on every axis
        let fit = model_fit(&mesh, 4.0);
        assert!(
            fit.center.abs_diff_eq(Vec3::ZERO, 1e-5),
            "an even mesh must be centred, got {:?}",
            fit.center
        );
        assert!(
            (fit.scale - 2.0).abs() < 1e-5,
            "target 4 over extent 2 must scale by 2, got {}",
            fit.scale
        );
    }

    #[test]
    fn an_off_centre_mesh_is_recentred_and_scaled() {
        let mesh = MeshBuilder::new("off-centre")
            .with_vertices(vec![
                Vertex::new(Vec3::new(0.0, 0.0, 0.0)),
                Vertex::new(Vec3::new(2.0, 2.0, 0.0)),
                Vertex::new(Vec3::new(0.0, 2.0, 4.0)),
                Vertex::new(Vec3::new(2.0, 0.0, 4.0)),
            ])
            .build();
        // AABB spans (0..2, 0..2, 0..4): centre (1,1,2), largest extent 4.
        let fit = model_fit(&mesh, 8.0);
        assert!(
            fit.center.abs_diff_eq(Vec3::new(1.0, 1.0, 2.0), 1e-5),
            "got {:?}",
            fit.center
        );
        assert!(
            (fit.scale - 2.0).abs() < 1e-5,
            "target 8 over extent 4 must scale by 2, got {}",
            fit.scale
        );
        // What the ship's transform must do with these numbers: undo the
        // centre and the scale, which parks the model's centre on the origin.
        let placement = -fit.center * fit.scale;
        assert!(
            placement.abs_diff_eq(Vec3::new(-2.0, -2.0, -4.0), 1e-4),
            "got {:?}",
            placement
        );
    }

    #[test]
    fn a_degenerate_mesh_fits_without_dividing_by_zero() {
        let mesh = MeshBuilder::new("point")
            .with_vertices(vec![Vertex::new(Vec3::ZERO), Vertex::new(Vec3::ZERO)])
            .build();
        let fit = model_fit(&mesh, 3.0);
        assert_eq!(
            fit.scale, 1.0,
            "a zero-extent mesh must fit at scale 1, got {}",
            fit.scale
        );
        assert_eq!(fit.center, Vec3::ZERO);
    }

    #[test]
    fn an_empty_mesh_fits_neutrally() {
        let fit = model_fit(&Mesh::new("empty"), 3.0);
        assert_eq!(fit.scale, 1.0);
        assert_eq!(fit.center, Vec3::ZERO);
    }

    #[test]
    fn winit_keys_map_to_camera_keys() {
        let key = |code: KeyCode| map_winit_key(PhysicalKey::Code(code));
        assert_eq!(key(KeyCode::KeyW), Some(CameraKey::Forward));
        assert_eq!(key(KeyCode::KeyS), Some(CameraKey::Backward));
        assert_eq!(key(KeyCode::KeyA), Some(CameraKey::Left));
        assert_eq!(key(KeyCode::KeyD), Some(CameraKey::Right));
        assert_eq!(key(KeyCode::KeyE), Some(CameraKey::Up));
        assert_eq!(key(KeyCode::KeyQ), Some(CameraKey::Down));
        assert_eq!(key(KeyCode::ArrowLeft), Some(CameraKey::RotateLeft));
        assert_eq!(key(KeyCode::ArrowRight), Some(CameraKey::RotateRight));
        assert_eq!(key(KeyCode::ArrowUp), Some(CameraKey::RotateUp));
        assert_eq!(key(KeyCode::ArrowDown), Some(CameraKey::RotateDown));
        assert_eq!(key(KeyCode::F5), None, "unmapped keys must stay unmapped");
    }

    #[test]
    fn a_key_without_a_code_maps_to_nothing() {
        assert_eq!(
            map_winit_key(PhysicalKey::Unidentified(NativeKeyCode::Unidentified)),
            None
        );
    }

    #[test]
    fn the_mirror_carries_the_controller_camera_into_the_scene() {
        let mut controller = CameraController::new(Box::new(mirror_camera(
            camera_start(),
            0.0,
            0.0,
            1.6,
        )));
        // Walk forward for one second: `move_speed` defaults to 5, the yaw is
        // 0, so the camera flies along its forward axis (+Z at rotation
        // identity). This is the position the scene must show afterwards.
        controller.handle_keyboard(CameraKey::Forward, true);
        controller.update_with_mode(Duration::from_secs_f32(1.0));
        let expected_position = controller.camera().position();
        assert!(
            expected_position
                .abs_diff_eq(Vec3::new(0.0, 2.5, -1.0), 1e-5),
            "the walk must move the controller's own camera, got {:?}",
            expected_position
        );

        let mut scene = Scene::new("mirror test");
        let mut entity = Entity::new("camera");
        entity.add_component(Transform::with_position(camera_start()));
        entity.add_component(CameraComponent::new(Arc::new(mirror_camera(
            camera_start(),
            0.0,
            0.0,
            1.0,
        ))));
        let camera_id = scene.add_entity(entity);
        scene.set_active_camera(camera_id);

        sync_camera_mirror(&mut scene, camera_id, &controller, 1.5);

        let component = scene
            .get_entity(camera_id)
            .and_then(|e| e.get_component::<CameraComponent>())
            .expect("the camera component must survive the mirror");
        assert!(
            component.camera.position().abs_diff_eq(expected_position, 1e-5),
            "the scene camera must be where the controller flew, got {:?}",
            component.camera.position()
        );
        assert!(
            (component.camera.aspect_ratio() - 1.5).abs() < 1e-5,
            "the mirror must adopt the frame's aspect, got {}",
            component.camera.aspect_ratio()
        );
        let transform = scene
            .get_entity(camera_id)
            .and_then(|e| e.transform())
            .expect("the camera entity keeps its transform");
        assert!(
            transform.position.abs_diff_eq(expected_position, 1e-5),
            "Scene::camera_position reads the transform, so it must follow, got {:?}",
            transform.position
        );
    }
}
