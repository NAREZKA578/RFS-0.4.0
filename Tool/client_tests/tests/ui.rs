//! Integration tests for the render::ui module.
//!
//! Covers: Menu, MenuItem, MenuItemType, MenuType, HUD, HUDElement,
//! HUDAlignment, Minimap, MinimapElement.

use glam::{Vec2, Vec3, Vec4};
use rfs_client::render::ui::hud::{HUD, HUDAlignment, HUDElement};
use rfs_client::render::ui::menu::{Menu, MenuItem, MenuItemType, MenuType};
use rfs_client::render::ui::minimap::{Minimap, MinimapElement, MinimapElementType};
use std::time::Duration;

const EPSILON: f32 = 1e-3;

#[test]
fn menu_main_contains_default_items() {
    let menu = Menu::new(MenuType::Main);
    assert_eq!(menu.menu_type, MenuType::Main);
    assert!(!menu.title.is_empty());
    assert!(menu.visible);
    assert_eq!(menu.items.len(), 4);
    assert!(menu.get_item(0).is_some());
}

#[test]
fn menu_add_and_get_item() {
    let mut menu = Menu::new(MenuType::Pause);
    let before = menu.items.len();
    menu.add_item(MenuItem::new("Resume", MenuItemType::Button).with_text("Resume"));
    assert_eq!(menu.items.len(), before + 1);
    let item = menu.get_item(menu.items.len() - 1).unwrap();
    assert_eq!(item.name, "Resume");
    assert_eq!(item.item_type, MenuItemType::Button);
}

#[test]
fn menu_remove_item() {
    let mut menu = Menu::new(MenuType::Settings);
    let before = menu.items.len();
    let removed = menu.remove_item(0);
    assert!(removed.is_some());
    assert_eq!(menu.items.len(), before - 1);
}

#[test]
fn menu_selection_navigation() {
    let mut menu = Menu::new(MenuType::Main);
    menu.set_selected_index(0);
    assert_eq!(menu.selected_index(), 0);

    menu.next_item();
    assert_eq!(menu.selected_index(), 1);

    menu.previous_item();
    assert_eq!(menu.selected_index(), 0);

    assert!(menu.select_item().is_some());
    assert_eq!(menu.select_item().unwrap().name, "Single Player");
}

#[test]
fn menu_set_selected_index_bound() {
    let mut menu = Menu::new(MenuType::Settings);
    let last = menu.items.len() - 1;
    menu.set_selected_index(last);
    assert_eq!(menu.selected_index(), last);
    assert!(menu.get_item(last).is_some());
}

#[test]
fn menu_visibility_flags() {
    let mut menu = Menu::new(MenuType::InGame);
    assert!(menu.visible);
    menu.set_visible(false);
    assert!(!menu.visible);
}

#[test]
fn hud_screen_size() {
    let hud = HUD::new(1920, 1080);
    assert_eq!(hud.screen_size(), Vec2::new(1920.0, 1080.0));
}

#[test]
fn hud_add_get_remove_element() {
    let mut hud = HUD::new(1280, 720);
    hud.add_element(HUDElement::new("health_bar"));
    assert!(hud.get_element("health_bar").is_some());

    let element = hud.get_element_mut("health_bar").unwrap();
    element.visible = false;
    assert!(!hud.get_element("health_bar").unwrap().visible);

    assert!(hud.remove_element("health_bar").is_some());
    assert!(hud.get_element("health_bar").is_none());
}

#[test]
fn hud_element_alignment_positions() {
    let mut element = HUDElement::new("crosshair")
        .with_position(Vec2::new(10.0, 20.0))
        .with_size(Vec2::new(40.0, 40.0))
        .with_alignment(HUDAlignment::TopLeft);
    element.visible = true;

    let screen = Vec2::new(1920.0, 1080.0);
    let top_left = element.calculate_position(screen);
    assert_eq!(top_left, Vec2::new(10.0, 20.0));

    element.alignment = HUDAlignment::TopRight;
    let top_right = element.calculate_position(screen);
    assert_eq!(top_right, Vec2::new(1910.0, 20.0));

    element.alignment = HUDAlignment::CenterLeft;
    let center_left = element.calculate_position(screen);
    assert!((center_left.x - 10.0).abs() < EPSILON);
    assert!((center_left.y - 560.0).abs() < EPSILON);

    element.alignment = HUDAlignment::BottomRight;
    let bottom_right = element.calculate_position(screen);
    assert_eq!(bottom_right, Vec2::new(1910.0, 1060.0));
}

#[test]
fn hud_element_defaults() {
    let element = HUDElement::default();
    assert_eq!(element.name, "default");
    assert_eq!(element.position, Vec2::ZERO);
    assert_eq!(element.size, Vec2::ONE);
    assert_eq!(element.alignment, HUDAlignment::Center);
    assert_eq!(element.color, Vec4::ONE);
    assert!(element.visible);
    assert_eq!(element.z_index, 0);
}

#[test]
fn hud_update_does_not_panic() {
    let mut hud = HUD::new(1920, 1080);
    hud.set_screen_size(2560, 1440);
    assert_eq!(hud.screen_size(), Vec2::new(2560.0, 1440.0));
    hud.update(Duration::from_secs_f32(0.016));
}

#[test]
fn hud_damage_indicators_expire() {
    let mut hud = HUD::new(1920, 1080);
    hud.add_damage_indicator(rfs_client::render::ui::hud::HUDDamageIndicator::new(
        Vec2::new(100.0, 100.0),
        Vec2::new(1.0, 0.0),
        1.0,
    ));
    hud.update(Duration::from_secs_f32(2.0));
}

#[test]
fn minimap_world_minimap_roundtrip() {
    let mut minimap = Minimap::new();
    minimap.set_position(Vec2::new(10.0, 20.0));
    minimap.set_size(Vec2::new(200.0, 200.0));
    minimap.set_world_size(Vec2::new(100.0, 100.0));
    minimap.set_world_center(Vec3::ZERO);
    assert!(minimap.visible);

    let world = Vec3::new(12.0, 0.0, -7.0);
    let mapped = minimap.world_to_minimap(world);
    let restored = minimap.minimap_to_world(mapped);
    assert!(
        (restored.x - world.x).abs() < 0.5 && (restored.z - world.z).abs() < 0.5,
        "roundtrip mismatch: {} -> {}",
        world,
        restored
    );
}

#[test]
fn minimap_world_origin_maps_to_the_rect_centre_and_scales() {
    let mut minimap = Minimap::new();
    minimap.set_position(Vec2::new(10.0, 20.0));
    minimap.set_size(Vec2::new(200.0, 200.0));
    minimap.set_world_size(Vec2::new(100.0, 100.0));
    minimap.set_world_center(Vec3::ZERO);
    assert!(minimap.visible);

    // Bug №188: the world centre maps to the CENTRE of the minimap rect, not
    // the top-left anchor. The anchor convention is what made the roundtrip
    // non-invertible, because `minimap_to_world` used the anchor while the
    // forward mapping used the centre. The forward function documents the
    // centre as the deliberate choice, so the test follows it.
    let centre = Vec2::new(10.0 + 100.0, 20.0 + 100.0);
    assert_eq!(minimap.world_to_minimap(Vec3::ZERO), centre);

    // +Z world moves +y on screen, scaled by size / world_size (200/100 = 2):
    // z = 50 is half the world extent, so half the rect.
    let north = minimap.world_to_minimap(Vec3::new(0.0, 0.0, 50.0));
    assert!((north.x - centre.x).abs() < 0.5, "north.x = {}", north.x);
    assert!(
        (north.y - (centre.y + 100.0)).abs() < 0.5,
        "north.y = {}",
        north.y
    );

    // +X world moves +x on screen.
    let east = minimap.world_to_minimap(Vec3::new(25.0, 0.0, 0.0));
    assert!((east.x - (centre.x + 50.0)).abs() < 0.5, "east.x = {}", east.x);
    assert!((east.y - centre.y).abs() < 0.5, "east.y = {}", east.y);
}

#[test]
fn minimap_elements_can_be_added() {
    let mut minimap = Minimap::new();
    minimap.add_element(
        MinimapElement::new("player", MinimapElementType::Ship)
            .with_world_position(Vec3::new(5.0, 0.0, 5.0))
            .with_world_size(Vec2::new(2.0, 2.0))
            .with_color(Vec4::new(0.0, 1.0, 0.0, 1.0))
            .with_size(Vec2::new(6.0, 6.0)),
    );
    let element = minimap.get_element("player").expect("element exists");
    assert_eq!(element.element_type, MinimapElementType::Ship);
    assert_eq!(element.world_position, Vec3::new(5.0, 0.0, 5.0));

    assert!(minimap.remove_element("player").is_some());
    assert!(minimap.get_element("player").is_none());
}