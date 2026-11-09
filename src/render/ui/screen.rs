//! Menu screens: the state machine between the main menu, settings, pause
//! and play, plus the layout that makes the panel usable.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
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

/// Which screen the player is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// The first screen after start-up.
    MainMenu,
    /// Opened from the main menu or from pause.
    Settings,
    /// The game is running but input belongs to the pause menu.
    Paused,
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
/// outside it (quit the loop, move the camera, change the water). Keeping the
/// two apart means the state machine stays testable without a window.
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
}

/// The menu state machine.
pub struct MenuScreen {
    screen: Screen,
    /// Where Settings returns to: the main menu or the pause menu.
    settings_return: Screen,
    screen_size: Vec2,
    wave_height: f32,
    focus: usize,
}

impl MenuScreen {
    pub fn new() -> Self {
        Self {
            screen: Screen::MainMenu,
            settings_return: Screen::MainMenu,
            // Replaced by the presentation extent before the first frame.
            screen_size: Vec2::new(1280.0, 720.0),
            wave_height: 0.5,
            focus: 0,
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

        let panel_position = self.panel_origin();
        let mut menu = Menu::new(self.menu_type());
        menu.title = self.title().to_string();
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
            Screen::Playing => MenuType::InGame,
        }
    }

    fn title(&self) -> &'static str {
        match self.screen {
            Screen::MainMenu => "RFS",
            Screen::Settings => "Settings",
            Screen::Paused => "Paused",
            Screen::Playing => "",
        }
    }

    fn item_count(&self) -> usize {
        match self.screen {
            Screen::MainMenu => 3,
            Screen::Settings => 2,
            Screen::Paused => 4,
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

    fn item_at(&self, point: Vec2) -> Option<usize> {
        (0..self.item_count()).find(|&index| {
            let at = self.item_origin(index);
            point.x >= at.x
                && point.x <= at.x + BUTTON_WIDTH
                && point.y >= at.y
                && point.y <= at.y + BUTTON_HEIGHT
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
            Screen::Playing => Vec::new(),
        }
    }

    // ---------------------------------------------------------- behaviour

    fn move_focus(&mut self, delta: isize) {
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
                0 => {
                    self.screen = Screen::Playing;
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
                2 => {
                    self.screen = Screen::MainMenu;
                    self.settings_return = Screen::MainMenu;
                    self.focus = 0;
                    Some(MenuCommand::QuitToMenu)
                }
                3 => Some(MenuCommand::Quit),
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
            Screen::MainMenu => Some(MenuCommand::Quit),
            Screen::Playing => None,
        }
    }
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
    fn play_moves_to_the_game_and_hides_the_menu() {
        let mut screen = MenuScreen::new();

        assert_eq!(
            screen.handle(MenuInput::Activate),
            Some(MenuCommand::StartGame)
        );
        assert_eq!(screen.screen(), Screen::Playing);

        let mut model = UiModel::new();
        screen.sync(&mut model);
        assert!(model.menu.is_none(), "the HUD has no menu in play");
    }

    #[test]
    fn escape_pauses_and_resumes() {
        let mut screen = MenuScreen::new();
        screen.handle(MenuInput::Activate); // Play

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
        screen.handle(MenuInput::Activate); // Play
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
