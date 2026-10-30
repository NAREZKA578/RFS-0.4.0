//! Does the UI pass draw what the `UiModel` compiled, and nothing it did not
//! ask for?
//!
//! Bug №724 wired `render/ui` — a data model with no producer — to `UIPass`,
//! which until then drew only its debug marker. The compile step sits between
//! two things that already had their own exact tests: the alignment math the
//! models document, and the pixel proof `ui_pixels.rs` established that a
//! `UiCommand` list reaches a framebuffer. What nothing covered was the joint,
//! in both directions:
//!
//!  - the compiled stream must be exactly the geometry the models describe —
//!    panel row pitches, bar insets, selected-colour rules — drift there shows
//!    on screen as a misplaced panel or a wrong-looking bar, not as an error
//!    anywhere;
//!  - the pass must compile *only* the parts its config gates open, register
//!    the textures those parts name, and hand the batch to the renderer before
//!    recording — the pass is where an over-eager compile (drawing what the
//!    config switched off) or an under-eager one (losing a model) could hide.
//!
//! Deliberately two sections. The CPU half asserts the exact command stream,
//! command by command — every constant cited here is defined once in the model
//! files and quoted here, which is the contract: change the constant, the test
//! name names the panel whose geometry moved. The GPU half drives the real
//! `UIPass::execute` against a hand-built headless context and reads both the
//! outcome and the renderer's per-frame counters; the pixel proof then runs a
//! compiled batch through an offscreen renderer and counts lit pixels on an
//! exact number, the discipline `ui_pixels.rs` states.
//!
//! What this file does *not* assert: validation errors. The SDK's validation
//! layer directory is missing from this machine (`validation_error_count`
//! asserts across the suite are vacuous), so `ui_pixels.rs`'s validator guards
//! are not copied — a guard that always passes would only fake coverage. The
//! pixel and counter assertions carry the proof instead.

use glam::Vec2;
use rfs_client::rhi::command::commands::Command;
use rfs_client::rhi::command::{
    CommandBuffer, CommandBufferDesc, CommandEncoder, CommandPool, CommandPoolDesc,
};
use rfs_client::rhi::ui::UiCommand;
use rfs_client::rhi::{Format, TextureUsage};
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rfs_client::render::graph::node::PassOutcome;
use rfs_client::render::graph::RenderPass as _;
use rfs_client::render::passes::ui::{UIConfig, UIPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::textures::{Texture, TextureFormat, TextureUsage as RenderTextureUsage};
use rfs_client::render::ui::hud::{
    HUDAlignment, HUDDamageIndicator, HUDObjective, HUDElement, ShipHUDInfo,
};
use rfs_client::render::ui::menu::{MenuItem, MenuItemType, MenuType};
use rfs_client::render::ui::minimap::{MinimapElementType, MinimapElement};
use rfs_client::render::ui::{HUD, Menu, Minimap, UiBatch, UiGates, UiModel};
use rfs_client::render::RenderContext;

// ---------------------------------------------------------------------------
// CPU: the compiled command stream, asserted exactly.
// ---------------------------------------------------------------------------

/// An element that carries no texture and no mesh compiles to a flat quad at
/// its alignment-resolved position. This is also the shape the crosshair
/// takes, so nearly every later test builds on it.
#[test]
fn a_plain_hud_element_compiles_to_a_flat_quad() {
    let mut hud = HUD::new(640, 480);
    hud.add_element(
        HUDElement::new("frame")
            .with_position(Vec2::new(20.0, 30.0))
            .with_size(Vec2::new(10.0, 10.0))
            .with_alignment(HUDAlignment::TopLeft),
    );

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);

    assert!(
        batch.textures.is_empty(),
        "a texture-less element must not register textures, got {:?}",
        batch.textures
    );
    assert_eq!(batch.commands.len(), 1, "one element, one command");
    assert_eq!(
        batch.commands[0],
        UiCommand::Rectangle {
            x: 20.0,
            y: 30.0,
            width: 10.0,
            height: 10.0,
            color: [1.0, 1.0, 1.0, 1.0],
            corner_radius: 0.0,
        },
        "a TopLeft element sits exactly where it is put"
    );
}

/// The alignment math is load-bearing: centres and edges resolve through
/// `calculate_position`, and a wrong offset there shows as a panel hugging the
/// wrong corner. Asserted on exact coordinates.
#[test]
fn alignment_math_lands_where_it_promises() {
    let mut hud = HUD::new(640, 480);

    hud.add_element(
        HUDElement::new("right")
            .with_position(Vec2::new(20.0, 30.0))
            .with_size(Vec2::new(10.0, 10.0))
            .with_alignment(HUDAlignment::TopRight),
    );
    hud.add_element(
        HUDElement::new("middle")
            .with_position(Vec2::new(10.0, 10.0))
            .with_size(Vec2::new(10.0, 10.0))
            .with_alignment(HUDAlignment::Center),
    );
    hud.add_element(
        HUDElement::new("corner")
            .with_position(Vec2::new(20.0, 30.0))
            .with_size(Vec2::new(10.0, 10.0))
            .with_alignment(HUDAlignment::BottomRight),
    );

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);

    let at = |index: usize| match &batch.commands[index] {
        UiCommand::Rectangle { x, y, .. } => (*x, *y),
        other => panic!("expected a Rectangle, got {other:?}"),
    };
    assert_eq!(batch.commands.len(), 3);
    assert_eq!(at(0), (620.0, 30.0), "TopRight hugs the right edge (640 - 20)");
    assert_eq!(at(1), (330.0, 250.0), "Center stops at the midpoints");
    assert_eq!(at(2), (620.0, 450.0), "BottomRight is mirrored on both axes");
}

/// Elements draw stable-sorted by `z_index` ascending; the crosshair follows
/// them; an element with `visible = false` compiles nothing.
///
/// The order is asserted, not "some order": a HUD whose health bar drew under
/// its panel background until the sort went in would pass every count
/// assertion and still look wrong.
#[test]
fn hud_elements_compile_in_z_order_and_respect_visibility() {
    let mut hud = HUD::new(640, 480);
    hud.add_element(
        HUDElement::new("late")
            .with_position(Vec2::new(100.0, 100.0))
            .with_alignment(HUDAlignment::TopLeft)
            .with_z_index(2),
    );
    hud.add_element(
        HUDElement::new("hidden")
            .with_position(Vec2::new(200.0, 200.0))
            .with_alignment(HUDAlignment::TopLeft)
            .with_visible(false),
    );
    hud.add_element(
        HUDElement::new("early")
            .with_position(Vec2::new(0.0, 0.0))
            .with_alignment(HUDAlignment::TopLeft)
            .with_z_index(0),
    );
    hud.set_crosshair(
        HUDElement::new("crosshair")
            .with_position(Vec2::new(315.0, 235.0))
            .with_size(Vec2::new(10.0, 10.0))
            .with_alignment(HUDAlignment::TopLeft),
    );

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);

    let at = batch
        .commands
        .iter()
        .filter_map(|c| match c {
            UiCommand::Rectangle { x, .. } => Some(*x),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        at,
        vec![0.0, 100.0, 315.0],
        "expected [early z0, late z2, crosshair], got {at:?}"
    );
}

/// The compiled ship info panel, quoted exactly: one background rectangle and
/// fixed-pitch rows, bars at a fixed inset. The constants live in `hud.rs`;
/// the test name names the panel whose geometry moved if a constant does.
///
/// `max_fuel = 0` is the NaN guard: a zero denominator must compile `0.0`,
/// because a NaN `progress` survives the renderer's own clamp and reaches the
/// GPU as NaN geometry.
#[test]
fn the_ship_info_panel_compiles_its_documented_geometry() {
    let mut hud = HUD::new(1920, 1080);
    hud.set_ship_info(ShipHUDInfo {
        name: "Probe".to_string(),
        hp: 128.0,
        max_hp: 512.0,
        speed: 100.0,
        max_speed: 200.0,
        fuel: 10.0,
        max_fuel: 0.0, // the degenerate denominator
        crew_count: 3,
        max_crew: 8,
        ..Default::default()
    });

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);

    // Five rows of 18px pitch: the panel is 240x90, bottom-left at
    // (16, 1080 - 16 - 90) = (16, 974).
    assert_eq!(batch.commands.len(), 9, "background + 5 row texts + 3 bars");
    assert_eq!(
        batch.commands[0],
        UiCommand::Rectangle {
            x: 16.0,
            y: 974.0,
            width: 240.0,
            height: 90.0,
            color: [0.05, 0.05, 0.08, 0.85],
            corner_radius: 0.0,
        },
        "the panel is 240x90 at the bottom-left margin"
    );
    assert_eq!(
        batch.commands[1],
        UiCommand::Text {
            content: "Probe".to_string(),
            x: 24.0, // PANEL_MARGIN + text pad
            y: 976.0,
            size: 14.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: None,
        },
        "the name is the panel's first row"
    );
    // Command order inside the panel: [0] bg, then per row the label text
    // followed by its bar where the row has one: [1] name, [2] "HP", [3] HP
    // bar, [4] "SPD", [5] SPD bar, [6] "FUEL", [7] FUEL bar, [8] crew text.
    assert_eq!(
        batch.commands[2],
        UiCommand::Text {
            content: "HP".to_string(),
            x: 24.0,
            y: 994.0,
            size: 14.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: None,
        }
    );
    assert_eq!(
        batch.commands[3],
        UiCommand::ProgressBar {
            x: 112.0, // PANEL_MARGIN + BAR_INSET
            y: 996.0,
            width: 136.0,
            height: 10.0,
            progress: 0.25, // 128 / 512
            background_color: [0.2, 0.2, 0.25, 1.0],
            fill_color: [0.3, 0.8, 0.3, 1.0],
            corner_radius: 0.0,
        },
        "the HP row compiles its bar at 0.25 fill"
    );
    assert_eq!(
        batch.commands[7],
        UiCommand::ProgressBar {
            x: 112.0,
            y: 974.0 + 3.0 * 18.0 + 4.0, // the FUEL row is the fourth
            width: 136.0,
            height: 10.0,
            progress: 0.0,
            background_color: [0.2, 0.2, 0.25, 1.0],
            fill_color: [0.3, 0.8, 0.3, 1.0],
            corner_radius: 0.0,
        },
        "a zero-max bar clamps to 0.0 and stays finite, never NaN"
    );
    assert_eq!(
        batch.commands[8],
        UiCommand::Text {
            content: "CREW 3/8".to_string(),
            x: 24.0,
            y: 974.0 + 4.0 * 18.0 + 2.0, // the crew line is the fifth row
            size: 14.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: None,
        },
        "the crew line closes the panel"
    );
}

/// A default HUD — no elements, empty info names, no compass — compiles
/// nothing. "The model is empty" is an assertable fact, and the pass relies on
/// it: marker-only frames are what a fresh boot shows.
#[test]
fn a_default_hud_compiles_nothing() {
    let mut batch = UiBatch::default();
    HUD::new(1920, 1080).to_commands(&mut batch);
    assert!(
        batch.commands.is_empty(),
        "an empty HUD published {:?} commands",
        batch.commands.len()
    );
    assert!(batch.textures.is_empty());
}

/// Panels draw only when they carry data: a weapon with an empty name is not
/// on screen. Together with the default-HUD test this pins the gate to the
/// name the game actually publishes, not to the struct existing.
#[test]
fn an_empty_name_keeps_its_panel_off_screen() {
    let mut hud = HUD::new(1920, 1080);
    hud.set_ship_info(ShipHUDInfo::default()); // name "" — the untouched default
    hud.set_weapon_info(Default::default());

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);
    assert!(
        batch.commands.is_empty(),
        "info structs without names compiled {:?}",
        batch.commands
    );
}

/// Objectives stack down the top-right corner unless their own position says
/// otherwise, and the completed flag picks the colour.
#[test]
fn objectives_stack_and_their_done_flag_colours_them() {
    let mut hud = HUD::new(1920, 1080);
    hud.add_objective(HUDObjective {
        title: "Reach the bay".to_string(),
        description: String::new(),
        completed: false,
        position: Vec2::ZERO,
        visible: true,
    });
    hud.add_objective(HUDObjective {
        title: "Docked".to_string(),
        description: String::new(),
        completed: true,
        position: Vec2::new(700.0, 90.0), // explicit position wins over the stack
        visible: true,
    });
    hud.add_objective(HUDObjective {
        title: "Not shown".to_string(),
        description: String::new(),
        completed: false,
        position: Vec2::ZERO,
        visible: false,
    });

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);

    let as_text = |command: &UiCommand| match command {
        UiCommand::Text { content, x, y, color, .. } => (content.clone(), (*x, *y), *color),
        other => panic!("expected a Text, got {other:?}"),
    };
    assert_eq!(batch.commands.len(), 2, "the invisible objective compiles nothing");
    assert_eq!(as_text(&batch.commands[0]).1, (1672.0, 16.0), "stacked objectives hang in the top-right margin");
    assert_eq!(as_text(&batch.commands[0]).2, [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(as_text(&batch.commands[1]).1, (700.0, 90.0), "an explicit objective position wins");
    assert_eq!(as_text(&batch.commands[1]).2, [0.35, 0.9, 0.35, 1.0], "completed objectives read green");
}

/// The compass is an outline and an "N" label anchored bottom-centre, so its
/// default offset docks it above the bottom edge instead of dropping it
/// off-screen.
#[test]
fn the_compass_docks_above_the_bottom_edge() {
    let mut hud = HUD::new(1920, 1080);
    hud.set_compass(Default::default());

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);

    assert_eq!(batch.commands.len(), 2, "outline + label");
    match &batch.commands[0] {
        UiCommand::RectangleOutline { x, y, width, height, thickness, .. } => {
            assert_eq!((*x, *y, *width, *height, *thickness), (960.0, 980.0, 200.0, 40.0, 2.0));
        }
        other => panic!("expected an outline, got {other:?}"),
    }
    match &batch.commands[1] {
        UiCommand::Text { content, x, y, size, .. } => {
            assert_eq!(content, "N");
            // The compass spans 960..1160 horizontally, so its centre — and
            // the label's left edge — sit at 1060 - 7.
            assert_eq!((*x, *y, *size), (1053.0, 993.0, 14.0));
        }
        other => panic!("expected the north label, got {other:?}"),
    }
}

/// Damage indicators draw a line along their direction, faded by intensity; a
/// zero-length indicator draws nothing rather than degenerate geometry.
#[test]
fn damage_indicators_draw_lines_and_skip_zero_length() {
    let mut hud = HUD::new(1920, 1080);
    hud.add_damage_indicator(HUDDamageIndicator::new(
        Vec2::new(500.0, 500.0),
        Vec2::new(1.0, 0.0),
        1.0,
    ));
    hud.add_damage_indicator(HUDDamageIndicator::new(Vec2::new(600.0, 600.0), Vec2::ZERO, 1.0));

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);

    assert_eq!(batch.commands.len(), 1, "the zero-length indicator must not compile");
    assert_eq!(
        batch.commands[0],
        UiCommand::Line {
            x1: 500.0,
            y1: 500.0,
            x2: 540.0, // one fix reach along +x
            y2: 500.0,
            color: [1.0, 0.0, 0.0, 0.8],
            thickness: 3.0,
        }
    );
}

/// The pause menu compiles its documented layout exactly: background fill,
/// title at the top-left padding, items stacked below the header one pitch
/// apart, the selected one in the selected colour.
#[test]
fn the_pause_menu_compiles_its_documented_geometry() {
    let mut menu = Menu::new(MenuType::Pause);
    menu.position = Vec2::new(10.0, 10.0);

    let mut batch = UiBatch::default();
    menu.to_commands(&mut batch);

    // [0] background, [1] title, then 4 items x (rect + text).
    assert_eq!(batch.commands.len(), 10);
    assert_eq!(
        batch.commands[0],
        UiCommand::Rectangle {
            x: 10.0,
            y: 10.0,
            width: 400.0,
            height: 600.0,
            color: [0.1, 0.1, 0.1, 0.9],
            corner_radius: 0.0,
        }
    );
    assert_eq!(
        batch.commands[1],
        UiCommand::Text {
            content: "Pause".to_string(),
            x: 34.0,
            y: 34.0,
            size: 24.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: None,
        }
    );
    // The selected item (index 0) picks the selected colour.
    assert_eq!(
        batch.commands[2],
        UiCommand::Rectangle {
            x: 34.0,             // 10 + 24 pad
            y: 74.0,             // 10 + 64 header
            width: 200.0,
            height: 40.0,
            color: [1.0, 1.0, 0.5, 1.0],
            corner_radius: 0.0,
        }
    );
    assert_eq!(
        batch.commands[3],
        UiCommand::Text {
            content: "Resume".to_string(),
            x: 42.0,
            y: 86.0, // vertically centred: 74 + (40 - 16) / 2
            size: 16.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: None,
        }
    );
    // Third row down keeps the normal colour while unselected.
    assert_eq!(
        batch.commands[6],
        UiCommand::Rectangle {
            x: 34.0,
            y: 170.0, // 74 + 2 * 48
            width: 200.0,
            height: 40.0,
            color: [0.8, 0.8, 0.8, 1.0],
            corner_radius: 0.0,
        },
        "unselected items keep the normal colour"
    );
}

/// One menu carrying every item type, so each per-type compilation is
/// asserted across one compiled batch: checkbox (outline + fill when
/// checked), slider (bar + label), and text input (outline + whatever it
/// displays).
#[test]
fn every_menu_item_type_compiles_its_documented_shape() {
    let mut menu = Menu::new(MenuType::Settings);
    menu.position = Vec2::new(0.0, 0.0);
    menu.items.clear();
    menu.selected_index = 99; // nothing selected: every item stays in its normal colour

    menu.items.push(MenuItem {
        text: "Option".to_string(),
        position: Vec2::ZERO, // stacked
        checked: true,
        item_type: MenuItemType::Checkbox,
        ..MenuItem::new("opt", MenuItemType::Checkbox)
    });
    menu.items.push(MenuItem {
        text: "Volume".to_string(),
        position: Vec2::new(300.0, 100.0), // explicit position wins over the stack
        value: 0.25,
        min_value: 0.0,
        max_value: 1.0,
        item_type: MenuItemType::Slider,
        ..MenuItem::new("vol", MenuItemType::Slider)
    });
    menu.items.push(MenuItem {
        text: "settings".to_string(),
        position: Vec2::ZERO,
        input_text: String::new(), // nothing typed yet: the prompt shows
        item_type: MenuItemType::TextInput,
        ..MenuItem::new("input", MenuItemType::TextInput)
    });

    let mut batch = UiBatch::default();
    menu.to_commands(&mut batch);

    // [2] checkbox outline, [3] checked fill — the checkbox is row 0.
    let box_y = 64.0 + (40.0 - 20.0) * 0.5; // vertically centred in the 40px row
    match &batch.commands[2] {
        UiCommand::RectangleOutline { x, y, width, height, thickness, .. } => {
            assert_eq!((*x, *y, *width, *height, *thickness), (24.0, box_y, 20.0, 20.0, 2.0));
        }
        other => panic!("expected the checkbox square, got {other:?}"),
    }
    match &batch.commands[3] {
        UiCommand::Rectangle { x, y, width, height, .. } => {
            assert_eq!((*x, *y, *width, *height), (28.0, box_y + 4.0, 12.0, 12.0));
        }
        other => panic!("expected the checked fill, got {other:?}"),
    }
    match &batch.commands[5] {
        UiCommand::ProgressBar { x, y, progress, width, height, .. } => {
            assert_eq!((*x, *y, *progress, *width, *height), (300.0, 100.0, 0.25, 200.0, 40.0));
        }
        other => panic!("expected the slider bar, got {other:?}"),
    }
    assert!(
        batch
            .commands
            .iter()
            .any(|c| matches!(c, UiCommand::Text { content, .. } if content == "settings")),
        "the text input falls back to its own text until something is typed"
    );
}

/// A hidden menu compiles nothing.
#[test]
fn a_hidden_menu_compiles_nothing() {
    let mut menu = Menu::new(MenuType::Main);
    menu.visible = false;
    let mut batch = UiBatch::default();
    menu.to_commands(&mut batch);
    assert!(batch.commands.is_empty(), "{:?}", batch.commands);
}

/// Minimap elements compile centred on their mapped world position, in stable
/// `z_index` order, invisible ones compile nothing. The centre-based mapping
/// is the one Bug №188 fixed, and this test quotes it.
#[test]
fn minimap_elements_compile_at_their_mapped_positions() {
    let mut minimap = Minimap::new(); // (10,10) 200x200, world 1000x1000, centre (0,0,0)
    minimap.elements.push(
        MinimapElement::new("late", MinimapElementType::Ship)
            .with_world_position(glam::Vec3::new(250.0, 0.0, 0.0))
            .with_z_index(1),
    );
    minimap.elements.push(
        MinimapElement::new("early", MinimapElementType::Marker)
            .with_world_position(glam::Vec3::ZERO)
            .with_z_index(0),
    );
    minimap.elements.push(
        MinimapElement::new("hidden", MinimapElementType::Marker)
            .with_world_position(glam::Vec3::new(500.0, 0.0, 0.0))
            .with_visible(false),
    );

    let mut batch = UiBatch::default();
    minimap.to_commands(&mut batch);

    let rect = |command: &UiCommand| match command {
        UiCommand::Rectangle { x, y, .. } => (*x, *y),
        other => panic!("expected a rectangle, got {other:?}"),
    };
    assert_eq!(batch.commands.len(), 2);
    // World (0,0,0) maps to the minimap centre (110,110); an element is
    // centred there, so its 10px rect starts half its size back.
    assert_eq!(
        rect(&batch.commands[0]),
        (105.0, 105.0),
        "the centre of the world is the centre of the map"
    );
    // World x = 250 of 1000 → +0.25 * 200 * zoom(1.0) = +50px → 160; centred.
    assert_eq!(
        rect(&batch.commands[1]),
        (155.0, 105.0),
        "elements sit where the forward mapping puts them"
    );
}

/// The gates survive the model: `compile_into` with `hud: false` drops every
/// HUD command and keeps the rest. This is the model side of the pass's
/// `enable_*` flags — the pass side is the GPU test below.
#[test]
fn gates_decide_which_parts_compile() {
    let mut hud = HUD::new(640, 480);
    hud.add_element(
        HUDElement::new("rect")
            .with_position(Vec2::new(50.0, 50.0))
            .with_alignment(HUDAlignment::TopLeft),
    );
    let mut menu = Menu::new(MenuType::Pause);
    menu.position = Vec2::new(0.0, 0.0);
    menu.items.truncate(1);
    let model = UiModel {
        hud,
        menu: Some(menu),
        minimap: Some(Minimap::new()),
    };

    let mut all = UiBatch::default();
    model.compile_into(&mut all, UiGates::default());
    let mut hudless = UiBatch::default();
    model.compile_into(
        &mut hudless,
        UiGates { hud: false, menu: true, minimap: true },
    );

    let hud_command = UiCommand::Rectangle {
        x: 50.0,
        y: 50.0,
        width: 1.0,
        height: 1.0,
        color: [1.0, 1.0, 1.0, 1.0],
        corner_radius: 0.0,
    };
    // HUD: 1. Menu: background + title + one item's rect and text: 4.
    assert_eq!(all.commands.len(), 5);
    assert_eq!(hudless.commands.len(), 4);
    assert!(all.commands.iter().any(|c| c == &hud_command));
    assert!(
        !hudless.commands.iter().any(|c| c == &hud_command),
        "hud: false must drop every HUD command, got {hudless:?}"
    );
}

// ---------------------------------------------------------------------------
// GPU: the model, the pass, and the pixels.
// ---------------------------------------------------------------------------

const EXTENT: u32 = 256;
const GEOMETRY_SPIRV: &[u8] = include_bytes!("../assets/shaders/ui_geom.spv");
const TEXTURE_SPIRV: &[u8] = include_bytes!("../assets/shaders/ui_tex.spv");
/// The full-colour image shader the bridge loads after `initialize`.
const IMAGE_SPIRV: &[u8] = include_bytes!("../assets/shaders/ui.spv");

fn open_device() -> Arc<rhi::Device> {
    let backend = rhi::backend::create_backend(&rhi::RhiConfig {
        validation: true,
        ..rhi::RhiConfig::default()
    })
    .expect("backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    assert!(!devices.is_empty(), "no Vulkan device available");
    Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc { features: Features::default(), ..Default::default() },
            )
            .expect("create device"),
    )
}

/// A texture the model can name: a 4x4 pure-red image, uploaded through the
/// same rhi texture the model's batches reference.
fn red_media(device: &Arc<rhi::Device>) -> Arc<Texture> {
    let texture = Texture::new_2d(
        "probe",
        TextureFormat::RGBA8,
        4,
        4,
        RenderTextureUsage::Sampled,
        device,
    )
    .expect("probe texture");
    let mut red = vec![0u8; 4 * 4 * 4];
    for px in red.chunks_exact_mut(4) {
        px.copy_from_slice(&[255, 0, 0, 255]);
    }
    device
        .upload_texture(texture.rhi_texture(), &red)
        .expect("upload the probe colour");
    Arc::new(texture)
}

/// A headless context staged exactly as the presentation-driven pass expects:
/// format, extent, and the typed view of the image that was "acquired".
fn staged_context(device: &Arc<rhi::Device>) -> (RenderContext, Arc<rhi::Texture>) {
    let target = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the staging target");
    let view = device
        .create_texture_view(&target, target.create_default_view().desc())
        .expect("typed view");

    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    context.presentation_format = Some(Format::RGBA8_UNORM);
    context.presentation_extent = Extent2D::new(EXTENT, EXTENT);
    context.set_presentation_texture_views(vec![view]);
    context.swapchain_image_index = 0;
    (context, Arc::new(target))
}

/// A model with exactly one HUD command: a texture-less crosshair quad.
fn crosshair_model() -> UiModel {
    let mut hud = HUD::new(EXTENT, EXTENT);
    hud.set_crosshair(
        HUDElement::new("crosshair")
            .with_position(Vec2::new(96.0, 120.0))
            .with_size(Vec2::new(64.0, 4.0))
            .with_alignment(HUDAlignment::TopLeft),
    );
    UiModel { hud, menu: None, minimap: None }
}

/// A model carrying one texture-backed element at the pixel-test geometry.
fn probe_model(media: &Arc<Texture>) -> UiModel {
    let mut hud = HUD::new(EXTENT, EXTENT);
    hud.add_element(
        HUDElement::new("probe")
            .with_position(Vec2::new(32.0, 32.0))
            .with_size(Vec2::new(160.0, 90.0))
            .with_alignment(HUDAlignment::TopLeft)
            .with_texture(Arc::clone(media)),
    );
    UiModel { hud, menu: None, minimap: None }
}

/// One `UIPass` frame against the staged context: the pass's own `execute`,
/// through the trait, so the test cannot end up calling a wrapper the
/// graph never runs.
fn one_frame(pass: &mut UIPass, context: &mut RenderContext) -> (PassOutcome, Vec<Command>) {
    let pool = CommandPool::new(CommandPoolDesc::default());
    let mut buffer = CommandBuffer::new(pool, CommandBufferDesc::default());
    buffer.begin().expect("begin");
    let mut encoder = CommandEncoder::new(buffer);
    let mut scene = Scene::new("ui");
    let resources = HashMap::new();
    let outcome = pass.execute(&mut encoder, context, &mut scene, &resources);
    (outcome, encoder.commands().to_vec())
}

fn begins_draws_ends(commands: &[Command]) -> (usize, usize, usize) {
    let begins = commands
        .iter()
        .position(|c| matches!(c, Command::BeginRenderPass(_)))
        .expect("no BeginRenderPass recorded");
    let draws = commands
        .iter()
        .position(|c| matches!(c, Command::DrawIndexed { .. }))
        .expect("no DrawIndexed recorded");
    let ends = commands
        .iter()
        .position(|c| matches!(c, Command::EndRenderPass))
        .expect("no EndRenderPass recorded");
    (begins, draws, ends)
}

/// A texture-backed element compiles the image command and names the same
/// handle twice; the renderer keys its cache by that handle, so both draws
/// must carry one id.
#[test]
fn two_elements_sharing_a_texture_name_one_id() {
    let device = open_device();
    let media = red_media(&device);

    let mut hud = HUD::new(640, 480);
    hud.add_element(
        HUDElement::new("first")
            .with_position(Vec2::new(10.0, 10.0))
            .with_size(Vec2::new(60.0, 30.0))
            .with_texture(Arc::clone(&media)),
    );
    hud.add_element(
        HUDElement::new("second")
            .with_position(Vec2::new(10.0, 50.0))
            .with_size(Vec2::new(60.0, 30.0))
            .with_texture(media),
    );

    let mut batch = UiBatch::default();
    hud.to_commands(&mut batch);

    assert_eq!(batch.commands.len(), 2);
    assert_eq!(batch.textures.len(), 2, "one registration per compile site");
    assert_eq!(
        batch.textures[0].0, batch.textures[1].0,
        "the same Arc must carry one id"
    );
    assert!(
        Arc::ptr_eq(&batch.textures[0].1, &batch.textures[1].1),
        "the registered handles must be the texture's own rhi texture"
    );
    for command in &batch.commands {
        match command {
            UiCommand::Image { texture_id, width, height, .. } => {
                assert_eq!(*texture_id, batch.textures[0].0);
                assert_eq!((*width, *height), (60.0, 30.0));
            }
            other => panic!("expected an Image command, got {other:?}"),
        }
    }
}

/// The pass compiles the model under the lock, registers its textures, and
/// submits exactly what it compiled — then hands the renderer one draw per
/// element inside a balanced render pass. The stats are quoted, not
/// "non-zero": one HUD command (the crosshair; the probe element) is exactly
/// two in this model.
#[test]
fn the_pass_compiles_and_records_the_model_it_is_given() {
    let device = open_device();
    let (mut context, _target) = staged_context(&device);

    let media = red_media(&device);
    let mut model = crosshair_model();
    model.hud.add_element(
        HUDElement::new("probe")
            .with_position(Vec2::new(32.0, 32.0))
            .with_size(Vec2::new(160.0, 90.0))
            .with_alignment(HUDAlignment::TopLeft)
            .with_texture(Arc::clone(&media)),
    );

    let config = UIConfig { debug_marker: false, ..Default::default() };
    let mut pass = UIPass::new(config).with_model(Arc::new(Mutex::new(model)));

    let (outcome, commands) = one_frame(&mut pass, &mut context);

    assert!(outcome.recorded, "the pass skipped, reason {:?} — the model must record", outcome.reason);
    let stats = pass.stats();
    assert_eq!(
        stats.commands_processed, 2,
        "the crosshair quad and the probe image are exactly two commands"
    );
    assert_eq!(
        stats.skipped, 0,
        "a registered texture and an on-gate HUD must not report skips: {:?}",
        stats.skipped
    );

    let (begin, draw, end) = begins_draws_ends(&commands);
    assert!(begin < draw, "the draw was issued before the pass began");
    assert!(draw < end, "the draw was issued after the pass ended");
}

/// The gate is the pass's decision, not the model's: with `enable_hud = false`
/// a populated HUD must compile nothing, reach the renderer as no commands,
/// and record no draw. This is the negative control behind the gate — break
/// the pass's `UiGates` construction and this goes red on
/// `commands_processed == 0`.
#[test]
fn the_hud_gate_drops_the_model_from_the_frame() {
    let device = open_device();
    let (mut context, _target) = staged_context(&device);

    let config = UIConfig {
        enable_hud: false,
        debug_marker: false,
        ..Default::default()
    };
    let mut pass = UIPass::new(config).with_model(Arc::new(Mutex::new(crosshair_model())));

    let (outcome, commands) = one_frame(&mut pass, &mut context);

    assert!(
        !outcome.recorded,
        "a gated-off HUD still recorded a draw, reason {:?}",
        outcome.reason
    );
    assert_eq!(
        outcome.reason.as_deref(),
        Some("nothing to draw"),
        "the skip must name the empty queue, not another exit"
    );
    assert_eq!(
        pass.stats().commands_processed, 0,
        "a gated-off HUD must not put commands in the queue"
    );
    assert!(
        !commands.iter().any(|c| matches!(c, Command::DrawIndexed { .. })),
        "a gated-off HUD reached the command list as a draw"
    );
}

/// The compiled model draws its pixels: the probe element is a 4x4 pure-red
/// texture compiled into a 160x90 image command, registered by the compile
/// path, and rasterised over an offscreen target. The red pixel count must be
/// exactly the element's pixel area — "some red" has been true of a frame
/// that drew the wrong thing.
///
/// This runs the compile output through an offscreen renderer rather than the
/// pass itself, because the pass builds for presentation, and a
/// presentation-intent render pass ends its frame in a layout pixels cannot
/// be read back from; that route is covered by the two pass-level tests
/// above.
#[test]
fn a_compiled_element_reaches_the_pixels_it_names() {
    let device = open_device();
    let media = red_media(&device);
    let model = probe_model(&media);

    // Compile exactly as the pass does, and hand the renderer exactly what
    // the pass would hand it.
    let batch = model.compile();
    assert_eq!(batch.commands.len(), 1, "one element, one image command");

    let format = Format::RGBA8_UNORM;
    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            format,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the readback target");
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("target view");

    let mut ui = rfs_client::rhi::ui::UiRenderer::new(
        Arc::clone(&device),
        rfs_client::rhi::ui::UiRendererConfig {
            initial_vertex_capacity: 256,
            initial_index_capacity: 512,
            debug_label: false,
        },
    );
    ui.initialize_offscreen(format, EXTENT, EXTENT, GEOMETRY_SPIRV, Some(TEXTURE_SPIRV))
        .expect("ui initialize");
    ui.load_image_shader(IMAGE_SPIRV).expect("image shader");
    for (id, rhi_texture) in &batch.textures {
        ui.add_texture(*id, Arc::clone(rhi_texture));
    }
    ui.submit_commands(batch.commands).expect("submit the compiled batch");

    let framebuffer = device
        .create_gpu_framebuffer(&rfs_client::rhi::FramebufferDesc {
            render_pass: ui.render_pass().expect("the UI must expose its render pass"),
            attachments: vec![rfs_client::rhi::FramebufferAttachment {
                texture_view: view,
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    let pool = CommandPool::new(CommandPoolDesc::default());
    let mut buffer = CommandBuffer::new(pool, CommandBufferDesc::default());
    buffer.begin().expect("begin");
    let mut encoder = CommandEncoder::new(buffer);
    assert!(
        ui.record_into_framebuffer(&mut encoder, &framebuffer, EXTENT, EXTENT)
            .expect("record"),
        "the compiled model reported nothing to draw"
    );
    device.submit_recorded(encoder.commands()).expect("submit");
    device.wait_idle().expect("wait idle");

    let bytes = device.download_texture(&texture, 0, 0).expect("readback");
    assert_eq!(
        bytes.len(),
        (EXTENT * EXTENT * 4) as usize,
        "the readback is not one RGBA pixel per texel"
    );

    let mut red = 0u32;
    let mut lit = 0u32;
    for px in bytes.chunks_exact(4) {
        if px.iter().any(|c| *c != 0) {
            lit += 1;
        }
        if px == [255, 0, 0, 255] {
            red += 1;
        }
    }
    let area = 160u32 * 90;
    assert_eq!(
        lit, area,
        "expected exactly the {area} element pixels to be lit, got {lit}"
    );
    assert_eq!(
        red, area,
        "expected every element pixel to read exactly [255,0,0,255], got {red}"
    );

    let center = ((32 + 80) as usize * (EXTENT as usize) + 32 + 80) * 4;
    assert_eq!(
        [bytes[center], bytes[center + 1], bytes[center + 2], bytes[center + 3]],
        [255u8, 0, 0, 255],
        "the centre of the element is not the uploaded red"
    );
}
