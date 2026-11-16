//! The RFS client's main game loop.
//!
//! **CLIENT-SIDE, CONNECTED TO THE SERVER THROUGH THE MENU**
//!
//! The owner's order: "make the main game loop so everything can be tested
//! and initialized", then "make the client connect to the server, with a
//! start menu". This binary is that loop, assembled from the pieces the
//! examples proved end to end:
//!
//! ```text
//! window (winit) -> RHI init -> attach_presentation -> scene assembly
//!   -> GeometryPass (items_for_scene) -> RenderGraph -> initialize
//!   -> every frame: dt -> controller -> scene mirror -> update_models
//!   -> update_uniforms -> render_frame (acquire -> draw -> present)
//!   -> net poll: bridge events -> menu screen; one input packet
//!                per observed server tick while Playing
//!   -> snapshot poll: take_latest_state -> ServerWorldView::apply + sync
//!                (the server's ships/players/shells become the drawn
//!                scene; the showcase ship stands down until the link ends)
//! ```
//!
//! The menu's Play starts a `NetBridge` (its own "rfs-net" thread; the loop
//! never awaits) and the connecting panel; a Connected event turns the
//! panel into the game, a failure or a lost link reads on the panel, and
//! Back or Quit to Menu stops the link. The server address comes from
//! `RFS_SERVER` (default 127.0.0.1:7777), the player name from `RFS_PLAYER`,
//! an optional token from `RFS_TOKEN`.
//!
//! Input: the main menu is up first. Up/Down (or the pointer) select, Enter
//! activates, Escape backs out; in play the pointer is captured and hidden so
//! moving the mouse looks around, WASD/QE walk, the arrows rotate, and Escape
//! pauses. Left-button drag is the fallback when the platform refuses the grab.
//! The model comes from `RFS_MODEL`
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
use rfs_client::net::{
    input_packet, resolve_server_addr, NetBridge, NetBridgeConfig, NetUpdate, DEFAULT_SERVER_ADDR,
};
use rfs_client::net_world::{ServerWorldView, SERVER_WAKE_RADIUS};
use rfs_client::render::camera::controller::CameraKey;
use rfs_client::render::game_loop::{
    build_scene, map_winit_key, mirror_camera, sync_camera_mirror, SceneRig,
};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::passes::geometry::GeometryPass;
use rfs_client::render::passes::ui::{UIConfig, UIPass};
use rfs_client::render::scene::components::MaterialComponent;
use rfs_client::render::ui::{MenuCommand, MenuInput, MenuScreen, Screen, UiModel};
use rfs_client::render::{CameraController, Renderer};
use rhi::{RhiConfig, RhiError, RhiResult};
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::HasWindowHandle;

const SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/geometry.spv");
const WATER_SHADER: &[u8] =
    include_bytes!("../Tool/client_tests/assets/shaders/water_surface.spv");
const SKY_SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/sky.spv");
const TITLE: &str = "RFS - menu: arrows/Enter, play: mouse looks + WASD/QE + arrows, Esc pauses";

/// How often the loop prints a one-line status (frames).
const STATS_EVERY: u32 = 120;
/// The frame autoplay presses Play on: late enough that the first frame
/// is drawn and the menu is settled.
const AUTOPLAY_AT_FRAME: u32 = 5;
/// Warm-up frames before the resize self-check starts counting objects.
const SELFCHECK_WARMUP_FRAMES: u32 = 5;
/// The settings slider's `0..=1` maps to a multiplier on the authored waves.
const WAVE_HEIGHT_SCALE: f32 = 1.5;

/// Where the camera parks when the server world takes over the picture:
/// the offset from our hull's drawn position (a little above and behind it,
/// so the hull and the sea around it fill the frame) and the matching pitch.
/// The yaw is 0, the controller's identity look, which stares at +Z — the
/// offset is negative-Z for exactly that.
const ANCHOR_CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 22.0, -85.0);
const ANCHOR_CAMERA_PITCH: f32 = -0.25;

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

/// A float from the environment, `0.0` when absent or unparsable. The
/// acceptance hooks read booleans from presence and levels from values.
fn env_f32(name: &str) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0)
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

/// Build the server-link configuration from the environment and start it.
///
/// `RFS_SERVER` (`host:port`, default 127.0.0.1:7777), `RFS_PLAYER`,
/// `RFS_TOKEN` (empty or unset is the unsigned dialect; the shipped
/// `server_config.json` runs unsigned, and a keyed server wants the same
/// secret here — see `server_src/README.md`).
fn start_net_bridge(address: &str) -> Result<NetBridge, String> {
    let server_addr = resolve_server_addr(address)?;
    let config = NetBridgeConfig {
        server_addr,
        player_name: std::env::var("RFS_PLAYER").unwrap_or_else(|_| "Player".to_string()),
        auth_token: std::env::var("RFS_TOKEN")
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty()),
    };
    NetBridge::start(config)
}

/// Apply the effects that live outside the menu state machine. Returns
/// whether the application should quit.
fn apply_menu_command(
    command: MenuCommand,
    rig: &mut SceneRig,
    menu_screen: &mut MenuScreen,
    net: &mut Option<NetBridge>,
) -> bool {
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
        MenuCommand::StartGame => {
            // The screen is already on the loading screen (the state
            // machine moved before returning the command). The bridge owns
            // its net thread; the frame loop polls its events. The big
            // line names the target before the first frame of the attempt.
            let address = std::env::var("RFS_SERVER")
                .unwrap_or_else(|_| DEFAULT_SERVER_ADDR.to_string());
            menu_screen.set_loading_name(&address);
            match start_net_bridge(&address) {
                Ok(bridge) => {
                    menu_screen.set_progress(0.10);
                    println!("[main] connecting to {address}...");
                    *net = Some(bridge);
                }
                Err(e) => {
                    // The loading screen stays up with Cancel; the reason
                    // sits in the status line instead of only in the
                    // console.
                    menu_screen.set_status_line(&format!("Cannot start: {e}"));
                    eprintln!("[main] {e}");
                }
            }
            false
        }
        MenuCommand::CancelConnect | MenuCommand::QuitToMenu => {
            // Leaving the session (or the attempt): stop the link. The
            // worker disconnects and joins before this returns.
            if let Some(bridge) = net.as_mut() {
                bridge.shutdown();
            }
            *net = None;
            false
        }
        MenuCommand::Resume | MenuCommand::OpenSettings | MenuCommand::Back => false,
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
    // `RFS_GPU` is a case-insensitive adapter-name substring, for choosing
    // between two discrete cards (or forcing the integrated one for a test).
    // With no override the renderer ranks adapters itself and prefers a
    // discrete GPU over an integrated one.
    let preferred_gpu = std::env::var("RFS_GPU")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let mut renderer = Renderer::new(
        std::ptr::null_mut(),
        RhiConfig {
            validation: true,
            preferred_gpu,
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

    // The server world's scene pool, attached BEFORE the geometry pass takes
    // its items: the pass builds one draw item per entity exactly once, so a
    // pool entity that missed that build could never be drawn. Idle slots
    // park at zero scale — degenerate triangles, no rasterizer work — and
    // the first snapshot claims them (`net_world` documents the contract).
    let player_name = std::env::var("RFS_PLAYER").unwrap_or_else(|_| "Player".to_string());
    let mut world_view = match ServerWorldView::attach(
        &mut rig.scene,
        renderer.device(),
        rig.ship_mesh.as_ref(),
        &player_name,
    ) {
        Ok(view) => {
            println!("[main] server world pool attached ({player_name})");
            Some(view)
        }
        Err(e) => {
            eprintln!("[main] the server world pool could not attach: {e}");
            None
        }
    };

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

    let (pass, state) = GeometryPass::create_with_water_pipeline(
        renderer.device(),
        built.items,
        format,
        SHADER,
        WATER_SHADER,
        SKY_SHADER,
    )?;
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
    // Diagnostics hook for scripted captures: ignore camera input so a
    // screenshot always shows the start pose, whatever keys the desktop is
    // sending. Menu navigation still works; only the in-game camera is pinned.
    let camera_lock = std::env::var_os("RFS_CAMERA_LOCK").is_some();
    if camera_lock {
        println!("[main] camera locked to the start pose");
    }
    let resize_steps: Option<usize> = env_u32("RFS_RESIZE_SELFCHECK").map(|v| v as usize);
    if let Some(steps) = resize_steps {
        println!("[main] resize self-check: {steps} steps");
    }
    // Diagnostics hook for the connection smoke run: activate the menu's
    // Play item a few frames in, so an unattended run reaches the server
    // the way a player would. Nothing else is scripted — whatever happens
    // after (accept, refuse, silence) is the honest wire behavior.
    let autoplay = std::env::var_os("RFS_AUTOPLAY").is_some();
    if autoplay {
        println!("[main] autoplay: the menu's Play will activate itself");
    }
    // The acceptance hook for the connection: while the server world owns
    // the picture, the input packet's forward axis is forced on, so the
    // server throttles the hull the client's player rides without anyone
    // touching the keyboard (a camera-lock run stays fully unattended). It
    // touches only the packet — never the camera.
    let autoplay_throttle = env_f32("RFS_AUTOPLAY_THROTTLE");
    if autoplay_throttle > 0.0 {
        println!("[main] autoplay throttle: the connected hull holds ahead");
    }
    if let Some(limit) = frame_limit {
        println!("[main] frame limit: {limit}");
    }
    let mut frames_done: u32 = 0;
    let mut resize_step: usize = 0;
    let mut baseline_objects: Option<usize> = None;
    let mut left_dragging = false;
    let mut last_cursor: Option<(f64, f64)> = None;
    // FPS-style look: while playing, the pointer is confined to the window and
    // hidden, and raw mouse motion turns the camera. There is no re-centring:
    // cursor-position feedback was what made the view jerk. Left-drag remains
    // the fallback if the platform refuses the grab.
    let mut mouse_captured = false;
    let mut mouse_capture_unavailable = false;
    let mut last_frame_time = Instant::now();
    let mut water_update_reported = false;
    // The server link, created by the menu's Play. The loop polls its events
    // every frame and feeds it one input packet per observed server tick.
    let mut net_bridge: Option<NetBridge> = None;
    let mut last_input_tick: u32 = 0;
    // Bug №783: the forward value last put on the wire, so a change is visible
    // where the packet is built. NaN until the first packet.
    let mut last_sent_forward = f32::NAN;
    // The shader clock. It advances with the same clamped `dt` as the waves,
    // so the ripple pattern and the mesh never disagree about how much time
    // has passed.
    let mut water_time = 0.0f32;
    // The server world's takeover state: whether a snapshot has been applied
    // to the scene (the picture is the server's world now) and whether the
    // anchor teleport has run. The takeover reverses itself when the link
    // ends — Cancel, Quit to Menu, failure, lost link — and the showcase
    // scene comes back exactly as it was.
    let mut world_taken_over = false;
    let mut camera_intro_done = false;

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
                // Raw mouse motion, not cursor position: the deltas stay
                // correct even when a confined cursor stops at a window edge,
                // and there is no re-centring feedback to make the view jerk.
                // Winit's default device-event filter is "when focused".
                winit::event::Event::DeviceEvent {
                    event: winit::event::DeviceEvent::MouseMotion { delta },
                    ..
                } => {
                    if mouse_captured
                        && menu_screen.screen() == Screen::Playing
                        && !camera_lock
                    {
                        controller.handle_mouse(delta.0 as f32, delta.1 as f32);
                    }
                }
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
                            } else if !event.repeat && !camera_lock {
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
                                        if apply_menu_command(command, &mut rig, &mut menu_screen, &mut net_bridge) {
                                            control_flow.exit();
                                        }
                                    }
                                }
                            }
                        } else if menu_screen.screen() == Screen::Playing && !camera_lock {
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
                            // With the pointer captured, buttons are not part
                            // of looking; only the fallback drag reads them.
                            if !mouse_captured {
                                left_dragging = pressed && !camera_lock;
                                if !left_dragging {
                                    last_cursor = None;
                                }
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
                                    if apply_menu_command(command, &mut rig, &mut menu_screen, &mut net_bridge) {
                                        control_flow.exit();
                                    }
                                }
                            }
                        }
                    }
                    // Fallback look for platforms that refuse the pointer
                    // grab: successive cursor positions while the left button
                    // is held (winit 0.29 has no button-held delta event).
                    // When the grab works, raw `MouseMotion` above is used and
                    // this path is skipped entirely.
                    winit::event::WindowEvent::CursorMoved { position, .. } => {
                        if menu_screen.screen() == Screen::Playing {
                            // With the pointer captured, raw `MouseMotion`
                            // drives the camera; cursor position is irrelevant
                            // and re-centring it here caused visible jerks.
                            if !mouse_captured {
                                if left_dragging {
                                    if let Some((previous_x, previous_y)) = last_cursor {
                                        controller.handle_mouse(
                                            (position.x - previous_x) as f32,
                                            (position.y - previous_y) as f32,
                                        );
                                    }
                                    last_cursor = Some((position.x, position.y));
                                } else {
                                    last_cursor = Some((position.x, position.y));
                                }
                            }
                        } else {
                            let point = Vec2::new(position.x as f32, position.y as f32);
                            if let Some(command) = menu_screen.handle(MenuInput::PointerMoved(point))
                            {
                                if apply_menu_command(command, &mut rig, &mut menu_screen, &mut net_bridge) {
                                    control_flow.exit();
                                }
                            }
                            last_cursor = Some((position.x, position.y));
                        }
                    }
                    // Alt-tabbing must give the desktop its pointer back, or
                    // the user is left with an invisible, confined cursor.
                    winit::event::WindowEvent::Focused(false) => {
                        left_dragging = false;
                        last_cursor = None;
                        if mouse_captured {
                            mouse_captured = false;
                            let _ =
                                window.set_cursor_grab(winit::window::CursorGrabMode::None);
                            window.set_cursor_visible(true);
                        }
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
                    // The server bridge first: its events decide whether the
                    // connecting panel becomes the game, an error line, or a
                    // lost-connection panel. Polled before the UI sync, so a
                    // screen change renders on this very frame.
                    let mut retire_bridge = false;
                    if let Some(bridge) = net_bridge.as_mut() {
                        for update in bridge.poll() {
                            match update {
                                NetUpdate::Connected { server_tick } => {
                                    // Not into the game yet: the loading
                                    // screen stays until the world arrives
                                    // (the Squad flow — connect, load the
                                    // map, then drop in). The bar's stage
                                    // moves; the next stage is a snapshot.
                                    menu_screen.set_status_line("Connected — loading the world...");
                                    menu_screen.set_progress(0.55);
                                    last_input_tick = 0;
                                    println!(
                                        "[main] server accepted us, world tick {server_tick}"
                                    );
                                }
                                NetUpdate::Failed(reason) => {
                                    menu_screen
                                        .set_status_line(&format!("Could not connect: {reason}"));
                                    eprintln!("[main] connect failed: {reason}");
                                    retire_bridge = true;
                                }
                                NetUpdate::Lost(reason) => {
                                    // The loading screen reopens wherever we
                                    // were (in play or in pause) with the
                                    // reason, and Cancel does the rest.
                                    menu_screen
                                        .open_connecting(&format!("Connection lost: {reason}"));
                                    eprintln!("[main] connection lost: {reason}");
                                    retire_bridge = true;
                                }
                                NetUpdate::Announcement(text) => {
                                    println!("[server] {text}");
                                }
                            }
                        }
                        // The world arrives through the coalesced slot, not
                        // the event queue. The first snapshot is what the
                        // loading screen has been waiting for — and this is
                        // also where connecting finally becomes visible:
                        // the snapshot's entities are mapped into the scene
                        // pool, the showcase ship stands down, the sea gets
                        // a server-scale wake collar, and the camera moves
                        // to our hull. (`connect_succeeded` only acts from
                        // Connecting.)
                        if bridge.is_connected() {
                            if let Some(state) = bridge.take_latest_state() {
                                // Our identity on the wire: the server
                                // echoes the ConnectAccept id in every
                                // player's `player_id`. That — not the
                                // display name, which the server shares
                                // among connections — binds the camera
                                // anchor to our own hull.
                                if let Some(view) = world_view.as_mut() {
                                    if let Some(id) = bridge.client().connection_id() {
                                        view.set_connection_id(id);
                                    }
                                }
                                let applied = match (
                                    world_view.as_mut(),
                                    rig.ocean.as_ref().map(|ocean| ocean.ocean()),
                                ) {
                                    (Some(view), Some(ocean)) => {
                                        view.apply(&state, ocean);
                                        view.sync(&mut rig.scene);
                                        true
                                    }
                                    (Some(_), None) => {
                                        // No sea, no client-space mapping to
                                        // draw against; the log already said
                                        // the ocean failed to build.
                                        false
                                    }
                                    (None, _) => false,
                                };
                                if applied {
                                    world_taken_over = true;
                                    menu_screen.connect_succeeded();
                                    // The camera frames our hull exactly
                                    // once — the intro — and then rides it:
                                    // every applied snapshot re-anchors the
                                    // camera to the hull's newest drawn
                                    // position, because the hull sails and a
                                    // one-shot teleport would leave it
                                    // watching an empty patch of sea while
                                    // the ship runs away in the picture.
                                    let extent = renderer.presentation_extent();
                                    let aspect = extent.width as f32
                                        / extent.height.max(1) as f32;
                                    if !camera_intro_done {
                                        if let Some(anchor) =
                                            world_view.as_ref().and_then(|view| view.anchor())
                                        {
                                            let position = anchor + ANCHOR_CAMERA_OFFSET;
                                            *controller.camera_mut() = Box::new(mirror_camera(
                                                position,
                                                0.0,
                                                ANCHOR_CAMERA_PITCH,
                                                aspect,
                                            ));
                                            controller.yaw = 0.0;
                                            controller.pitch = ANCHOR_CAMERA_PITCH;
                                            camera_intro_done = true;
                                            println!(
                                                "[main] server world took over: camera -> {position:?}"
                                            );
                                        }
                                    } else if let Some(anchor) =
                                        world_view.as_ref().and_then(|view| view.anchor())
                                    {
                                        // The ride: the offset stays fixed in
                                        // world axes, so the camera trails the
                                        // hull at the same distance and heading
                                        // no matter how far it sails. The
                                        // player's look angles are preserved —
                                        // the view turns where they look, from
                                        // a position that follows the hull.
                                        // (`RFS_CAMERA_LOCK` zeroes the angles
                                        // every frame on its own.)
                                        let position = anchor + ANCHOR_CAMERA_OFFSET;
                                        *controller.camera_mut() = Box::new(mirror_camera(
                                            position,
                                            controller.yaw,
                                            controller.pitch,
                                            aspect,
                                        ));
                                    }
                                    // The showcase ship stands down: the
                                    // server's hulls own the picture now.
                                    // Buoyancy owns position and rotation,
                                    // so collapsing the scale is the whole
                                    // hide; the base scale restores it.
                                    if let Some(id) = ship_id {
                                        if let Some(entity) = rig.scene.get_entity_mut(id) {
                                            if let Some(transform) = entity.transform_mut() {
                                                transform.set_scale(Vec3::ZERO);
                                            }
                                        }
                                    }
                                    // A hull at server scale churns a wider
                                    // collar than the showcase model. The
                                    // water shader reads the radius from the
                                    // material's wake uniform.
                                    if let Some(water_id) = rig.water_id {
                                        if let Some(entity) = rig.scene.get_entity_mut(water_id) {
                                            if let Some(component) =
                                                entity.get_component_mut::<MaterialComponent>()
                                            {
                                                if let Some(water) =
                                                    component.water_material.as_mut()
                                                {
                                                    water.set_wake_radius(SERVER_WAKE_RADIUS);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if retire_bridge {
                        if let Some(bridge) = net_bridge.as_mut() {
                            bridge.shutdown();
                        }
                        net_bridge = None;
                    }
                    // The session is over, however it ended — Cancel, Quit
                    // to Menu, a failed attempt, a lost link. If the server
                    // world had taken the picture over, it stands down now:
                    // the pool parks, the showcase ship's scale comes back
                    // (buoyancy kept position and rotation correct all
                    // along), and the wake collar returns to the showcase
                    // hull's own.
                    if world_taken_over && net_bridge.is_none() {
                        if let Some(view) = world_view.as_mut() {
                            view.reset();
                            view.sync(&mut rig.scene);
                        }
                        if let Some(id) = ship_id {
                            if let Some(entity) = rig.scene.get_entity_mut(id) {
                                if let Some(transform) = entity.transform_mut() {
                                    transform.set_scale(Vec3::splat(rig.ship_base_scale));
                                }
                            }
                        }
                        if let Some(water_id) = rig.water_id {
                            if let Some(entity) = rig.scene.get_entity_mut(water_id) {
                                if let Some(component) =
                                    entity.get_component_mut::<MaterialComponent>()
                                {
                                    if let Some(water) = component.water_material.as_mut() {
                                        water.set_wake_radius(0.0);
                                    }
                                }
                            }
                        }
                        world_taken_over = false;
                        camera_intro_done = false;
                        println!("[main] server world stood down: the showcase scene is back");
                    }

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
                        water_time += dt.as_secs_f32();
                        controller.enabled = menu_screen.screen() == Screen::Playing;
                        if !controller.enabled {
                            left_dragging = false;
                        }

                        // Grab the pointer only while the game owns the view:
                        // menus need an ordinary cursor, play wants FPS look.
                        // `has_focus` keeps a background window from stealing
                        // the cursor; losing focus releases it in the handler.
                        let wants_mouse =
                            controller.enabled && !camera_lock && window.has_focus();
                        if wants_mouse && !mouse_captured && !mouse_capture_unavailable {
                            match window
                                .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                            {
                                Ok(()) => {
                                    mouse_captured = true;
                                    window.set_cursor_visible(false);
                                    last_cursor = None;
                                    left_dragging = false;
                                }
                                Err(e) => {
                                    // Some platforms refuse confinement. The
                                    // left-button drag below still works; say
                                    // so once instead of retrying every frame.
                                    mouse_capture_unavailable = true;
                                    eprintln!(
                                        "[main] cursor capture unavailable ({e}); \
                                         hold the left button to look"
                                    );
                                }
                            }
                        } else if !wants_mouse && mouse_captured {
                            mouse_captured = false;
                            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                            window.set_cursor_visible(true);
                            left_dragging = false;
                            last_cursor = None;
                        }
                        controller.update_with_mode(dt);
                        if camera_lock {
                            controller.yaw = 0.0;
                            controller.pitch = 0.0;
                        }

                        // Server input rides the observed tick: one packet
                        // per server tick, the cadence the stress bots use.
                        // A player holding no keys still sends the packet —
                        // the server would otherwise time a silent one out.
                        if menu_screen.screen() == Screen::Playing {
                            if let Some(bridge) = net_bridge.as_ref() {
                                if bridge.is_connected() {
                                    let tick = (bridge.client().server_tick().0
                                        & 0xFFFF_FFFF) as u32;
                                    if tick != last_input_tick {
                                        last_input_tick = tick;
                                        let mut movement = controller.movement();
                                        // The acceptance hook: a connected
                                        // run with the throttle set holds
                                        // ahead without a keyboard. It touches
                                        // only the packet's forward axis —
                                        // the camera stays wherever the run
                                        // left it.
                                        if world_taken_over && autoplay_throttle > 0.0 {
                                            movement.forward = true;
                                        }
                                        // Bug №783: name the wire value where it
                                        // is built; change-only, so a steady
                                        // held-ahead run prints once.
                                        let forward = if movement.forward {
                                            1.0
                                        } else {
                                            0.0
                                        } + if movement.backward {
                                            -1.0
                                        } else {
                                            0.0
                                        };
                                        if (last_sent_forward - forward).abs() > f32::EPSILON {
                                            last_sent_forward = forward;
                                            println!(
                                                "[main] frame {frames_done}: input forward {forward:+.1}"
                                            );
                                        }
                                        bridge.client().send_input(input_packet(
                                            tick,
                                            dt.as_secs_f32(),
                                            &movement,
                                        ));
                                    }
                                }
                            }
                        }

                        // The server world rides on: the sea's window slides
                        // under our hull (so a connected client walking a
                        // kilometre-scale world keeps the sea), and the pool
                        // entities get their transforms rewritten — the
                        // pass's per-frame matrix refresh carries them to the
                        // GPU. This must run BEFORE `ocean.update`, which
                        // uploads the grid from the centre it now has. The
                        // showcase scene keeps the sea at the origin.
                        if world_taken_over {
                            if let (Some(view), Some(ocean)) =
                                (world_view.as_mut(), rig.ocean.as_mut())
                            {
                                let focus = view
                                    .anchor()
                                    .unwrap_or_else(|| controller.camera().position());
                                ocean.ocean_mut().recenter_toward(focus);
                                view.sync(&mut rig.scene);
                            }
                        }

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
                        // The wake's focus point: the showcase hull's
                        // waterline normally, our server hull's drawn
                        // position once the connection owns the picture.
                        let wake_focus = if world_taken_over {
                            world_view
                                .as_ref()
                                .and_then(|view| view.anchor())
                                .unwrap_or(base + Vec3::Y * lift)
                        } else {
                            base + Vec3::Y * lift
                        };
                        // The water material lives on the ocean entity, which
                        // is also what tells the pass to use the water
                        // pipeline. The borrow ends before `render_frame`
                        // needs the scene mutably.
                        let uniforms_ok = {
                            let water_material = rig
                                .water_id
                                .and_then(|id| rig.scene.get_entity(id))
                                .and_then(|entity| entity.get_component::<MaterialComponent>())
                                .and_then(|component| component.get_water());
                            let waves = rig.ocean.as_ref().map(|ocean| ocean.ocean().waves());
                            GeometryPass::update_uniforms_with_water(
                                &state,
                                renderer.device(),
                                view,
                                projection,
                                water_time,
                                water_material,
                                waves,
                                Some(wake_focus),
                            )
                        };
                        if uniforms_ok {
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
                    // Autoplay: press Play once, a few frames in, when the
                    // first frame has certainly been drawn and the menu is
                    // settled. The same `handle` -> `apply_menu_command`
                    // path a real key press takes, so the diagnostic
                    // exercises the production wiring, not a shortcut.
                    if autoplay && frame_no == AUTOPLAY_AT_FRAME {
                        if let Some(command) = menu_screen.handle(MenuInput::Activate) {
                            if apply_menu_command(
                                command,
                                &mut rig,
                                &mut menu_screen,
                                &mut net_bridge,
                            ) {
                                control_flow.exit();
                            }
                        }
                    }
                    if frames_done % STATS_EVERY == 0 {
                        let camera = controller.camera().position();
                        // The link's vitals ride the same line, so a live
                        // session is visible in the log without a debugger.
                        let net_note = match net_bridge.as_ref() {
                            Some(bridge) if bridge.is_connected() => format!(
                                ", connected: {} fresh worlds, {} input acks",
                                bridge.snapshots_seen(),
                                bridge.input_acks()
                            ),
                            Some(_) => ", connecting...".to_string(),
                            None => String::new(),
                        };
                        // The drawn server world rides the same line: what
                        // the takeover is showing right now.
                        let world_note = match world_view.as_ref() {
                            Some(view) if world_taken_over => {
                                let drive = view
                                    .hull_drive()
                                    .map(|(throttle, speed)| {
                                        format!(", our hull: throttle {throttle:.2} speed {speed:.2}")
                                    })
                                    .unwrap_or_default();
                                let anchor = view
                                    .anchor()
                                    .map(|a| format!(", anchor ({:.1}, {:.1}, {:.1})", a.x, a.y, a.z))
                                    .unwrap_or_default();
                                format!(
                                    ", world: {} ({} snapshots){anchor}{drive}",
                                    view.drawn_summary(),
                                    view.applied_snapshots()
                                )
                            }
                            _ => String::new(),
                        };
                        println!(
                            "[main] frame {frames_done}: camera {:?} yaw {:.2} pitch {:.2} extent {}x{} screen {:?}{net_note}{world_note}",
                            camera,
                            controller.yaw,
                            controller.pitch,
                            extent.width,
                            extent.height,
                            menu_screen.screen()
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
