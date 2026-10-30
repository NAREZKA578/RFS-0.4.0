//! HUD (Head-Up Display)
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::rhi::ui::UiCommand;
use crate::render::meshes::Mesh;
use crate::render::textures::Texture;
use glam::{Vec2, Vec4};
use std::sync::Arc;

use super::model::{rgba, UiBatch};

/// HUD element alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum HUDAlignment {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    #[default]
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}


/// HUD element
#[derive(Debug, Clone)]
pub struct HUDElement {
    pub name: String,
    pub position: Vec2,
    pub size: Vec2,
    pub alignment: HUDAlignment,
    pub color: Vec4,
    pub texture: Option<Arc<Texture>>,
    pub mesh: Option<Arc<Mesh>>,
    pub visible: bool,
    pub z_index: i32,
}

impl HUDElement {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            position: Vec2::ZERO,
            size: Vec2::ONE,
            alignment: HUDAlignment::Center,
            color: Vec4::ONE,
            texture: None,
            mesh: None,
            visible: true,
            z_index: 0,
        }
    }

    pub fn with_position(mut self, position: Vec2) -> Self {
        self.position = position;
        self
    }

    pub fn with_size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    pub fn with_alignment(mut self, alignment: HUDAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn with_color(mut self, color: Vec4) -> Self {
        self.color = color;
        self
    }

    pub fn with_texture(mut self, texture: Arc<Texture>) -> Self {
        self.texture = Some(texture);
        self
    }

    pub fn with_mesh(mut self, mesh: Arc<Mesh>) -> Self {
        self.mesh = Some(mesh);
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_z_index(mut self, z_index: i32) -> Self {
        self.z_index = z_index;
        self
    }

    /// Calculate final position based on alignment
    pub fn calculate_position(&self, screen_size: Vec2) -> Vec2 {
        match self.alignment {
            HUDAlignment::TopLeft => self.position,
            HUDAlignment::TopCenter => {
                Vec2::new(screen_size.x / 2.0 + self.position.x, self.position.y)
            }
            HUDAlignment::TopRight => Vec2::new(screen_size.x - self.position.x, self.position.y),
            HUDAlignment::CenterLeft => {
                Vec2::new(self.position.x, screen_size.y / 2.0 + self.position.y)
            }
            HUDAlignment::Center => Vec2::new(
                screen_size.x / 2.0 + self.position.x,
                screen_size.y / 2.0 + self.position.y,
            ),
            HUDAlignment::CenterRight => Vec2::new(
                screen_size.x - self.position.x,
                screen_size.y / 2.0 + self.position.y,
            ),
            HUDAlignment::BottomLeft => Vec2::new(self.position.x, screen_size.y - self.position.y),
            HUDAlignment::BottomCenter => Vec2::new(
                screen_size.x / 2.0 + self.position.x,
                screen_size.y - self.position.y,
            ),
            HUDAlignment::BottomRight => Vec2::new(
                screen_size.x - self.position.x,
                screen_size.y - self.position.y,
            ),
        }
    }
}

impl Default for HUDElement {
    fn default() -> Self {
        Self::new("default")
    }
}

/// HUD (Head-Up Display)
pub struct HUD {
    /// All HUD elements
    elements: Vec<HUDElement>,
    /// Screen size
    screen_size: Vec2,
    /// Crosshair
    crosshair: Option<HUDElement>,
    /// Ship information
    ship_info: ShipHUDInfo,
    /// Weapon information
    weapon_info: WeaponHUDInfo,
    /// Player information
    player_info: PlayerHUDInfo,
    /// Objectives
    objectives: Vec<HUDObjective>,
    /// Compass
    compass: Option<HUDCompass>,
    /// Damage indicators
    damage_indicators: Vec<HUDDamageIndicator>,
}

impl HUD {
    pub fn new(screen_width: u32, screen_height: u32) -> Self {
        Self {
            elements: Vec::new(),
            screen_size: Vec2::new(screen_width as f32, screen_height as f32),
            crosshair: None,
            ship_info: ShipHUDInfo::default(),
            weapon_info: WeaponHUDInfo::default(),
            player_info: PlayerHUDInfo::default(),
            objectives: Vec::new(),
            compass: None,
            damage_indicators: Vec::new(),
        }
    }

    pub fn screen_size(&self) -> Vec2 {
        self.screen_size
    }

    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_size = Vec2::new(width as f32, height as f32);
    }

    pub fn add_element(&mut self, element: HUDElement) {
        self.elements.push(element);
    }

    pub fn remove_element(&mut self, name: &str) -> Option<HUDElement> {
        let index = self.elements.iter().position(|e| e.name == name);
        if let Some(index) = index {
            Some(self.elements.remove(index))
        } else {
            None
        }
    }

    pub fn get_element(&self, name: &str) -> Option<&HUDElement> {
        self.elements.iter().find(|e| e.name == name)
    }

    pub fn get_element_mut(&mut self, name: &str) -> Option<&mut HUDElement> {
        self.elements.iter_mut().find(|e| e.name == name)
    }

    pub fn set_crosshair(&mut self, element: HUDElement) {
        self.crosshair = Some(element);
    }

    pub fn set_ship_info(&mut self, info: ShipHUDInfo) {
        self.ship_info = info;
    }

    pub fn set_weapon_info(&mut self, info: WeaponHUDInfo) {
        self.weapon_info = info;
    }

    pub fn set_player_info(&mut self, info: PlayerHUDInfo) {
        self.player_info = info;
    }

    pub fn add_objective(&mut self, objective: HUDObjective) {
        self.objectives.push(objective);
    }

    pub fn set_compass(&mut self, compass: HUDCompass) {
        self.compass = Some(compass);
    }

    pub fn add_damage_indicator(&mut self, indicator: HUDDamageIndicator) {
        self.damage_indicators.push(indicator);
    }

    pub fn update(&mut self, delta_time: std::time::Duration) {
        // Update all elements
        for _element in &mut self.elements {
            // Update animations, etc.
        }

        // Update damage indicators
        for indicator in &mut self.damage_indicators {
            indicator.update(delta_time);
        }

        // Remove expired damage indicators
        self.damage_indicators.retain(|i| !i.expired());
    }

    /// Compiles the HUD into `UiCommand`s for the frame, replacing the stub
    /// `render` this once replaced.
    ///
    /// ** Bug №724.** `render` and its seven `render_*` helpers were empty
    /// bodies with zero callers: every part of the HUD the data model can
    /// express now lands in the command stream instead, so the pass draws
    /// exactly what `to_commands` compiled and nothing is drawn from a
    /// parallel path.
    ///
    /// Draw order: elements (stable `z_index` ascending), then the crosshair,
    /// then the info panels. Panels are drawn only when they carry data (a
    /// non-empty name), so a default `HUD` — no elements, default infos,
    /// no compass — compiles nothing at all, and an empty screen is an
    /// assertable fact rather than a coincidence of layout.
    ///
    /// v1 scope, on the record:
    ///  - an element with a texture is an `Image`; one without (its `mesh` is
    ///    ignored, mesh HUDs need their own pipeline) is a flat `Rectangle`;
    ///  - an info panel is one background rectangle and fixed-pitch rows —
    ///    a label, and where there is a quantity a `ProgressBar` — so every
    ///    compiled coordinate is recomputable in a test;
    ///  - the compass is an outline and an "N" label; its markers are v2;
    ///  - a damage indicator is a line from its position along its direction,
    ///    faded by intensity; zero-length indicators draw nothing.
    pub fn to_commands(&self, batch: &mut UiBatch) {
        let screen = self.screen_size;

        let mut order: Vec<&HUDElement> = self.elements.iter().filter(|e| e.visible).collect();
        order.sort_by_key(|e| e.z_index);
        for element in &order {
            push_element(batch, element, screen);
        }

        if let Some(crosshair) = &self.crosshair {
            if crosshair.visible {
                push_element(batch, crosshair, screen);
            }
        }

        if !self.ship_info.name.is_empty() {
            let rows = [
                Row::Label(self.ship_info.name.clone()),
                Row::Bar(
                    "HP".to_string(),
                    ratio(self.ship_info.hp, self.ship_info.max_hp),
                ),
                Row::Bar(
                    "SPD".to_string(),
                    ratio(self.ship_info.speed, self.ship_info.max_speed),
                ),
                Row::Bar(
                    "FUEL".to_string(),
                    ratio(self.ship_info.fuel, self.ship_info.max_fuel),
                ),
                Row::Label(format!(
                    "CREW {}/{}",
                    self.ship_info.crew_count, self.ship_info.max_crew
                )),
            ];
            let y = screen.y - PANEL_MARGIN - rows.len() as f32 * ROW_PITCH;
            push_panel(batch, PANEL_MARGIN, y, &rows);
        }

        if !self.weapon_info.name.is_empty() {
            let rows = [
                Row::Label(self.weapon_info.name.clone()),
                Row::Label(format!(
                    "AMMO {}/{}",
                    self.weapon_info.ammo, self.weapon_info.max_ammo
                )),
                Row::Bar(
                    "COOL".to_string(),
                    ratio(self.weapon_info.cooldown, self.weapon_info.max_cooldown),
                ),
            ];
            let y = screen.y - PANEL_MARGIN - rows.len() as f32 * ROW_PITCH;
            push_panel(batch, screen.x - PANEL_MARGIN - PANEL_WIDTH, y, &rows);
        }

        if !self.player_info.name.is_empty() {
            let rows = [
                Row::Label(self.player_info.name.clone()),
                Row::Bar(
                    "HP".to_string(),
                    ratio(self.player_info.health, self.player_info.max_health),
                ),
                Row::Label(format!("ROLE {}", self.player_info.role)),
            ];
            push_panel(batch, PANEL_MARGIN, PANEL_MARGIN, &rows);
        }

        for (index, objective) in self.objectives.iter().enumerate() {
            if !objective.visible {
                continue;
            }
            // An objective set at (0, 0) stacks under the previous one in the
            // top-right corner; an explicit position wins.
            let at = if objective.position == Vec2::ZERO {
                Vec2::new(
                    screen.x - PANEL_MARGIN - PANEL_WIDTH + PANEL_PAD,
                    PANEL_MARGIN + index as f32 * ROW_PITCH,
                )
            } else {
                objective.position
            };
            batch.commands.push(UiCommand::Text {
                content: objective.title.clone(),
                x: at.x,
                y: at.y,
                size: TEXT_SIZE,
                color: if objective.completed {
                    OBJECTIVE_DONE
                } else {
                    OBJECTIVE_OPEN
                },
                font: None,
            });
        }

        if let Some(compass) = &self.compass {
            if compass.visible {
                // The compass carries no anchor of its own, so it uses the
                // bottom-centre one: `position` is an offset from there, and
                // the default (0, -100) docks it just above the bottom edge.
                let x = screen.x * 0.5 + compass.position.x;
                let y = screen.y + compass.position.y;
                batch.commands.push(UiCommand::RectangleOutline {
                    x,
                    y,
                    width: compass.size.x,
                    height: compass.size.y,
                    color: COMPASS_COLOR,
                    thickness: 2.0,
                    corner_radius: 0.0,
                });
                batch.commands.push(UiCommand::Text {
                    content: "N".to_string(),
                    x: x + compass.size.x * 0.5 - TEXT_SIZE * 0.5,
                    y: y + compass.size.y * 0.5 - TEXT_SIZE * 0.5,
                    size: TEXT_SIZE,
                    color: COMPASS_COLOR,
                    font: None,
                });
            }
        }

        for indicator in &self.damage_indicators {
            if indicator.direction.length_squared() > 0.0 {
                let color = [
                    indicator.color.x,
                    indicator.color.y,
                    indicator.color.z,
                    indicator.color.w * indicator.intensity.clamp(0.0, 1.0),
                ];
                batch.commands.push(UiCommand::Line {
                    x1: indicator.position.x,
                    y1: indicator.position.y,
                    x2: indicator.position.x + indicator.direction.x * DAMAGE_REACH,
                    y2: indicator.position.y + indicator.direction.y * DAMAGE_REACH,
                    color,
                    thickness: 3.0,
                });
            }
        }
    }
}

/// Where the drawn panels sit and how far apart their rows are.
///
/// Every one of these is load-bearing: `ui_pass_model.rs` asserts the compiled
/// geometry exactly, so the constants are named here once and cited there —
/// they are not to drift independently.
const PANEL_WIDTH: f32 = 240.0;
const PANEL_MARGIN: f32 = 16.0;
const PANEL_PAD: f32 = 8.0;
const ROW_PITCH: f32 = 18.0;
const TEXT_SIZE: f32 = 14.0;
const BAR_HEIGHT: f32 = 10.0;
/// Bar left edge, relative to its panel: 240 - 96 - 8 = 136 wide.
const BAR_INSET: f32 = 96.0;
const BAR_WIDTH: f32 = PANEL_WIDTH - BAR_INSET - PANEL_PAD;
/// A damage indicator's line reaches this far along its direction.
const DAMAGE_REACH: f32 = 40.0;
const PANEL_BACKGROUND: [f32; 4] = [0.05, 0.05, 0.08, 0.85];
const LABEL_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const OBJECTIVE_DONE: [f32; 4] = [0.35, 0.9, 0.35, 1.0];
const OBJECTIVE_OPEN: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const COMPASS_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// One row of an info panel: a label on the left, and — where the row reports
/// a quantity — a bar anchored at `BAR_INSET`.
enum Row {
    Label(String),
    /// Label text and the already-clamped, NaN-free fill fraction for the bar.
    Bar(String, f32),
}

/// `value / max` clamped into 0..1, with the degenerate denominator handled:
/// `f32` division by zero is NaN, and a NaN `progress` survives the
/// renderer's own `.clamp(0.0, 1.0)` and reaches the GPU as NaN geometry.
fn ratio(value: f32, max: f32) -> f32 {
    if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Emits one visible HUD element: texture-backed elements go out as full-rect
/// `Image` commands, the rest as flat quads (a `mesh` field with no texture is
/// v2 and co-equally ignored by the old stub).
fn push_element(batch: &mut UiBatch, element: &HUDElement, screen: Vec2) {
    let at = element.calculate_position(screen);
    if let Some(texture) = &element.texture {
        let id = batch.push_texture(texture);
        batch.commands.push(UiCommand::Image {
            texture_id: id,
            x: at.x,
            y: at.y,
            width: element.size.x,
            height: element.size.y,
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
            color: rgba(element.color),
        });
    } else {
        batch.commands.push(UiCommand::Rectangle {
            x: at.x,
            y: at.y,
            width: element.size.x,
            height: element.size.y,
            color: rgba(element.color),
            corner_radius: 0.0,
        });
    }
}

/// Emits one info panel: the background rectangle, then its rows left-padded
/// by `PANEL_PAD`, one every `ROW_PITCH`; bars sit at `BAR_INSET`, centred in
/// the row.
fn push_panel(batch: &mut UiBatch, x: f32, y: f32, rows: &[Row]) {
    batch.commands.push(UiCommand::Rectangle {
        x,
        y,
        width: PANEL_WIDTH,
        height: rows.len() as f32 * ROW_PITCH,
        color: PANEL_BACKGROUND,
        corner_radius: 0.0,
    });
    for (index, row) in rows.iter().enumerate() {
        let y = y + index as f32 * ROW_PITCH;
        let (text, bar) = match row {
            Row::Label(text) => (text, None),
            Row::Bar(text, progress) => (text, Some(*progress)),
        };
        batch.commands.push(UiCommand::Text {
            content: text.clone(),
            x: x + PANEL_PAD,
            y: y + (ROW_PITCH - TEXT_SIZE) * 0.5,
            size: TEXT_SIZE,
            color: LABEL_COLOR,
            font: None,
        });
        if let Some(progress) = bar {
            batch.commands.push(UiCommand::ProgressBar {
                x: x + BAR_INSET,
                y: y + (ROW_PITCH - BAR_HEIGHT) * 0.5,
                width: BAR_WIDTH,
                height: BAR_HEIGHT,
                progress,
                background_color: [0.2, 0.2, 0.25, 1.0],
                fill_color: [0.3, 0.8, 0.3, 1.0],
                corner_radius: 0.0,
            });
        }
    }
}

impl Default for HUD {
    fn default() -> Self {
        Self::new(1920, 1080)
    }
}

/// Ship HUD information
#[derive(Debug, Clone, Default)]
pub struct ShipHUDInfo {
    pub name: String,
    pub hp: f32,
    pub max_hp: f32,
    pub speed: f32,
    pub max_speed: f32,
    pub fuel: f32,
    pub max_fuel: f32,
    pub crew_count: usize,
    pub max_crew: usize,
    pub fire_level: f32,
    pub water_level: f32,
}

/// Weapon HUD information
#[derive(Debug, Clone, Default)]
pub struct WeaponHUDInfo {
    pub name: String,
    pub ammo: usize,
    pub max_ammo: usize,
    pub cooldown: f32,
    pub max_cooldown: f32,
    pub ready: bool,
    pub reloading: bool,
}

/// Player HUD information
#[derive(Debug, Clone, Default)]
pub struct PlayerHUDInfo {
    pub name: String,
    pub health: f32,
    pub max_health: f32,
    pub role: String,
    pub team: String,
    pub alive: bool,
}

/// HUD objective
#[derive(Debug, Clone)]
pub struct HUDObjective {
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub position: Vec2,
    pub visible: bool,
}

impl Default for HUDObjective {
    fn default() -> Self {
        Self {
            title: "Objective".to_string(),
            description: "Complete the objective".to_string(),
            completed: false,
            position: Vec2::ZERO,
            visible: true,
        }
    }
}

/// HUD compass
#[derive(Debug, Clone)]
pub struct HUDCompass {
    pub position: Vec2,
    pub size: Vec2,
    pub north_direction: f32,
    pub visible: bool,
    pub markers: Vec<CompassMarker>,
}

impl Default for HUDCompass {
    fn default() -> Self {
        Self {
            position: Vec2::new(0.0, -100.0),
            size: Vec2::new(200.0, 40.0),
            north_direction: 0.0,
            visible: true,
            markers: Vec::new(),
        }
    }
}

/// Compass marker
#[derive(Debug, Clone)]
pub struct CompassMarker {
    pub name: String,
    pub direction: f32,
    pub distance: f32,
    pub color: Vec4,
    pub icon: Option<Arc<Texture>>,
}

impl Default for CompassMarker {
    fn default() -> Self {
        Self {
            name: "Marker".to_string(),
            direction: 0.0,
            distance: 0.0,
            color: Vec4::ONE,
            icon: None,
        }
    }
}

/// HUD damage indicator
#[derive(Debug, Clone)]
pub struct HUDDamageIndicator {
    pub position: Vec2,
    pub direction: Vec2,
    pub intensity: f32,
    pub color: Vec4,
    pub size: Vec2,
    pub lifetime: f32,
    pub max_lifetime: f32,
}

impl HUDDamageIndicator {
    pub fn new(position: Vec2, direction: Vec2, intensity: f32) -> Self {
        Self {
            position,
            direction,
            intensity,
            color: Vec4::new(1.0, 0.0, 0.0, 0.8),
            size: Vec2::new(20.0, 2.0),
            lifetime: 0.0,
            max_lifetime: 1.0,
        }
    }

    pub fn update(&mut self, delta_time: std::time::Duration) {
        self.lifetime += delta_time.as_secs_f32();
    }

    pub fn expired(&self) -> bool {
        self.lifetime >= self.max_lifetime
    }
}

impl Default for HUDDamageIndicator {
    fn default() -> Self {
        Self::new(Vec2::ZERO, Vec2::ZERO, 1.0)
    }
}
