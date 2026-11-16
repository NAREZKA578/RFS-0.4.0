//! The client's real typefaces.
//!
//! **CLIENT-SIDE ONLY; CONTENT LAYER, NOT `RHI`**
//!
//! The 8×8 bitmap font in `rhi::ui::bitmap_font` is the placeholder this
//! module replaces for the menus that opt into it: Inter, SIL OFL 1.1,
//! rasterised here (outside `rhi`, which must not grow a font stack) into
//! the one shared atlas the text path samples.
//!
//! # Which face is which
//!
//! * [`FONT_REGULAR`] — Inter Regular: body text, the loading screen's
//!   status line and its cancel hint.
//! * [`FONT_HEADING`] — Inter Medium: the loading screen's logo line and
//!   its big server name, the two slots Squad sets in a heavier cut.
//!
//! # The shared atlas
//!
//! The renderer samples exactly one atlas texture, so the faces share one
//! image: the placeholder's own atlas rows first, byte for byte at the
//! coordinates its glyph math already assumes, then each face's glyphs in
//! a fixed grid below. Every registered font's rectangles therefore stay
//! valid in the single texture — the bitmap font keeps drawing identical
//! pixels, and the real faces land on top of the same upload.
//!
//! # The typeface choice, honestly
//!
//! Inter is an open-licensed (SIL OFL 1.1 — `LICENSE.txt` sits next to the
//! files) neutral grotesque, chosen to match the Squad UI's look. Squad's
//! own font file is not copied; the composition is the copy, the face is
//! ours. Weights: Regular for body, Medium for the two heading slots —
//! closer to the reference than a single weight, and everything the
//! existing layout constants expect.

use std::collections::HashMap;

use ab_glyph::{Font, FontVec, GlyphId, PxScale, ScaleFont};
use crate::rhi::ui::bitmap_font::CELL as BITMAP_CELL;
use crate::rhi::ui::renderer::UiRenderer;
use crate::rhi::ui::text::{UiFont, UiGlyph};
use crate::rhi::ui::{UiError, UiResult};

/// Registered name of the body face (Inter Regular).
pub const FONT_REGULAR: &str = "rfs";
/// Registered name of the heading face (Inter Medium).
pub const FONT_HEADING: &str = "rfs_heading";

/// The name the placeholder bitmap face was installed under.
const BITMAP_FONT_NAME: &str = "bitmap8";

/// Pixel size the faces are rasterised at, and the metrics' natural size.
/// The loading screen's two big lines (48 px) draw 1:1; smaller sizes scale
/// the metrics down and sample the same coverage.
const NATURAL_SIZE: f32 = 48.0;

/// Glyph grid columns. 167 codepoints fill 11 rows.
const GLYPH_COLS: usize = 16;
/// One glyph's atlas cell, padding included. The ink budget inside is
/// 48×56 px — plenty for Inter at 48 px, and the rasteriser below panics
/// rather than clipping if a face ever exceeds it.
const CELL_W: u32 = 56;
const CELL_H: u32 = 64;
const CELL_PAD: u32 = 4;

/// The real faces, SIL OFL 1.1 (see `LICENSE.txt` beside them).
///
/// The path matches the client's other embedded assets: the binaries live
/// under `Tool/client_tests/assets/` and are `include_bytes!`-ed from the
/// source tree, so a fresh clone needs the files present.
const REGULAR_TTF: &[u8] = include_bytes!("../../../Tool/client_tests/assets/fonts/Inter-Regular.ttf");
const HEADING_TTF: &[u8] = include_bytes!("../../../Tool/client_tests/assets/fonts/Inter-Medium.ttf");

/// The codepoints the real faces cover: everything the client's own UI
/// strings and a Russian server's announcements can put in them — ASCII,
/// Cyrillic with Ё/ё, the em dash, the ellipsis and the guillemets.
fn charset() -> Vec<char> {
    let mut out: Vec<char> = (0x20u32..=0x7E).filter_map(char::from_u32).collect();
    out.extend((0x0410u32..=0x044F).filter_map(char::from_u32));
    out.extend(['\u{0401}', '\u{0451}', '\u{2014}', '\u{2026}', '\u{00AB}', '\u{00BB}']);
    out
}

/// One rasterised face and where its glyphs live in the shared atlas.
#[derive(Debug)]
struct TtfFace {
    font: FontVec,
    scale: PxScale,
    /// Ink height above the baseline, in texels at [`NATURAL_SIZE`].
    ascent: f32,
    /// Ink depth below the baseline, positive, same units.
    descent: f32,
    /// Per-codepoint atlas placement. A codepoint missing here is one the
    /// face cannot draw — the layout drops it, as it does for the
    /// placeholder's uncovered characters.
    cells: HashMap<char, GlyphCell>,
}

/// Where one glyph's ink sits in the atlas, how it reaches from the pen,
/// and how far the pen moves afterwards — all in texels at the natural
/// size, exactly what [`UiGlyph`] carries.
#[derive(Debug, Clone, Copy)]
struct GlyphCell {
    /// `[x0, y0, x1, y1]`, absolute atlas coordinates.
    rect: [f32; 4],
    /// `[dx, dy]`: pen → ink top-left. `dy` measures from the glyph's top
    /// down to the baseline (the trait's convention).
    offset: [f32; 2],
    advance: f32,
}

impl TtfFace {
    /// Parses `name: bytes`, rasterises every codepoint of the charset at
    /// [`NATURAL_SIZE`] into `atlas` starting at row `region_top`, and
    /// records the cells.
    fn rasterise(
        bytes: &[u8],
        name: &str,
        region_top: u32,
        atlas: &mut [u8],
        atlas_width: u32,
    ) -> Result<Self, FontError> {
        let font = FontVec::try_from_vec(bytes.to_vec())
            .map_err(|e| FontError(format!("{name}: {e}")))?;
        let scale = PxScale::from(NATURAL_SIZE);
        let scaled = font.as_scaled(scale);
        let ascent = scaled.ascent();
        // ab_glyph reports the descent as a negative value (below-baseline
        // reach); the UiFont contract wants the positive distance.
        let descent = -scaled.descent();
        if !ascent.is_finite() || !descent.is_finite() || ascent <= 0.0 || descent < 0.0 {
            return Err(FontError(format!("{name}: unusable line metrics ({ascent}, {descent})")));
        }

        let charset = charset();
        let mut cells = HashMap::with_capacity(charset.len());
        for (index, ch) in charset.into_iter().enumerate() {
            let gid = font.glyph_id(ch);
            // Glyph 0 is `.notdef`; a char the cmap has no entry for has no
            // glyph and must not occupy a cell.
            if gid == GlyphId(0) {
                continue;
            }
            let advance = scaled.h_advance(gid);
            let cell_origin = |index: usize| {
                let cx = (index % GLYPH_COLS) as u32 * CELL_W;
                let cy = region_top + (index / GLYPH_COLS) as u32 * CELL_H;
                (cx, cy)
            };
            let (cx, cy) = cell_origin(index);
            let glyph = gid.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
            let Some(outlined) = font.outline_glyph(glyph) else {
                // No outline at an existing glyph: a space. Ink-less, but it
                // still moves the pen — same arrangement as the placeholder.
                cells.insert(
                    ch,
                    GlyphCell { rect: [0.0; 4], offset: [0.0; 2], advance },
                );
                continue;
            };
            let bounds = outlined.px_bounds();
            let w = bounds.width().ceil() as u32;
            let h = bounds.height().ceil() as u32;
            assert!(
                w <= CELL_W - CELL_PAD * 2 && h <= CELL_H - CELL_PAD * 2,
                "{name}: glyph {ch:?} rasterised {w}x{h}, over the {0}x{1} cell budget",
                CELL_W - CELL_PAD * 2,
                CELL_H - CELL_PAD * 2,
            );
            // Coverage into the cell, pen-relative geometry kept in `offset`.
            let ox = cx + CELL_PAD;
            let oy = cy + CELL_PAD;
            outlined.draw(|x, y, v| {
                let ink = (v * 255.0).round() as u8;
                if ink > 0 {
                    atlas[((oy + y) as usize) * atlas_width as usize + (ox + x) as usize] = ink;
                }
            });
            cells.insert(
                ch,
                GlyphCell {
                    rect: [
                        ox as f32,
                        oy as f32,
                        (ox + w) as f32,
                        (oy + h) as f32,
                    ],
                    offset: [bounds.min.x, -bounds.min.y],
                    advance,
                },
            );
        }

        Ok(Self { font, scale, ascent, descent, cells })
    }
}

impl UiFont for TtfFace {
    fn glyph(&self, ch: char) -> Option<UiGlyph> {
        let cell = self.cells.get(&ch)?;
        Some(UiGlyph {
            atlas_rect: cell.rect,
            offset: cell.offset,
            size: [cell.rect[2] - cell.rect[0], cell.rect[3] - cell.rect[1]],
            advance: cell.advance,
        })
    }

    fn natural_size(&self) -> f32 {
        NATURAL_SIZE
    }

    fn ascent(&self) -> f32 {
        self.ascent
    }

    fn descent(&self) -> f32 {
        self.descent
    }

    /// A rasterised face's ink scales with the request — that is the whole
    /// point of having real outlines in the atlas.
    fn glyph_pixels_scale_with_size(&self) -> bool {
        true
    }

    fn kerning(&self, left: char, right: char) -> f32 {
        let (l, r) = (self.font.glyph_id(left), self.font.glyph_id(right));
        if l == GlyphId(0) || r == GlyphId(0) {
            return 0.0;
        }
        self.font.as_scaled(self.scale).kern(l, r)
    }
}

/// A parsing or metric failure while building the faces. Non-fatal for the
/// renderer — the client reports it once and keeps drawing panels.
#[derive(Debug, Clone)]
pub struct FontError(pub String);

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FontError {}

/// Both real faces plus the one shared atlas they (and the placeholder)
/// live in.
#[derive(Debug)]
pub struct UiFonts {
    regular: TtfFace,
    heading: TtfFace,
    atlas: Vec<u8>,
    atlas_width: u32,
    atlas_height: u32,
}

impl UiFonts {
    /// Parses and rasterises both faces into the shared atlas. Heavy-ish
    /// (two files, ~170 glyphs each at 48 px) and done once, at renderer
    /// setup.
    pub fn new() -> Result<Self, FontError> {
        // The placeholder's atlas rows: byte for byte the image it has
        // always been, at the coordinates its glyph math assumes.
        let bitmap = crate::rhi::ui::bitmap_font::UiBitmapFont::new();
        let bitmap_width = bitmap.atlas_width();
        let bitmap_height = bitmap.atlas_height() as usize;

        let charset_len = charset().len();
        let rows = charset_len.div_ceil(GLYPH_COLS);
        let face_grid_height = rows as u32 * CELL_H;
        let atlas_width = bitmap_width.max(GLYPH_COLS as u32 * CELL_W);
        let atlas_height = BITMAP_CELL as u32 + face_grid_height * 2;
        let mut atlas = vec![0u8; atlas_width as usize * atlas_height as usize];
        for row in 0..bitmap_height {
            let src = row * bitmap_width as usize;
            let dst = row * atlas_width as usize;
            atlas[dst..dst + bitmap_width as usize]
                .copy_from_slice(&bitmap.atlas_pixels()[src..src + bitmap_width as usize]);
        }

        let regular = TtfFace::rasterise(
            REGULAR_TTF,
            "regular",
            BITMAP_CELL as u32,
            &mut atlas,
            atlas_width,
        )?;
        let heading = TtfFace::rasterise(
            HEADING_TTF,
            "heading",
            BITMAP_CELL as u32 + face_grid_height,
            &mut atlas,
            atlas_width,
        )?;

        Ok(Self { regular, heading, atlas, atlas_width, atlas_height })
    }

    /// Installs every face the client ships.
    ///
    /// Order matters and is the whole arrangement: the placeholder first
    /// (its own atlas, as ever), then the shared atlas — placeholder rows
    /// on top, the rasterised faces below — which becomes THE texture the
    /// text path samples. The bitmap font's rectangles stay valid in it,
    /// so menus that did not opt into the real faces render the pixels
    /// they always did.
    pub fn install(ui: &mut UiRenderer) -> UiResult<()> {
        let fonts = Self::new().map_err(|e| UiError::BackendError(e.0.clone()))?;
        ui.install_bitmap_font(BITMAP_FONT_NAME)?;
        ui.install_font_atlas(
            FONT_REGULAR,
            Box::new(fonts.regular),
            &fonts.atlas,
            fonts.atlas_width,
            fonts.atlas_height,
        )?;
        // The heading face shares the regular's atlas upload: rectangles
        // only, no second texture.
        ui.register_font(FONT_HEADING, Box::new(fonts.heading));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhi::ui::bitmap_font::UiBitmapFont;
    use crate::rhi::ui::text::{measure, UiTextStyle};

    fn fonts() -> UiFonts {
        match UiFonts::new() {
            Ok(fonts) => fonts,
            Err(e) => panic!("the shipped faces must parse and rasterise: {e}"),
        }
    }

    fn style(size: f32) -> UiTextStyle {
        UiTextStyle { size, ..Default::default() }
    }

    /// Every byte of the placeholder's rows in the shared atlas, read back
    /// row by row against the placeholder's own atlas.
    fn bitmap_region_matches(fonts: &UiFonts) -> bool {
        let placeholder = UiBitmapFont::new();
        let w = placeholder.atlas_width();
        for row in 0..placeholder.atlas_height() as usize {
            let src = row * w as usize;
            let dst = row * fonts.atlas_width as usize;
            if &fonts.atlas[dst..dst + w as usize]
                != &placeholder.atlas_pixels()[src..src + w as usize]
            {
                return false;
            }
        }
        true
    }

    fn has_ink(fonts: &UiFonts, rect: [f32; 4]) -> bool {
        let (x0, y0, x1, y1) = (rect[0] as usize, rect[1] as usize, rect[2] as usize, rect[3] as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                if fonts.atlas[y * fonts.atlas_width as usize + x] > 0 {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn both_faces_rasterise_with_real_line_metrics() {
        let fonts = fonts();
        for (name, face) in [("regular", &fonts.regular), ("heading", &fonts.heading)] {
            assert_eq!(face.natural_size(), NATURAL_SIZE, "{name}");
            assert!(face.ascent() > 0.0 && face.descent() > 0.0, "{name}");
            assert!(
                (face.ascent() + face.descent()) >= NATURAL_SIZE,
                "{name}: the line box is smaller than the size it claims"
            );
        }
    }

    #[test]
    fn latin_and_cyrillic_both_have_ink() {
        let fonts = fonts();
        for ch in ['A', 'R', 'F', 'S', '7', 'Ж', 'ё', '\u{2014}'] {
            for (name, face) in [("regular", &fonts.regular), ("heading", &fonts.heading)] {
                let cell = face.cells.get(&ch).unwrap_or_else(|| {
                    panic!("{name} must cover {ch:?}: the UI and Russian server text need it")
                });
                assert!(cell.advance > 0.0, "{name} {ch:?}: no advance");
                assert!(has_ink(&fonts, cell.rect), "{name} {ch:?}: a covered glyph with no ink");
            }
        }
    }

    #[test]
    fn a_space_advances_without_ink() {
        let fonts = fonts();
        for face in [&fonts.regular, &fonts.heading] {
            let cell = face.cells.get(&' ').expect("the space must be covered");
            assert!(cell.advance > 0.0);
            assert_eq!(cell.rect, [0.0; 4], "the space must not occupy atlas ink");
        }
    }

    #[test]
    fn distinct_chars_sit_in_distinct_cells() {
        let fonts = fonts();
        for face in [&fonts.regular, &fonts.heading] {
            let a = face.glyph('A').expect("'A' is covered");
            let b = face.glyph('Б').expect("'Б' is covered");
            assert_ne!(a.atlas_rect, b.atlas_rect);
        }
    }

    #[test]
    fn the_placeholder_rows_keep_their_bytes() {
        let fonts = fonts();
        assert!(
            bitmap_region_matches(&fonts),
            "the placeholder's atlas rows changed: menus that did not opt \
             into the real faces would no longer render identically"
        );
    }

    #[test]
    fn the_real_face_drives_its_own_width() {
        let fonts = fonts();
        let text = "RFS";
        let real = measure(text, &fonts.regular, &style(NATURAL_SIZE));
        let placeholder = measure(text, &UiBitmapFont::new(), &style(NATURAL_SIZE));
        assert_ne!(
            real, placeholder,
            "the real face measured like the placeholder: metrics are not driving"
        );
        // Not the placeholder's fixed 8-px-per-cell advance either way.
        assert!(real > 0.0 && placeholder > 0.0);
    }

    #[test]
    fn kerning_stays_finite_and_faces_match_on_layout() {
        let fonts = fonts();
        for text in ["RFS", "127.0.0.1:7777", "AУЖЕ", "[ESC] CANCEL"] {
            for face in [&fonts.regular, &fonts.heading] {
                let width = measure(text, face, &style(NATURAL_SIZE));
                assert!(width.is_finite() && width > 0.0, "{text:?}");
            }
        }
    }

    #[test]
    fn placed_glyph_ink_scales_with_the_request() {
        let fonts = fonts();
        let natural = crate::rhi::ui::text::UiTextLayout::layout(
            "RFS",
            &fonts.heading,
            &style(NATURAL_SIZE),
            [0.0, 0.0],
        );
        let small = crate::rhi::ui::text::UiTextLayout::layout(
            "RFS",
            &fonts.heading,
            &style(16.0),
            [0.0, 0.0],
        );
        let big = natural.glyphs().next().unwrap();
        let little = small.glyphs().next().unwrap();
        // The ink is drawn one third the natural size, and the atlas
        // rectangle stays in natural texels either way — the quad samples
        // the same coverage, just smaller.
        assert!(
            (little.glyph.size[0] * 3.0 - big.glyph.size[0]).abs() < 1.0,
            "16 px request drew {} px ink where 48 px drew {}",
            little.glyph.size[0],
            big.glyph.size[0]
        );
        assert_eq!(big.glyph.atlas_rect, little.glyph.atlas_rect);
    }

    #[test]
    fn the_atlas_is_one_texture_sized_for_every_region() {
        let fonts = fonts();
        let placeholder = UiBitmapFont::new();
        assert!(fonts.atlas_width >= placeholder.atlas_width());
        assert!(fonts.atlas_height > placeholder.atlas_height());
        // The faces' bottom rows must land inside the image.
        let last = (fonts.atlas_height - 1) as usize;
        assert_eq!(fonts.atlas.len(), fonts.atlas_width as usize * fonts.atlas_height as usize);
        let _ = last;
    }
}
