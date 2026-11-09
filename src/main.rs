//! The RFS client's main game loop.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The owner's order: "make the main game loop so everything can be tested
//! and initialized". This binary is that loop, assembled from the pieces the
//! examples proved end to end:
//!
//! ```text
//! window (winit) -> RHI init -> attach_presentation -> scene assembly
//!   -> GeometryPass (items_for_scene) -> RenderGraph -> initialize
//!   -> every frame: dt -> controller -> scene mirror -> update_models
//!   -> update_uniforms -> render_frame (acquire -> draw -> present)
//! ```
//!
//! Input: the main menu is up first. Up/Down (or the pointer) select, Enter
//! activates, Escape backs out; in play WASD/QE walk, the arrows rotate,
//! left-button drag looks, and Escape pauses. The model comes from `RFS_MODEL`
//! (default `Model/TestModel/EXPLOSIVE.glb`), with a cube fallback when the
//! asset will not load, so a broken asset shows up as a note in the run log
//! instead of a crash.
//!
//! The sea is a CPU Gerstner grid drawn by the geometry pass; the same wave
//! field lifts and tilts the ship, so the hull follows the water that is
//! actually on screen.
//!
//! Diagnostics, same contract as the examples:
//! * `RFS_FRAME_LIMIT=n` renders n frames and exits;
//! * `RFS_RESIZE_SELFCHECK=n` resizes n times, printing the tracked object
//!   count per step — it must return to its baseline or the resize path leaks.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use glam::{Quat, Vec2, Vec3};
use rfs_client::render::camera::controller::CameraKey;
use rfs_client::render::game_loop::{
    build_scene, map_winit_key, mirror_camera, sync_camera_mirror, SceneRig,
};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::passes::geometry::GeometryPass;
use rfs_client::render::passes::ui::{UIConfig, UIPass};
use rfs_client::render::ui::{MenuCommand, MenuInput, MenuScreen, Screen, UiModel};
use rfs_client::render::{CameraController, Renderer};
use rhi::{RhiConfig, RhiError, RhiResult};
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::HasWindowHandle;

const SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/geometry.spv");
const TITLE: &str = "RFS - menu: arrows/Enter, play: WASD/QE + arrows, Esc pauses";

/// How often the loop prints a one-line status (frames).
const STATS_EVERY: u32 = 120;
/// Warm-up frames before the resize self-check starts counting objects.
const SELFCHECK_WARMUP_FRAMES: u32 = 5;
/// The settings slider's `0..=1` maps to a multiplier on the authored waves.
const WAVE_HEIGHT_SCALE: f32 = 1.5;

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

/// Translate the keys a menu understands. Escape is `Back` here; the caller
/// only asks while a menu is open, because in play Escape pauses instead.
fn key_to_menu_input(key: &Key) -> Option<MenuInput> {
    match key {
        Key::Named(NamedKey::ArrowUp) => Some(MenuInput::Up),
        Key::Named(NamedKey::ArrowDown) => Some(MenuInput::Down),
        Key::Named(NamedKey::ArrowLeft) => Some(MenuInput::Left),
        Key::Named(NamedKey::ArrowRight) => Some(MenuInput::Right),
        Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => Some(MenuInput::Activate),
        Key::Named(NamedKey::Escape) => Some(MenuInput::Back),
        _ => None,
    }
}

/// Drop every held movement key. Without this, tapping W and pausing leaves
/// the movement flag set, and resuming sends the camera flying on its own.
fn release_camera_keys(controller: &mut CameraController) {
    for key in [
        CameraKey::Forward,
        CameraKey::Backward,
        CameraKey::Left,
        CameraKey::Right,
        CameraKey::Up,
        CameraKey::Down,
        CameraKey::RotateLeft,
        CameraKey::RotateRight,
        CameraKey::RotateUp,
        CameraKey::RotateDown,
    ] {
        controller.handle_keyboard(key, false);
    }
}

/// Apply the effects that live outside the menu state machine. Returns
/// whether the application should quit.
fn apply_menu_command(command: MenuCommand, rig: &mut SceneRig) -> bool {
    match command {
        MenuCommand::WaveHeightChanged(value) => {
            if let Some(ocean) = rig.ocean.as_mut() {
                ocean
                    .ocean_mut()
                    .waves_mut()
                    .set_height_scale(value * WAVE_HEIGHT_SCALE);
            }
            false
        }
        MenuCommand::Quit => true,
        MenuCommand::StartGame
        | MenuCommand::Resume
        | MenuCommand::OpenSettings
        | MenuCommand::Back
        | MenuCommand::QuitToMenu => false,
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("main loop failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> RhiResult<()> {
    // ---------------------------------------------------------- the window
    let event_loop = winit::event_loop::EventLoop::new()
        .map_err(|e| RhiError::BackendError(format!("event loop: {e}")))?;
    let window = winit::window::WindowBuilder::new()
        .with_title(TITLE)
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        .build(&event_loop)
        .map_err(|e| RhiError::BackendError(format!("window: {e}")))?;

    // raw-window-handle 0.6 hands back a generic handle; on Windows the
    // HINSTANCE only lives on the window handle.
    let window_handle = window
        .window_handle()
        .map_err(|e| RhiError::BackendError(format!("window handle: {e}")))?;
    let (hwnd, hinstance) = match window_handle.as_raw() {
        winit::raw_window_handle::RawWindowHandle::Win32(win32) => (
            win32.hwnd.get() as u64,
            win32.hinstance
                .map(|h| h.get() as u64)
                .ok_or_else(|| {
                    RhiError::BackendError("Win32 window handle carries no HINSTANCE".into())
                })?,
        ),
        other => {
            return Err(RhiError::BackendError(format!(
                "expected a Win32 window handle, got {other:?}"
            )));
        }
    };

    // The window argument is unused by `new`: the surface is attached
    // afterwards from the handles.
    let mut renderer = Renderer::new(
        std::ptr::null_mut(),
        RhiConfig {
            validation: true,
            ..RhiConfig::default()
        },
    )?;
    let size = window.inner_size();
    renderer.attach_presentation(hinstance, hwnd, size.width.max(1), size.height.max(1))?;

    // Failure here would leave every present-reading pass drawing nothing;
    // attach_presentation publishes the typed views (Bug #645).
    let typed_views = renderer.context().presentation_texture_views.len();
    if typed_views == 0 {
        return Err(RhiError::BackendError(
            "attach_presentation must publish the typed presentation views; it left 0 \
             of them, so every pass that needs a typed view will return before drawing"
                .into(),
        ));
    }
    println!("[main] typed presentation views: {typed_views}");

    let Some(format) = renderer.presentation_format() else {
        return Err(RhiError::BackendError(
            "the surface format cannot be expressed by the RHI".into(),
        ));
    };

    // ------------------------------------------------------------ the scene
    let model_path = std::env::var("RFS_MODEL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("Model/TestModel/EXPLOSIVE.glb"));
    let rig = build_scene(renderer.device(), Some(&model_path));
    let mut rig = rig;
    for note in &rig.notes {
        println!("[main] {note}");
    }
    let ship_id = rig.ship_id;
    let camera_id = rig.camera_id;

    // The menu is the first screen. Its slider starts at 50%, and the sea has
    // to start at the same value or the first Settings visit would jump the
    // water.
    let mut menu_screen = MenuScreen::new();
    if let Some(ocean) = rig.ocean.as_mut() {
        ocean
            .ocean_mut()
            .waves_mut()
            .set_height_scale(menu_screen.wave_height() * WAVE_HEIGHT_SCALE);
    }
    let ui_model = Arc::new(Mutex::new(UiModel::new()));
    menu_screen.set_screen_size(
        renderer.presentation_extent().width as f32,
        renderer.presentation_extent().height as f32,
    );
    match ui_model.lock() {
        Ok(mut model) => menu_screen.sync(&mut model),
        Err(poisoned) => menu_screen.sync(&mut poisoned.into_inner()),
    }

    // The pass is built for the format the surface settled on, from the
    // items the scene produces now that the camera is active.
    let built = GeometryPass::items_for_scene(&rig.scene);
    for (name, reason) in &built.skipped {
        eprintln!("[main] {name} cannot be drawn: {reason}");
    }
    if built.items.is_empty() {
        return Err(RhiError::BackendError(
            "the scene produced nothing to draw".into(),
        ));
    }
    println!(
        "[main] {} item(s) from the scene, {} skipped",
        built.items.len(),
        built.skipped.len()
    );

    let (pass, state) = GeometryPass::create(renderer.device(), built.items, format, SHADER)?;
    renderer.render_graph_mut().add_pass(RenderPassNode {
        name: "geometry".into(),
        pass: Box::new(pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });

    // The UI is an overlay on the image the geometry pass produced. Its own
    // dependencies name passes this slice does not have; the graph ignores
    // unknown names, and node order keeps the overlay after the scene.
    let ui_pass = UIPass::new(UIConfig {
        // The green corner marker is a diagnostic for "does any pixel reach
        // the screen"; the menu itself answers that now.
        debug_marker: false,
        ..Default::default()
    })
    .with_model(ui_model.clone());
    renderer.render_graph_mut().add_pass(RenderPassNode {
        name: "ui".into(),
        pass: Box::new(ui_pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });
    // A graph that was never initialized records nothing and submits empty
    // frames; initialize makes that failure impossible, as the examples do.
    renderer.initialize(&mut rig.scene);

    // -------------------------------------------------------- the controller
    // The controller owns its camera (Box); the scene mirror is rebuilt from
    // it every frame. The initial aspect comes from the live extent.
    let extent = renderer.presentation_extent();
    let initial_aspect = extent.width as f32 / extent.height.max(1) as f32;
    let mut controller = CameraController::new(Box::new(mirror_camera(
        rfs_client::render::game_loop::camera_start(),
        0.0,
        0.0,
        initial_aspect,
    )));

    // ------------------------------------------------------ the diagnostics
    let frame_limit = env_u32("RFS_FRAME_LIMIT");
    let resize_steps: Option<usize> = env_u32("RFS_RESIZE_SELFCHECK").map(|v| v as usize);
    if let Some(steps) = resize_steps {
        println!("[main] resize self-check: {steps} steps");
    }
    if let Some(limit) = frame_limit {
        println!("[main] frame limit: {limit}");
    }
    let mut frames_done: u32 = 0;
    let mut resize_step: usize = 0;
    let mut baseline_objects: Option<usize> = None;
    let mut left_dragging = false;
    let mut last_cursor: Option<(f64, f64)> = None;
    let mut last_frame_time = Instant::now();
    let mut water_update_reported = false;

    // Replace the controller's camera with one rebuilt for the new aspect,
    // keeping the state the player has moved to. The controller has no
    // aspect setter: the camera is boxed and `PerspectiveCamera` carries the
    // aspect only at construction.
    let rebuild_controller_camera =
        |controller: &mut CameraController, width: u32, height: u32| {
            let aspect = width as f32 / height.max(1) as f32;
            let camera = controller.camera().position();
            *controller.camera_mut() = Box::new(mirror_camera(
                camera,
                controller.yaw,
                controller.pitch,
                aspect,
            ));
        };

    // ---------------------------------------------------------- the loop
    event_loop
        .run(move |event, control_flow| {
            control_flow.set_control_flow(winit::event_loop::ControlFlow::Poll);

            match event {
                winit::event::Event::WindowEvent { event, .. } => match event {
                    winit::event::WindowEvent::CloseRequested => control_flow.exit(),
                    winit::event::WindowEvent::KeyboardInput { event, .. } => {
                        use winit::event::ElementState;

                        let pressed = matches!(event.state, ElementState::Pressed);
                        if pressed && menu_screen.screen() == Screen::Playing {
                            if event.logical_key == Key::Named(NamedKey::Escape) {
                                menu_screen.open_pause();
                                release_camera_keys(&mut controller);
                                left_dragging = false;
                                last_cursor = None;
                            } else if !event.repeat {
                                if let Some(key) = map_winit_key(event.physical_key) {
                                    controller.handle_keyboard(key, true);
                                }
                            }
                        } else if pressed {
                            // Held arrows would race through the list; one
                            // press is one step, which is what a menu wants.
                            if !event.repeat {
                                if let Some(input) = key_to_menu_input(&event.logical_key) {
                                    if let Some(command) = menu_screen.handle(input) {
                                        if apply_menu_command(command, &mut rig) {
                                            control_flow.exit();
                                        }
                                    }
                                }
                            }
                        } else if menu_screen.screen() == Screen::Playing {
                            if let Some(key) = map_winit_key(event.physical_key) {
                                controller.handle_keyboard(key, false);
                            }
                        }
                    }
                    winit::event::WindowEvent::MouseInput {
                        button: winit::event::MouseButton::Left,
                        state,
                        ..
                    } => {
                        let pressed = matches!(state, winit::event::ElementState::Pressed);
                        if menu_screen.screen() == Screen::Playing {
                            left_dragging = pressed;
                            if !left_dragging {
                                last_cursor = None;
                            }
                        } else if pressed {
                            // `MouseInput` carries no position on this winit
                            // version. A click can only land on a button after
                            // a move put the cursor there, so the last move is
                            // the click's position.
                            let point =
                                last_cursor.map(|(x, y)| Vec2::new(x as f32, y as f32));
                            if let Some(point) = point {
                                if let Some(command) =
                                    menu_screen.handle(MenuInput::PointerPressed(point))
                                {
                                    if apply_menu_command(command, &mut rig) {
                                        control_flow.exit();
                                    }
                                }
                            }
                        }
                    }
                    // Mouse delta comes from successive cursor positions
                    // (DeltaPos with the button held, which winit 0.29 does
                    // not deliver as a window event at all).
                    winit::event::WindowEvent::CursorMoved { position, .. } => {
                        if menu_screen.screen() == Screen::Playing {
                            if left_dragging {
                                if let Some((previous_x, previous_y)) = last_cursor {
                                    controller.handle_mouse(
                                        (position.x - previous_x) as f32,
                                        (position.y - previous_y) as f32,
                                    );
                                }
                            }
                        } else {
                            let point = Vec2::new(position.x as f32, position.y as f32);
                            if let Some(command) = menu_screen.handle(MenuInput::PointerMoved(point))
                            {
                                if apply_menu_command(command, &mut rig) {
                                    control_flow.exit();
                                }
                            }
                        }
                        last_cursor = Some((position.x, position.y));
                    }
                    // Rebuilt lazily on the next frame: the swapchain cannot
                    // be recreated from inside the resize event reliably.
                    winit::event::WindowEvent::Resized(size) => {
                        rebuild_controller_camera(&mut controller, size.width, size.height);
                        menu_screen.set_screen_size(size.width as f32, size.height as f32);
                        renderer.resize(size.width.max(1), size.height.max(1));
                    }
                    // Going fullscreen changes the scale factor as well;
                    // winit delivers that separately from `Resized`.
                    winit::event::WindowEvent::ScaleFactorChanged { .. } => {
                        let size = window.inner_size();
                        rebuild_controller_camera(&mut controller, size.width, size.height);
                        menu_screen.set_screen_size(size.width as f32, size.height as f32);
                        renderer.resize(size.width.max(1), size.height.max(1));
                    }
                    _ => {}
                },
                winit::event::Event::AboutToWait => {
                    // Resize before drawing, so the frame being drawn is the
                    // one that rebuilds the chain.
                    if let Some(steps) = resize_steps {
                        if resize_step < steps && frames_done >= SELFCHECK_WARMUP_FRAMES {
                            let w = 640 + (resize_step as u32) * 37;
                            let h = 480 + (resize_step as u32) * 23;
                            rebuild_controller_camera(&mut controller, w, h);
                            renderer.resize(w, h);
                            resize_step += 1;
                        }
                    }

                    // A minimised window has no swapchain: skip until restored.
                    let extent = renderer.presentation_extent();
                    if extent.width != 0 && extent.height != 0 {
                        // The panel is in physical pixels and the pass draws
                        // in the same space, so one extent feeds both.
                        menu_screen.set_screen_size(extent.width as f32, extent.height as f32);
                        match ui_model.lock() {
                            Ok(mut model) => menu_screen.sync(&mut model),
                            Err(poisoned) => menu_screen.sync(&mut poisoned.into_inner()),
                        }

                        // dt first: the controller is the single camera
                        // writer. Clamp it so a dragged or minimised window
                        // does not teleport the camera when it comes back.
                        let dt = last_frame_time
                            .elapsed()
                            .min(std::time::Duration::from_millis(100));
                        last_frame_time = Instant::now();
                        controller.enabled = menu_screen.screen() == Screen::Playing;
                        if !controller.enabled {
                            left_dragging = false;
                        }
                        controller.update_with_mode(dt);

                        // The sea and its GPU grid. One update advances both,
                        // so buoyancy samples the same instant the picture
                        // shows.
                        if let Some(ocean) = rig.ocean.as_mut() {
                            if let Err(e) = ocean.update(renderer.device(), dt) {
                                if !water_update_reported {
                                    eprintln!("[main] ocean update failed: {e}");
                                    water_update_reported = true;
                                }
                            }
                        }

                        // The scene mirror, from the controller's state.
                        let aspect = extent.width as f32 / extent.height.max(1) as f32;
                        sync_camera_mirror(&mut rig.scene, camera_id, &controller, aspect);

                        // Buoyancy: the hull follows the height and slope of
                        // the water under it. Relative to the *base*
                        // transform, never to the previous frame — adding a
                        // displacement to the current position integrates it
                        // into a drift that outruns the sea.
                        let base = rig.ship_base_position;
                        let authored_rotation = rig.ship_base_rotation;
                        let (lift, tilt) = match rig.ocean.as_ref() {
                            Some(ocean) => {
                                let surface = ocean.ocean().height_at(base.x, base.z);
                                let level = ocean.ocean().level();
                                let normal = ocean.ocean().normal_at(base.x, base.z);
                                let tilt = Quat::IDENTITY
                                    .slerp(Quat::from_rotation_arc(Vec3::Y, normal), 0.55);
                                ((surface - level) * 0.85, tilt)
                            }
                            None => (0.0, Quat::IDENTITY),
                        };
                        if let Some(id) = ship_id {
                            for entity in rig.scene.entities_mut() {
                                if entity.id() != id {
                                    continue;
                                }
                                if let Some(transform) = entity.transform_mut() {
                                    transform.set_position(base + Vec3::Y * lift);
                                    transform.set_rotation(authored_rotation * tilt);
                                }
                            }
                        }
                        let moved = GeometryPass::update_models(&state, &rig.scene);
                        debug_assert!(
                            moved > 0,
                            "the scene's transforms did not reach the pass"
                        );

                        // View/projection straight from the controller's
                        // camera, which resize already rebuilt for the new
                        // aspect: one source of pixels-are-not-stretched.
                        let view = controller.camera().view_matrix();
                        let projection = controller.camera().projection_matrix();
                        if GeometryPass::update_uniforms(
                            &state,
                            renderer.device(),
                            view,
                            projection,
                        ) {
                            renderer.render_frame(&mut rig.scene);
                        } else {
                            eprintln!(
                                "[main] update_uniforms failed -> frame skipped (no render_frame)"
                            );
                        }
                    }

                    let frame_no = {
                        let n = frames_done;
                        frames_done = n + 1;
                        n
                    };
                    if frames_done % STATS_EVERY == 0 {
                        let camera = controller.camera().position();
                        println!(
                            "[main] frame {frames_done}: camera {:?} yaw {:.2} pitch {:.2} extent {}x{}",
                            camera,
                            controller.yaw,
                            controller.pitch,
                            extent.width,
                            extent.height
                        );
                    }
                    if let Some(limit) = frame_limit {
                        if frame_no >= limit {
                            control_flow.exit();
                        }
                    }

                    if let Some(steps) = resize_steps {
                        // Sampled after the frame that used the new size, so
                        // the replacement has already happened.
                        let objects = renderer.device().tracked_object_count();
                        match baseline_objects {
                            None => {
                                baseline_objects = Some(objects);
                                println!(
                                    "[main] baseline tracked objects: {objects} [{}]",
                                    renderer.device().tracked_object_summary()
                                );
                            }
                            Some(baseline) => {
                                let verdict = if objects == baseline { "ok" } else { "LEAK" };
                                println!(
                                    "[main] resize step {}/{steps}: {objects} tracked objects \
                                     (baseline {baseline}) -> {verdict} [{}]",
                                    resize_step,
                                    renderer.device().tracked_object_summary()
                                );
                            }
                        }
                        if resize_step >= steps && resize_step > 0 {
                            let baseline = baseline_objects.unwrap_or(0);
                            let objects = renderer.device().tracked_object_count();
                            println!(
                                "[main] resize self-check done: {objects} objects, baseline {baseline}"
                            );
                            control_flow.exit();
                        }
                    }
                }
                _ => {}
            }
        })
        .map_err(|e| RhiError::BackendError(format!("event loop: {e}")))?;

    Ok(())
}
