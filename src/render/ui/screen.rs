//! Menu screens: the state machine between the main menu, settings, pause
//! and play, plus the layout that makes the panel usable.
//!
//! **CLIENT-SIDE; THE LOADING SCREEN IS THE CONNECTION'S UX**
//!
//! The wire itself lives in `crate::net`; this module owns what the player
//! sees while the link comes up or goes down: the full-screen loading
//! screen laid out like the Squad loading screen (dark takeover, logo
//! top-left, the server's name in large type above a muted status line
//! and a thin white progress bar, one cancel control top-right), the
//! Play -> Connecting -> Playing path, and the CancelConnect command that
//! tells the owner to stop the bridge.
//!
//! `Menu` owns how a menu *looks*; this module owns *which* menu is open and
//! what activating an item means. The split matters: `Menu::to_commands` is
//! shared with tests and with the HUD-era code, while the state machine is the
//! only thing that ever drives a menu from input.
//!
//! Layout is deliberately in physical pixels, matching `UIPass`: the window is
//! created at a logical size while the swapchain holds physical pixels, so a
//! panel centred from logical coordinates covers a corner at any scale factor
//! other than 100%. The owner publishes the presentation extent every frame
//! through [`MenuScreen::set_screen_size`], and the panel is re-centred from
//! that.

use glam::{Vec2, Vec4};

use super::fonts::{FONT_HEADING, FONT_REGULAR};
use super::menu::{Menu, MenuItem, MenuItemType, MenuScrim, MenuType};
use super::model::UiModel;

/// Panel width in physical pixels.
pub const PANEL_WIDTH: f32 = 380.0;
/// Button width. The difference to [`PANEL_WIDTH`] is the panel's side pad.
pub const BUTTON_WIDTH: f32 = 320.0;
/// Button height.
pub const BUTTON_HEIGHT: f32 = 48.0;
/// Vertical distance between button tops.
pub const ITEM_PITCH: f32 = 64.0;
/// Height of the title zone; the first item starts here.
pub const HEADER_HEIGHT: f32 = 88.0;
/// Padding below the last item.
pub const BOTTOM_PAD: f32 = 18.0;
/// Horizontal inset of every item inside the panel.
pub const PANEL_SIDE_PAD: f32 = (PANEL_WIDTH - BUTTON_WIDTH) * 0.5;
/// One arrow-key or click step on the wave-height slider.
pub const WAVE_STEP: f32 = 0.05;

const SCRIM_COLOR: Vec4 = Vec4::new(0.015, 0.025, 0.04, 0.62);
const PANEL_COLOR: Vec4 = Vec4::new(0.055, 0.075, 0.10, 0.96);
const ITEM_NORMAL_COLOR: Vec4 = Vec4::new(0.12, 0.17, 0.23, 1.0);
const ITEM_SELECTED_COLOR: Vec4 = Vec4::new(0.19, 0.38, 0.52, 1.0);
const ITEM_DISABLED_COLOR: Vec4 = Vec4::new(0.09, 0.10, 0.12, 0.7);

// The loading screen's palette, laid out like the Squad loading screen: a
// near-opaque dark takeover, white type, one muted sub-line, one thin white
// bar. The two big lines ride the real heading face, the small text the
// real body face (`render::ui::fonts`) — the 8px-cell sizes were picked for
// the placeholder's integer scaling and read fine in the real faces too,
// so the layout constants survived the swap unchanged.
const LOADING_MARGIN: f32 = 48.0;
/// Sizes in whole 8px steps: the bitmap font scales by integer factors.
const LOADING_LOGO_SIZE: f32 = 48.0;
const LOADING_NAME_SIZE: f32 = 48.0;
const LOADING_SUB_SIZE: f32 = 16.0;
const LOADING_BAR_HEIGHT: f32 = 8.0;
const LOADING_CANCEL_WIDTH: f32 = 160.0;
const LOADING_CANCEL_HEIGHT: f32 = 40.0;
const LOADING_BACKGROUND: Vec4 = Vec4::new(0.028, 0.033, 0.042, 0.985);
const LOADING_TEXT: Vec4 = Vec4::new(1.0, 1.0, 1.0, 1.0);
const LOADING_MUTED: Vec4 = Vec4::new(0.70, 0.73, 0.78, 1.0);
/// The bar's fill reads white regardless of focus, which is what Squad's
/// bare bar does; the track stays the menu's own `TRACK_COLOR`.
const LOADING_BAR_FILL: Vec4 = Vec4::new(0.96, 0.96, 0.97, 1.0);
const LOADING_CANCEL_COLOR: Vec4 = Vec4::new(0.14, 0.15, 0.18, 1.0);
const LOADING_CANCEL_HOVER: Vec4 = Vec4::new(0.24, 0.26, 0.30, 1.0);

/// Which screen the player is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// The first screen after start-up.
    MainMenu,
    /// Opened from the main menu or from pause.
    Settings,
    /// The game is running but input belongs to the pause menu.
    Paused,
    /// The Play item was activated: the client is trying to reach the
    /// server. On success this becomes `Playing`; on failure the panel
    /// stays and shows why, with Back to return to the main menu.
    Connecting,
    /// No menu: the camera and game input are live.
    Playing,
}

/// One input event, already translated out of winit's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuInput {
    Up,
    Down,
    Left,
    Right,
    Activate,
    Back,
    PointerMoved(Vec2),
    PointerPressed(Vec2),
}

/// What the owner has to do after the screen machine handled an input.
///
/// Screen transitions happen inside [`MenuScreen`]; commands are the effects
/// outside it (quit the loop, move the camera, change the water, start or
/// stop the server link). Keeping the two apart means the state machine stays
/// testable without a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuCommand {
    StartGame,
    Resume,
    OpenSettings,
    Back,
    QuitToMenu,
    Quit,
    /// The slider moved; the value is in `0.0..=1.0`.
    WaveHeightChanged(f32),
    /// The player walked out of a connection attempt (Back on the
    /// connecting panel, or Quit to Menu while connected). The owner stops
    /// the server link.
    CancelConnect,
}

/// The menu state machine.
pub struct MenuScreen {
    screen: Screen,
    /// Where Settings returns to: the main menu or the pause menu.
    settings_return: Screen,
    screen_size: Vec2,
    wave_height: f32,
    focus: usize,
    /// The one read-only line the connecting panel shows: what the client is
    /// doing or why it could not. Empty outside `Screen::Connecting`.
    status_line: String,
    /// The loading screen's progress, `0.0..=1.0`. Stages: the attempt
    /// started, the server answered, the world arrived. The bar is the
    /// only place this shows.
    progress: f32,
    /// The big line on the loading screen: where the client is loading
    /// into. Squad shows the map name there; our analog is the server the
    /// Play item dialed. The owner sets it when the attempt starts.
    loading_name: String,
}

/// The loading screen's items, in build order:
/// `0` logo, `1` map name, `2` status line, `3` progress bar,
/// `4` the cancel button — the one control, so the one focus target.
const LOADING_LOGO: usize = 0;
const LOADING_NAME: usize = 1;
const LOADING_STATUS: usize = 2;
const LOADING_BAR: usize = 3;
const LOADING_CANCEL: usize = 4;

impl MenuScreen {
    pub fn new() -> Self {
        Self {
            screen: Screen::MainMenu,
            settings_return: Screen::MainMenu,
            // Replaced by the presentation extent before the first frame.
            screen_size: Vec2::new(1280.0, 720.0),
            wave_height: 0.5,
            focus: 0,
            status_line: String::new(),
            progress: 0.0,
            loading_name: "SERVER".to_string(),
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn wave_height(&self) -> f32 {
        self.wave_height
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_size = Vec2::new(width.max(1.0), height.max(1.0));
    }

    pub fn screen_size(&self) -> Vec2 {
        self.screen_size
    }

    /// Enter the pause screen from play. A no-op on any other screen, so a
    /// stray Escape cannot reopen a closed menu.
    pub fn open_pause(&mut self) {
        if self.screen == Screen::Playing {
            self.screen = Screen::Paused;
            self.settings_return = Screen::Paused;
            self.focus = 0;
        }
    }

    /// Open the loading screen. `status` is the read-only line it shows
    /// (what the client is doing, or later why it failed). The focus starts
    /// on the cancel button, so Escape — which is `Back` in every menu —
    /// abandons the attempt.
    pub fn open_connecting(&mut self, status: &str) {
        self.screen = Screen::Connecting;
        self.status_line = status.to_string();
        self.progress = 0.0;
        self.focus = LOADING_CANCEL;
    }

    /// Move the loading screen's progress bar. Values clamp into
    /// `0.0..=1.0`; off-screen or after the world arrived the bar is gone
    /// and the call is harmless.
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    /// The loading screen's progress, for tests and diagnostics.
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// The world arrived: hide the loading screen and hand control to the
    /// game. A no-op from any other screen — a snapshot that lands while
    /// the player is in pause or back at the menu must not yank them into
    /// play.
    pub fn connect_succeeded(&mut self) {
        if self.screen == Screen::Connecting {
            self.screen = Screen::Playing;
            self.status_line.clear();
            self.progress = 1.0;
            self.focus = 0;
        }
    }

    /// Replace the connecting panel's line (a progress note, or the reason
    /// the attempt failed — the panel keeps its Back item either way).
    pub fn set_status_line(&mut self, status: &str) {
        self.status_line = status.to_string();
    }

    /// Set the loading screen's big line — the server the attempt dials.
    /// Squad shows the map name in that slot; the server address is what
    /// the client honestly knows. Set when the attempt starts, so the
    /// first frame of the loading screen already names the target.
    pub fn set_loading_name(&mut self, name: &str) {
        // Squad sets its map name in caps; the bitmap font has no
        // small-caps style, so the copy uppercases here.
        self.loading_name = name.to_uppercase();
    }

    /// The loading screen's big line, for tests and diagnostics.
    pub fn loading_name(&self) -> &str {
        &self.loading_name
    }

    /// The connecting panel's current line, for tests and diagnostics.
    pub fn status_line(&self) -> &str {
        &self.status_line
    }

    /// Publish this screen's menu into the UI model.
    ///
    /// The menu is rebuilt rather than mutated: item positions depend on the
    /// current extent, and the model is the batch the pass compiles. On
    /// `Playing` the menu is removed entirely — an empty menu that still owns
    /// a command list is exactly the kind of invisible state this repository
    /// keeps finding.
    pub fn sync(&self, model: &mut UiModel) {
        if self.screen == Screen::Playing {
            model.menu = None;
            return;
        }

        let mut menu = Menu::new(self.menu_type());
        menu.title = self.title().to_string();

        if self.screen == Screen::Connecting {
            // The Squad-style loading screen: a full-screen takeover, not a
            // floating panel. The menu's own background is the layer — no
            // scrim behind a panel that already covers everything — and
            // the title slot stays empty because the logo is an item.
            menu.position = Vec2::ZERO;
            menu.size = self.screen_size;
            menu.background_color = LOADING_BACKGROUND;
            menu.set_scrim(None);
            menu.items = self.build_items();
            menu.selected_index = self.focus.min(menu.items.len().saturating_sub(1));
            model.menu = Some(menu);
            return;
        }

        let panel_position = self.panel_origin();
        menu.position = panel_position;
        menu.size = self.panel_size();
        menu.background_color = PANEL_COLOR;
        menu.set_scrim(Some(MenuScrim {
            position: Vec2::ZERO,
            size: self.screen_size,
            color: SCRIM_COLOR,
        }));
        menu.items = self.build_items();
        menu.selected_index = self.focus.min(menu.items.len().saturating_sub(1));
        model.menu = Some(menu);
    }

    /// Handle one input. Returns the effect the owner must apply, if any.
    pub fn handle(&mut self, input: MenuInput) -> Option<MenuCommand> {
        if self.screen == Screen::Playing {
            return None;
        }

        match input {
            MenuInput::Up => {
                self.move_focus(-1);
                None
            }
            MenuInput::Down => {
                self.move_focus(1);
                None
            }
            MenuInput::Left => self.adjust_wave(-WAVE_STEP),
            MenuInput::Right => self.adjust_wave(WAVE_STEP),
            MenuInput::Activate => self.activate(self.focus),
            MenuInput::Back => self.back(),
            MenuInput::PointerMoved(point) => {
                if let Some(index) = self.item_at(point) {
                    self.focus = index;
                }
                None
            }
            MenuInput::PointerPressed(point) => {
                let Some(index) = self.item_at(point) else {
                    return None;
                };
                self.focus = index;

                // A slider is set from where it was clicked, the way a real
                // track works; a button is simply activated.
                if self.screen == Screen::Settings && index == 0 {
                    let origin = self.item_origin(0);
                    let ratio = ((point.x - origin.x) / BUTTON_WIDTH).clamp(0.0, 1.0);
                    self.wave_height = ratio;
                    Some(MenuCommand::WaveHeightChanged(ratio))
                } else {
                    self.activate(index)
                }
            }
        }
    }

    // ------------------------------------------------------------- layout

    fn menu_type(&self) -> MenuType {
        match self.screen {
            Screen::MainMenu => MenuType::Main,
            Screen::Settings => MenuType::Settings,
            Screen::Paused => MenuType::Pause,
            // The loading screen is a full-screen takeover (MenuType::Loading),
            // not a floating panel; `sync` lays it out separately.
            Screen::Connecting => MenuType::Loading,
            Screen::Playing => MenuType::InGame,
        }
    }

    fn title(&self) -> &'static str {
        match self.screen {
            Screen::MainMenu => "RFS",
            Screen::Settings => "Settings",
            Screen::Paused => "Paused",
            // The loading screen draws its own logo item; the Menu title
            // slot would duplicate it at the panel's small size.
            Screen::Connecting => "",
            Screen::Playing => "",
        }
    }

    fn item_count(&self) -> usize {
        match self.screen {
            Screen::MainMenu => 3,
            Screen::Settings => 2,
            Screen::Paused => 4,
            Screen::Connecting => 5,
            Screen::Playing => 0,
        }
    }

    fn panel_size(&self) -> Vec2 {
        Vec2::new(
            PANEL_WIDTH,
            HEADER_HEIGHT + self.item_count() as f32 * ITEM_PITCH + BOTTOM_PAD,
        )
    }

    fn panel_origin(&self) -> Vec2 {
        ((self.screen_size - self.panel_size()) * 0.5).max(Vec2::ZERO)
    }

    fn item_origin(&self, index: usize) -> Vec2 {
        let panel = self.panel_origin();
        Vec2::new(
            panel.x + PANEL_SIDE_PAD,
            panel.y + HEADER_HEIGHT + index as f32 * ITEM_PITCH,
        )
    }

    /// Where item `index` sits and how big it is, on the current screen.
    ///
    /// The panel screens share one grid (`item_origin` plus the fixed
    /// button size); the loading screen has five different shapes, laid
    /// out like the Squad loading screen: logo top-left, map name and
    /// status above a full-width bar at the bottom edge, cancel top-right.
    /// One method is what keeps the hit test and the build honest with
    /// each other.
    fn item_rect(&self, index: usize) -> (Vec2, Vec2) {
        if self.screen == Screen::Connecting {
            return self.loading_rect(index);
        }
        (
            self.item_origin(index),
            Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT),
        )
    }

    /// The loading screen's geometry: one edge margin from every screen
    /// edge, the bar pinned to the bottom edge, the name and status lines
    /// stacked above it, the logo and the cancel in the top corners.
    fn loading_rect(&self, index: usize) -> (Vec2, Vec2) {
        let width = (self.screen_size.x - LOADING_MARGIN * 2.0).max(1.0);
        // The bar's bottom edge sits on the margin line.
        let bar_y = (self.screen_size.y - LOADING_MARGIN - LOADING_BAR_HEIGHT).max(0.0);
        let top_right = Vec2::new(
            (self.screen_size.x - LOADING_MARGIN - LOADING_CANCEL_WIDTH).max(0.0),
            LOADING_MARGIN,
        );
        match index {
            LOADING_LOGO => (
                Vec2::new(LOADING_MARGIN, LOADING_MARGIN),
                Vec2::new(320.0, LOADING_LOGO_SIZE + 8.0),
            ),
            LOADING_NAME => (
                Vec2::new(LOADING_MARGIN, (bar_y - 100.0).max(0.0)),
                Vec2::new(width, LOADING_NAME_SIZE + 8.0),
            ),
            LOADING_STATUS => (
                Vec2::new(LOADING_MARGIN, (bar_y - 34.0).max(0.0)),
                Vec2::new(width, LOADING_SUB_SIZE + 4.0),
            ),
            LOADING_BAR => (
                Vec2::new(LOADING_MARGIN, bar_y),
                Vec2::new(width, LOADING_BAR_HEIGHT),
            ),
            LOADING_CANCEL => (top_right, Vec2::new(LOADING_CANCEL_WIDTH, LOADING_CANCEL_HEIGHT)),
            _ => (Vec2::ZERO, Vec2::new(1.0, 1.0)),
        }
    }

    fn item_at(&self, point: Vec2) -> Option<usize> {
        (0..self.item_count()).find(|&index| {
            let (at, size) = self.item_rect(index);
            point.x >= at.x && point.x <= at.x + size.x && point.y >= at.y && point.y <= at.y + size.y
        })
    }

    fn button(&self, text: &str, index: usize) -> MenuItem {
        MenuItem::new(text, MenuItemType::Button)
            .with_position(self.item_origin(index))
            .with_size(Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT))
            .with_colors(ITEM_NORMAL_COLOR, ITEM_SELECTED_COLOR, ITEM_DISABLED_COLOR)
    }

    fn wave_slider(&self, index: usize) -> MenuItem {
        let percent = (self.wave_height * 100.0).round() as i32;
        MenuItem::new("wave_height", MenuItemType::Slider)
            .with_text(&format!("Wave Height  {percent:>3}%"))
            .with_position(self.item_origin(index))
            .with_size(Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT))
            .with_value_range(self.wave_height, 0.0, 1.0)
            .with_colors(ITEM_NORMAL_COLOR, ITEM_SELECTED_COLOR, ITEM_DISABLED_COLOR)
    }

    /// The connecting panel's status line: text only, no button, never
    /// selectable. `enabled: false` is what keeps the highlight off it when
    /// focus walks past, and greys it the way disabled text reads.
    fn status_label(&self, index: usize) -> MenuItem {
        MenuItem::new("status", MenuItemType::Label)
            .with_text(&truncate_status_line(&self.status_line))
            .with_position(self.item_origin(index))
            .with_size(Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT))
            .with_colors(ITEM_NORMAL_COLOR, ITEM_SELECTED_COLOR, ITEM_DISABLED_COLOR)
            .with_enabled(false)
    }

    /// A loading screen label: text only, never selectable, sized, coloured
    /// and faced by its slot (logo and name carry the heading face, the
    /// muted status line the body face).
    fn loading_label(&self, text: &str, index: usize, text_size: f32, color: Vec4, font: &str) -> MenuItem {
        let (position, size) = self.loading_rect(index);
        MenuItem::new("loading", MenuItemType::Label)
            .with_text(text)
            .with_position(position)
            .with_size(size)
            .with_text_size(text_size)
            .with_text_color(color)
            .with_font(font)
            .with_colors(LOADING_TEXT, LOADING_TEXT, LOADING_TEXT)
            .with_enabled(false)
    }

    /// The loading screen's progress bar: a bare Slider whose text is
    /// empty, so the bar speaks alone — Squad's bar carries no label
    /// either. `enabled: false` pins the fill to the white colour whatever
    /// the focus does, and the value is this frame's progress.
    fn loading_bar(&self) -> MenuItem {
        let (position, size) = self.loading_rect(LOADING_BAR);
        MenuItem::new("loading_progress", MenuItemType::Slider)
            .with_text("")
            .with_position(position)
            .with_size(size)
            .with_value_range(self.progress, 0.0, 1.0)
            .with_font(FONT_REGULAR)
            .with_colors(LOADING_BAR_FILL, LOADING_BAR_FILL, LOADING_BAR_FILL)
            .with_enabled(false)
    }

    /// The loading screen's one control. Squad's loading screen has no
    /// button; the escape hint doubles as one, so a pointer user can leave
    /// the same way a keyboard user does.
    fn loading_cancel(&self) -> MenuItem {
        let (position, size) = self.loading_rect(LOADING_CANCEL);
        MenuItem::new("loading_cancel", MenuItemType::Button)
            .with_text("[ESC] CANCEL")
            .with_position(position)
            .with_size(size)
            .with_text_size(LOADING_SUB_SIZE)
            .with_font(FONT_REGULAR)
            .with_colors(LOADING_CANCEL_COLOR, LOADING_CANCEL_HOVER, LOADING_CANCEL_COLOR)
    }

    fn build_items(&self) -> Vec<MenuItem> {
        match self.screen {
            Screen::MainMenu => vec![
                self.button("Play", 0),
                self.button("Settings", 1),
                self.button("Quit", 2),
            ],
            Screen::Settings => vec![self.wave_slider(0), self.button("Back", 1)],
            Screen::Paused => vec![
                self.button("Resume", 0),
                self.button("Settings", 1),
                self.button("Quit to Menu", 2),
                self.button("Quit Game", 3),
            ],
            Screen::Connecting => vec![
                self.loading_label("RFS", LOADING_LOGO, LOADING_LOGO_SIZE, LOADING_TEXT, FONT_HEADING),
                self.loading_label(
                    &self.loading_name,
                    LOADING_NAME,
                    LOADING_NAME_SIZE,
                    LOADING_TEXT,
                    FONT_HEADING,
                ),
                self.loading_label(
                    &truncate_status_line(&self.status_line),
                    LOADING_STATUS,
                    LOADING_SUB_SIZE,
                    LOADING_MUTED,
                    FONT_REGULAR,
                ),
                self.loading_bar(),
                self.loading_cancel(),
            ],
            Screen::Playing => Vec::new(),
        }
    }

    // ---------------------------------------------------------- behaviour

    fn move_focus(&mut self, delta: isize) {
        // The loading screen has one control; walking past it only moves
        // the highlight onto text that cannot be activated.
        if self.screen == Screen::Connecting {
            self.focus = LOADING_CANCEL;
            return;
        }
        let count = self.item_count();
        if count == 0 {
            return;
        }
        self.focus = (self.focus as isize + delta).rem_euclid(count as isize) as usize;
    }

    fn adjust_wave(&mut self, delta: f32) -> Option<MenuCommand> {
        // Only the first Settings row is a slider; pressing left/right on
        // "Back" must not silently change the water.
        if self.screen != Screen::Settings || self.focus != 0 {
            return None;
        }
        self.wave_height = (self.wave_height + delta).clamp(0.0, 1.0);
        Some(MenuCommand::WaveHeightChanged(self.wave_height))
    }

    fn activate(&mut self, index: usize) -> Option<MenuCommand> {
        match self.screen {
            Screen::MainMenu => match index {
                // Play no longer jumps into a local scene: it opens the
                // connecting panel, and the owner starts the server link.
                // Only a Connected event turns this into `Playing`.
                0 => {
                    self.open_connecting("Connecting to the server...");
                    Some(MenuCommand::StartGame)
                }
                1 => {
                    self.screen = Screen::Settings;
                    self.settings_return = Screen::MainMenu;
                    self.focus = 0;
                    Some(MenuCommand::OpenSettings)
                }
                2 => Some(MenuCommand::Quit),
                _ => None,
            },
            Screen::Settings => match index {
                1 => self.back(),
                _ => None,
            },
            Screen::Paused => match index {
                0 => {
                    self.screen = Screen::Playing;
                    Some(MenuCommand::Resume)
                }
                1 => {
                    self.screen = Screen::Settings;
                    self.settings_return = Screen::Paused;
                    self.focus = 0;
                    Some(MenuCommand::OpenSettings)
                }
                // Quit to Menu from a live session is also a disconnect: the
                // owner stops the link either way.
                2 => {
                    self.screen = Screen::MainMenu;
                    self.settings_return = Screen::MainMenu;
                    self.focus = 0;
                    Some(MenuCommand::QuitToMenu)
                }
                3 => Some(MenuCommand::Quit),
                _ => None,
            },
            Screen::Connecting => match index {
                // The logo, the name, the status line and the bar are
                // text or readouts, not controls; only Cancel acts.
                LOADING_CANCEL => self.back(),
                _ => None,
            },
            Screen::Playing => None,
        }
    }

    fn back(&mut self) -> Option<MenuCommand> {
        match self.screen {
            Screen::Settings => {
                self.screen = self.settings_return;
                self.focus = 0;
                Some(MenuCommand::Back)
            }
            Screen::Paused => {
                self.screen = Screen::Playing;
                Some(MenuCommand::Resume)
            }
            Screen::Connecting => {
                // Abandoning the attempt. The owner stops the link; whether
                // it was still connecting or already established does not
                // change what the menu has to do. The name stays — the
                // next attempt likely dials the same server.
                self.screen = Screen::MainMenu;
                self.status_line.clear();
                self.progress = 0.0;
                self.focus = 0;
                Some(MenuCommand::CancelConnect)
            }
            Screen::MainMenu => Some(MenuCommand::Quit),
            Screen::Playing => None,
        }
    }
}

/// The status line is one muted row under the big name; a long reason is
/// cut with an ellipsis rather than running under the screen edge.
fn truncate_status_line(status: &str) -> String {
    const LIMIT: usize = 96;
    if status.chars().count() <= LIMIT {
        return status.to_string();
    }
    // The ellipsis is part of the budget, so the line never grows past LIMIT.
    let head: String = status.chars().take(LIMIT - 3).collect();
    format!("{head}...")
}

impl Default for MenuScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhi::ui::UiCommand;

    fn compiled(screen: &MenuScreen) -> (Menu, Vec<UiCommand>) {
        let mut model = UiModel::new();
        screen.sync(&mut model);
        let menu = model.menu.expect("the screen must publish a menu");
        let mut batch = crate::render::ui::UiBatch::default();
        menu.to_commands(&mut batch);
        (menu, batch.commands)
    }

    #[test]
    fn the_main_menu_is_a_centred_panel_with_three_buttons() {
        let screen = MenuScreen::new();
        let (menu, commands) = compiled(&screen);

        assert_eq!(menu.items.len(), 3);
        assert_eq!(menu.items[0].text, "Play");
        assert_eq!(menu.items[1].text, "Settings");
        assert_eq!(menu.items[2].text, "Quit");
        assert!(menu.scrim.is_some(), "a menu over the scene needs the dim");

        // 1280x720, panel 380x298: origin (450, 211); the first button starts
        // 30 px in and 88 px down.
        assert!(menu.position.abs_diff_eq(Vec2::new(450.0, 211.0), 1e-4));
        assert!(
            menu.items[0]
                .position
                .abs_diff_eq(Vec2::new(480.0, 299.0), 1e-4),
            "got {:?}",
            menu.items[0].position
        );
        assert_eq!(menu.items[0].size, Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT));

        // scrim + panel + title + (button + label) * 3
        assert_eq!(commands.len(), 9);
        assert!(matches!(commands[0], UiCommand::Rectangle { .. }));
    }

    #[test]
    fn play_opens_the_loading_screen_not_the_game() {
        let mut screen = MenuScreen::new();

        assert_eq!(
            screen.handle(MenuInput::Activate),
            Some(MenuCommand::StartGame)
        );
        // The loading screen is up while the attempt is in flight; only a
        // world snapshot turns this into play.
        assert_eq!(screen.screen(), Screen::Connecting);

        let (menu, _) = compiled(&screen);
        assert_eq!(menu.items.len(), 5);
        assert_eq!(menu.items[LOADING_LOGO].text, "RFS");
        assert_eq!(menu.items[LOADING_NAME].text, "SERVER");
        assert!(
            !menu.items[LOADING_BAR].enabled,
            "the bar is a readout, not a control"
        );
        assert_eq!(menu.items[LOADING_CANCEL].text, "[ESC] CANCEL");
        assert!(
            menu.items[LOADING_CANCEL].enabled,
            "the cancel button is the one control"
        );
        assert_eq!(menu.selected_index, LOADING_CANCEL, "focus pins on Cancel");
    }

    #[test]
    fn the_loading_screen_rides_the_real_faces_and_the_panels_do_not() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play -> Connecting
        let (loading, _) = compiled(&screen);

        // The two big lines carry the heading face; the small text the
        // body face. That is the typographic half of the Squad copy.
        assert_eq!(loading.items[LOADING_LOGO].font.as_deref(), Some(FONT_HEADING));
        assert_eq!(loading.items[LOADING_NAME].font.as_deref(), Some(FONT_HEADING));
        assert_eq!(loading.items[LOADING_STATUS].font.as_deref(), Some(FONT_REGULAR));
        assert_eq!(loading.items[LOADING_BAR].font.as_deref(), Some(FONT_REGULAR));
        assert_eq!(loading.items[LOADING_CANCEL].font.as_deref(), Some(FONT_REGULAR));

        // Every other screen keeps the placeholder: its item font stays
        // unset, so the renderer default (bitmap8) drives them and the
        // old menus remain pixel-identical.
        let (main, _) = compiled(&MenuScreen::new());
        assert!(main.items.iter().all(|item| item.font.is_none()));
    }

    #[test]
    fn the_loading_screen_is_a_full_screen_takeover() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play -> Connecting
        let (menu, commands) = compiled(&screen);

        // Squad's loading screen covers the picture: the menu's own
        // background is the layer, no panel, no separate dim.
        assert!(menu.position.abs_diff_eq(Vec2::ZERO, 1e-4));
        assert!(menu.size.abs_diff_eq(Vec2::new(1280.0, 720.0), 1e-4));
        assert!(menu.scrim.is_none(), "the background IS the takeover");
        assert!(menu.title.is_empty(), "the logo item plays that role");
        let background = match &commands[0] {
            UiCommand::Rectangle { x, y, width, height, .. } => {
                (*x, *y, *width, *height)
            }
            other => panic!("the first command must be the background, got {other:?}"),
        };
        assert_eq!(background, (0.0, 0.0, 1280.0, 720.0));
    }

    #[test]
    fn the_loading_screen_lays_out_like_squad() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play -> Connecting
        screen.set_loading_name("127.0.0.1:7777");
        screen.set_progress(0.25);
        let (menu, commands) = compiled(&screen);

        // The name is the big line, uppercased the way Squad sets map
        // names; the logo sits top-left, above the margin line.
        assert_eq!(menu.items[LOADING_NAME].text, "127.0.0.1:7777".to_uppercase());
        assert_eq!(
            menu.items[LOADING_NAME].text_size,
            Some(LOADING_NAME_SIZE)
        );
        let logo_at = menu.items[LOADING_LOGO].position;
        assert_eq!(logo_at.x, LOADING_MARGIN);
        assert_eq!(menu.items[LOADING_LOGO].text_size, Some(LOADING_LOGO_SIZE));

        // The bar is the full inner width, pinned above the bottom margin.
        let (bar_at, bar_size) = (
            menu.items[LOADING_BAR].position,
            menu.items[LOADING_BAR].size,
        );
        assert_eq!(bar_at.x, LOADING_MARGIN);
        assert!(
            (bar_size.x - (1280.0 - LOADING_MARGIN * 2.0)).abs() < 1e-4,
            "the bar spans the inner width"
        );
        assert!(
            (bar_at.y + bar_size.y - (720.0 - LOADING_MARGIN)).abs() < 1e-4,
            "the bar's bottom edge sits on the margin line"
        );

        // The progress arrives as a real ProgressBar command.
        let bar = commands.iter().find_map(|command| match command {
            UiCommand::ProgressBar { progress, .. } => Some(*progress),
            _ => None,
        });
        assert_eq!(bar, Some(0.25), "the bar shows the current stage");
    }

    #[test]
    fn the_progress_bar_follows_the_stages_and_clamps() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play -> Connecting

        screen.set_progress(0.10);
        assert!((screen.progress() - 0.10).abs() < 1e-6);
        screen.set_progress(0.55);
        assert!((screen.progress() - 0.55).abs() < 1e-6);

        // Overflow and nonsense clamp into 0..=1: the bar is a ratio.
        screen.set_progress(2.0);
        assert!((screen.progress() - 1.0).abs() < 1e-6);
        screen.set_progress(-1.0);
        assert!((screen.progress() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn a_successful_connection_enters_the_game_and_hides_the_menu() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play -> Connecting
        screen.connect_succeeded(); // the world arrived

        assert_eq!(screen.screen(), Screen::Playing);
        assert!(screen.status_line().is_empty(), "the loading line is gone");
        assert!((screen.progress() - 1.0).abs() < 1e-6, "the bar finished");

        let mut model = UiModel::new();
        screen.sync(&mut model);
        assert!(model.menu.is_none(), "the HUD has no menu in play");
    }

    #[test]
    fn a_world_snapshot_outside_the_attempt_does_not_start_the_game() {
        let mut screen = MenuScreen::new();

        // connect_succeeded only acts from the loading screen: a snapshot
        // landing in the main menu or pause must not yank the player into
        // play.
        screen.connect_succeeded();
        assert_eq!(screen.screen(), Screen::MainMenu);
    }

    #[test]
    fn back_on_the_loading_screen_cancels_the_attempt() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play -> Connecting

        // Escape maps to Back in every menu, the loading screen included.
        assert_eq!(
            screen.handle(MenuInput::Back),
            Some(MenuCommand::CancelConnect)
        );
        assert_eq!(screen.screen(), Screen::MainMenu);
        assert!(screen.status_line().is_empty(), "the loading line is gone");
    }

    #[test]
    fn a_failed_attempt_reads_on_the_loading_screen_until_cancel() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play -> Connecting
        screen.set_status_line("Could not connect: no answer");

        let (menu, _) = compiled(&screen);
        assert_eq!(
            menu.items[LOADING_STATUS].text,
            "Could not connect: no answer"
        );

        // Focus starts on Cancel, so Enter abandons the attempt.
        assert_eq!(screen.focus(), LOADING_CANCEL);
        assert_eq!(
            screen.handle(MenuInput::Activate),
            Some(MenuCommand::CancelConnect)
        );
        assert_eq!(screen.screen(), Screen::MainMenu);
    }

    #[test]
    fn a_connection_lost_from_play_reuses_the_loading_screen() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate);
        screen.connect_succeeded();
        screen.open_pause();
        assert_eq!(screen.screen(), Screen::Paused);

        // The bridge died in play: the owner reopens the loading screen
        // with the reason, and Cancel goes to the main menu as usual.
        screen.open_connecting("Connection lost: timed out");
        assert_eq!(screen.screen(), Screen::Connecting);
        assert!((screen.progress() - 0.0).abs() < 1e-6, "a fresh attempt restarts the bar");
        assert_eq!(
            screen.handle(MenuInput::Back),
            Some(MenuCommand::CancelConnect)
        );
        assert_eq!(screen.screen(), Screen::MainMenu);
    }

    #[test]
    fn the_status_line_is_truncated_to_one_row() {
        let long = "x".repeat(120);
        let truncated = truncate_status_line(&long);
        assert_eq!(truncated.chars().count(), 96);
        assert!(truncated.ends_with("..."));
        assert_eq!(truncate_status_line("short"), "short");
    }

    #[test]
    fn escape_pauses_and_resumes() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play -> Connecting
        screen.connect_succeeded(); // the server said yes

        screen.open_pause();
        assert_eq!(screen.screen(), Screen::Paused);

        // The first pause item is Resume, and Escape also resumes.
        assert_eq!(screen.handle(MenuInput::Back), Some(MenuCommand::Resume));
        assert_eq!(screen.screen(), Screen::Playing);
    }

    #[test]
    fn settings_returns_to_the_screen_it_came_from() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Down); // Settings
        assert_eq!(
            screen.handle(MenuInput::Activate),
            Some(MenuCommand::OpenSettings)
        );
        assert_eq!(screen.screen(), Screen::Settings);
        assert_eq!(screen.handle(MenuInput::Back), Some(MenuCommand::Back));
        assert_eq!(screen.screen(), Screen::MainMenu);

        // The same Settings item is reachable from pause, and Back must go
        // back there rather than to the main menu.
        screen.handle(MenuInput::Activate); // Play -> Connecting
        screen.connect_succeeded(); // the server said yes
        screen.open_pause();
        screen.handle(MenuInput::Down); // Settings
        screen.handle(MenuInput::Activate);
        assert_eq!(screen.screen(), Screen::Settings);
        screen.handle(MenuInput::Back);
        assert_eq!(screen.screen(), Screen::Paused);
    }

    #[test]
    fn the_wave_slider_moves_in_steps_and_clamps() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Down);
        screen.handle(MenuInput::Activate); // Settings
        assert_eq!(screen.focus(), 0);

        let changed = screen.handle(MenuInput::Right).expect("slider moved");
        match changed {
            MenuCommand::WaveHeightChanged(value) => {
                assert!((value - 0.55).abs() < 1e-6, "got {value}");
            }
            other => panic!("expected a wave-height command, got {other:?}"),
        }

        for _ in 0..40 {
            screen.handle(MenuInput::Right);
        }
        assert!((screen.wave_height() - 1.0).abs() < 1e-6);

        for _ in 0..40 {
            screen.handle(MenuInput::Left);
        }
        assert!(screen.wave_height().abs() < 1e-6);
    }

    #[test]
    fn left_and_right_do_nothing_on_a_button() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Down);
        screen.handle(MenuInput::Activate); // Settings
        screen.handle(MenuInput::Down); // Back
        assert_eq!(screen.handle(MenuInput::Right), None);
    }

    #[test]
    fn a_click_selects_the_button_under_the_pointer() {
        let mut screen = MenuScreen::new();
        let mut model = UiModel::new();
        screen.sync(&mut model);
        let settings = model.menu.unwrap().items[1].position;

        assert_eq!(
            screen.handle(MenuInput::PointerPressed(settings + Vec2::new(4.0, 4.0))),
            Some(MenuCommand::OpenSettings)
        );
        assert_eq!(screen.screen(), Screen::Settings);
    }

    #[test]
    fn a_click_outside_every_item_changes_nothing() {
        let mut screen = MenuScreen::new();
        assert_eq!(
            screen.handle(MenuInput::PointerPressed(Vec2::new(2.0, 2.0))),
            None
        );
        assert_eq!(screen.screen(), Screen::MainMenu);
    }

    #[test]
    fn the_panel_is_recentred_on_resize() {
        let mut screen = MenuScreen::new();
        screen.set_screen_size(1920.0, 1080.0);
        let mut model = UiModel::new();
        screen.sync(&mut model);
        let menu = model.menu.unwrap();
        let expected_x = (1920.0 - PANEL_WIDTH) * 0.5;
        assert!((menu.position.x - expected_x).abs() < 1e-4);
        // The button stays inside the panel at its new place.
        assert!((menu.items[0].position.x - (expected_x + PANEL_SIDE_PAD)).abs() < 1e-4);
    }
}
