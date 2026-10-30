//! `UiModel` — the UI's data model, compiled into `UiCommand`s for the pass.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Track item №724. Before this file the UI was half-dead in the ways the
//! defect registry knows: `HUD::render`/`Menu::render`/`Minimap::render` were
//! stubs with zero callers, and `UIPass` drew only its debug marker. The model
//! here is the single producer of the commands the pass queues per frame:
//!
//!  - the pass owns an `Arc<Mutex<UiModel>>` (see `UIPass::with_model`) and
//!    compiles it per frame; the lock is the frame's read snapshot, and the
//!    pass drops it before recording, so nothing holds it across a GPU call;
//!  - gating lives in the pass (`UIConfig::enable_*` flags pick which parts of
//!    the model run), compiling lives in the models — the model never needs the
//!    frame's config;
//!  - commands are pure `rhi::ui::UiCommand` values; nothing here touches the
//!    GPU, so the whole compile path is testable on the CPU against an exact
//!    command list;
//!  - textures travel separately (`UiBatch::push_texture`): the renderer keys
//!    images by a `u64`, and the id is the `Arc`'s address — stable for the
//!    texture's lifetime, and no second registry to keep in sync.

use super::hud::HUD;
use super::menu::Menu;
use super::minimap::Minimap;
use crate::rhi::Texture as RhiTexture;
use crate::render::textures::Texture;
use crate::rhi::ui::UiCommand;
use glam::Vec4;
use std::sync::Arc;

/// The colour layout `UiCommand` wants: an RGBA array, in that order.
///
/// The models carry `glam::Vec4`; every compile site would otherwise spell out
/// the same field-by-field conversion, so the conversion lives once, here.
pub(crate) fn rgba(color: Vec4) -> [f32; 4] {
    [color.x, color.y, color.z, color.w]
}

/// Compiled UI events: the commands the pass queues, plus the textures those
/// commands refer to.
///
/// The split exists because [`UiCommand::Image`] names a texture by `u64` while
/// the registration in `UiRenderer::add_texture` wants an owned `Arc` — one
/// return type carrying both keeps an image command and its backing texture
/// from ever diverging.
#[derive(Debug, Default)]
pub struct UiBatch {
    pub commands: Vec<UiCommand>,
    pub textures: Vec<(u64, Arc<RhiTexture>)>,
}

impl UiBatch {
    /// Registers `texture` as an image the renderer can draw and returns its
    /// id.
    ///
    /// The id is the `Arc`'s address: unique for as long as that `Arc` lives,
    /// so one texture instance always maps to one id without an extra table.
    /// Registering the same `Arc` twice yields the same id twice — harmless,
    /// the renderer keys its cache by exactly this value.
    pub fn push_texture(&mut self, texture: &Arc<Texture>) -> u64 {
        let id = Arc::as_ptr(texture) as u64;
        self.textures.push((id, texture.rhi_texture().clone()));
        id
    }
}

/// Which parts of the model compile. Mirrors `passes::ui::UIConfig`'s
/// `enable_*` flags one-to-one: gating lives in the pass, compiling lives in
/// the model, and this type is the seam between the two — the model never
/// needs to know the config exists, and the pass compiles only the parts it
/// is asked to draw.
///
/// Default means "everything on", which is what a model compiled on its own
/// (a test, a preview) should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiGates {
    pub hud: bool,
    pub menu: bool,
    pub minimap: bool,
}

impl Default for UiGates {
    fn default() -> Self {
        Self {
            hud: true,
            menu: true,
            minimap: true,
        }
    }
}

/// Everything the UI draws this frame, as plain data.
///
/// `menu` and `minimap` are options because their absence is a state, not a
/// default — the main menu is on, the pause menu is not, and a minimap exists
/// only in-scene. The HUD is always there; if it has no elements its own
/// compile is empty and costs nothing.
pub struct UiModel {
    pub hud: HUD,
    pub menu: Option<Menu>,
    pub minimap: Option<Minimap>,
}

impl UiModel {
    pub fn new() -> Self {
        Self {
            hud: HUD::default(),
            menu: None,
            minimap: None,
        }
    }

    /// Publishes the logical screen size to the HUD, which is the only part
    /// that resolves elements through alignment math. Menus and the minimap
    /// are positioned in absolute pixels by their owners.
    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.hud.set_screen_size(width, height);
    }

    /// Compiles the whole model into one batch, every part on. Order within
    /// the batch is the read order below: HUD, menu, minimap — bottom to top
    /// by design, since the menu and the overlays must win over the HUD if
    /// they overlap.
    pub fn compile(&self) -> UiBatch {
        let mut batch = UiBatch::default();
        self.compile_into(&mut batch, UiGates::default());
        batch
    }

    /// Compiles only the gated parts of the model into `batch`, appending to
    /// whatever is already in it. This is the form the pass uses: it decides
    /// from its config which parts exist this frame and calls this once.
    pub fn compile_into(&self, batch: &mut UiBatch, gates: UiGates) {
        if gates.hud {
            self.hud.to_commands(batch);
        }
        if gates.menu {
            if let Some(menu) = &self.menu {
                menu.to_commands(batch);
            }
        }
        if gates.minimap {
            if let Some(minimap) = &self.minimap {
                minimap.to_commands(batch);
            }
        }
    }
}

impl Default for UiModel {
    fn default() -> Self {
        Self::new()
    }
}
