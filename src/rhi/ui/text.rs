//! Text Layout
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Turns a string plus a [`UiFont`] into positioned glyph quads.
//!
//! # What lives here and what does not
//!
//! This module decides **where each glyph goes** — advances, kerning, alignment,
//! line breaking, baseline offsets. That is geometry, so it belongs next to the
//! geometry that consumes it.
//!
//! Parsing a TTF and rasterising outlines into an atlas does **not** belong
//! here. `rhi` is a hardware abstraction with no knowledge of content or assets,
//! and growing it a font stack would make every backend carry one. Rasterisation
//! is supplied from outside through [`UiFont`]; this file never sees a font
//! file.
//!
//! # Coordinate conventions
//!
//! Layout is in pixels with Y growing downwards, matching [`super::UiVertex`]'s
//! screen space and the projection in [`super::pass`]. Glyph offsets are
//! therefore measured from the *top* of the line downwards, and a glyph's
//! `offset` moves it from the pen position to where its pixels belong.

use super::UiAlignment;

/// Where one glyph sits in an atlas and how it advances the pen.
///
/// All rectangles and offsets are in **texels**, not UVs: the atlas dimensions
/// are needed to turn them into UVs, and only the code that uploads the atlas
/// knows those.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiGlyph {
    /// Top-left of the glyph inside the atlas.
    pub atlas_rect: [f32; 4],
    /// Offset from the pen to the top-left of the drawn pixels.
    ///
    /// Negative on the left is normal: a `j` hangs left of the pen, and some
    /// faces have a negative left side bearing.
    pub offset: [f32; 2],
    /// Size of the drawn pixels.
    pub size: [f32; 2],
    /// How far the pen moves after drawing this glyph.
    pub advance: f32,
}

/// A glyph with its final on-screen position resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiPlacedGlyph {
    pub ch: char,
    /// Top-left of the quad, in screen pixels.
    pub position: [f32; 2],
    /// The glyph this came from, for UV computation at upload time.
    pub glyph: UiGlyph,
}

impl UiPlacedGlyph {
    /// The quad this glyph occupies, in screen pixels.
    pub fn rect(&self) -> [f32; 4] {
        [
            self.position[0],
            self.position[1],
            self.position[0] + self.glyph.size[0],
            self.position[1] + self.glyph.size[1],
        ]
    }
}

/// Supplies glyph metrics.
///
/// Implemented by whatever rasterises fonts: a TrueType backend, a bitmap font
/// reader, or a test double. Every method is in texels at the font's
/// [`natural_size`](UiFont::natural_size).
pub trait UiFont {
    /// Metrics for `ch`, or `None` if the font cannot draw it.
    fn glyph(&self, ch: char) -> Option<UiGlyph>;

    /// Pixel size the metrics above are expressed at.
    fn natural_size(&self) -> f32;

    /// Baseline distance from the top of a line.
    fn ascent(&self) -> f32;

    /// Distance from the baseline to the bottom of a line.
    fn descent(&self) -> f32;

    /// Whether the font can draw `ch`.
    fn has_glyph(&self, ch: char) -> bool {
        self.glyph(ch).is_some()
    }

    /// Extra spacing inserted between `left` and `right`, in texels.
    ///
    /// Negative is a tightening pair. Returning zero is a valid, common answer:
    /// a font with no kerning table is not an error.
    fn kerning(&self, _left: char, _right: char) -> f32 {
        0.0
    }

    /// Glyph substituted for `ch` when the font has none.
    ///
    /// Returning `None` means the character is dropped entirely.
    fn fallback_glyph(&self, _ch: char) -> Option<UiGlyph> {
        None
    }
}

/// How a run of text is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct UiTextStyle {
    /// Requested pixel size. Metrics are scaled from the font's natural size.
    pub size: f32,
    pub color: [f32; 4],
    /// Horizontal alignment within the available width.
    pub align: UiAlignment,
    /// Width available for text. `None` means unbounded: one line, left aligned.
    ///
    /// Lines longer than this are broken; see [`UiTextLayout::layout`].
    pub max_width: Option<f32>,
    /// Extra space between lines, as a multiple of the line height.
    pub line_spacing: f32,
}

impl Default for UiTextStyle {
    fn default() -> Self {
        Self {
            size: 16.0,
            color: [1.0, 1.0, 1.0, 1.0],
            align: UiAlignment::TopLeft,
            max_width: None,
            line_spacing: 0.0,
        }
    }
}

/// Line height for `style` at `font`, in pixels.
pub fn line_height(font: &dyn UiFont, style: &UiTextStyle) -> f32 {
    let scale = scale_for(font, style.size);
    (font.ascent() + font.descent()) * scale * (1.0 + style.line_spacing)
}

/// The factor glyph metrics are multiplied by to reach `size`.
fn scale_for(font: &dyn UiFont, size: f32) -> f32 {
    let natural = font.natural_size();
    // A font that reports a zero natural size would divide by zero and put
    // infinities into every position. Treated as unscaled instead.
    if natural.is_finite() && natural > 0.0 && size.is_finite() && size > 0.0 {
        size / natural
    } else {
        1.0
    }
}

/// Width of `text` on one line, in pixels.
pub fn measure(text: &str, font: &dyn UiFont, style: &UiTextStyle) -> f32 {
    let scale = scale_for(font, style.size);
    let mut width = 0.0;
    let mut previous: Option<char> = None;
    for ch in text.chars() {
        if let Some(prev) = previous {
            width += font.kerning(prev, ch) * scale;
        }
        let Some(glyph) = font.glyph(ch).or_else(|| font.fallback_glyph(ch)) else {
            continue;
        };
        width += glyph.advance * scale;
        previous = Some(ch);
    }
    width
}

/// One laid-out line of text.
#[derive(Debug, Clone, PartialEq)]
pub struct UiTextLine {
    /// The text this line came from.
    pub text: String,
    /// Top-left of the line box, in screen pixels.
    pub origin: [f32; 2],
    /// Width of the line, in pixels.
    pub width: f32,
    pub glyphs: Vec<UiPlacedGlyph>,
}

/// Lays text out into positioned glyphs.
///
/// Pure arithmetic over a [`UiFont`], so it is testable without a GPU, an atlas
/// or a font file.
#[derive(Debug, Clone, PartialEq)]
pub struct UiTextLayout {
    lines: Vec<UiTextLine>,
    line_height: f32,
    /// Width of the widest line, or the wrap width when one is set.
    pub measured_width: f32,
}

impl UiTextLayout {
    /// Lay out `text` with its top-left box at `origin`.
    ///
    /// Line breaking:
    ///
    /// * `\n` always breaks.
    /// * With [`UiTextStyle::max_width`], a line longer than the limit is
    ///   broken at spaces, greedily.
    /// * A single word longer than the limit is **not** broken and overflows.
    ///   Breaking inside a word would need per-character measurement, and a
    ///   hyphenation pass, and an overflowing word is the honest outcome until
    ///   one of those exists. It also keeps this loop from failing to terminate.
    /// * A space at a break point is consumed rather than drawn, so a wrapped
    ///   line does not start with a gap.
    pub fn layout(
        text: &str,
        font: &dyn UiFont,
        style: &UiTextStyle,
        origin: [f32; 2],
    ) -> Self {
        let scale = scale_for(font, style.size);
        let lh = line_height(font, style);
        let ascent = font.ascent() * scale;

        let raw_lines: Vec<String> = match style.max_width {
            None => text.split('\n').map(str::to_string).collect(),
            Some(limit) if !(limit.is_finite() && limit > 0.0) => {
                // A nonsensical limit would break every iteration. Unbounded is
                // the safe reading.
                text.split('\n').map(str::to_string).collect()
            }
            Some(limit) => break_lines(text, font, style, limit),
        };

        let mut lines = Vec::with_capacity(raw_lines.len());
        let mut measured_width: f32 = 0.0;

        for (i, raw) in raw_lines.iter().enumerate() {
            let trimmed = raw.trim_end_matches(' ');
            let width = measure(trimmed, font, style);
            measured_width = measured_width.max(width);

            let offset_x = match style.align {
                UiAlignment::TopLeft
                | UiAlignment::CenterLeft
                | UiAlignment::BottomLeft => 0.0,
                UiAlignment::TopCenter
                | UiAlignment::Center
                | UiAlignment::BottomCenter => {
                    let available = style.max_width.unwrap_or(width);
                    (available - width) * 0.5
                }
                UiAlignment::TopRight
                | UiAlignment::CenterRight
                | UiAlignment::BottomRight => {
                    let available = style.max_width.unwrap_or(width);
                    available - width
                }
            };

            let y = origin[1] + i as f32 * lh;
            let glyphs = place_line(
                trimmed,
                font,
                scale,
                ascent,
                [origin[0] + offset_x, y],
            );

            lines.push(UiTextLine {
                text: trimmed.to_string(),
                origin: [origin[0] + offset_x, y],
                width,
                glyphs,
            });
        }

        Self {
            lines,
            line_height: lh,
            measured_width: style
                .max_width
                .filter(|w| w.is_finite() && *w > 0.0)
                .unwrap_or(measured_width),
        }
    }

    pub fn lines(&self) -> &[UiTextLine] {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.glyphs.is_empty())
    }

    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    /// Height of the whole block, in pixels.
    pub fn height(&self) -> f32 {
        self.lines.len() as f32 * self.line_height
    }

    /// Every placed glyph across every line.
    pub fn glyphs(&self) -> impl Iterator<Item = &UiPlacedGlyph> {
        self.lines.iter().flat_map(|l| l.glyphs.iter())
    }

    /// Total glyph count, for callers sizing their own buffers.
    pub fn glyph_count(&self) -> usize {
        self.lines.iter().map(|l| l.glyphs.len()).sum()
    }
}

/// Greedy word wrap.
///
/// Splits on spaces and never breaks inside a word. A word wider than the limit
/// is emitted on its own line and overflows, which is the documented behaviour:
/// breaking mid-word needs per-character measurement plus hyphenation, and an
/// honest overflow beats a word chopped at an arbitrary letter.
///
/// Runs of spaces collapse to one, so a line is never padded by trailing
/// whitespace and a break point never leaves a leading gap.
fn break_lines(
    text: &str,
    font: &dyn UiFont,
    style: &UiTextStyle,
    limit: f32,
) -> Vec<String> {
    let mut out = Vec::new();

    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            out.push(String::new());
            continue;
        }

        // Measured once rather than per word.
        let space_width = measure(" ", font, style);
        let mut current = String::new();
        let mut current_width = 0.0f32;

        for word in paragraph.split(' ').filter(|w| !w.is_empty()) {
            let word_width = measure(word, font, style);
            // No leading space on a fresh line.
            let separator = if current.is_empty() { 0.0 } else { space_width };

            if !current.is_empty() && current_width + separator + word_width > limit {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
                current_width = word_width;
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
                current_width += separator + word_width;
            }
        }

        out.push(current);
    }

    out
}

/// Walk one line, resolving each glyph's final position.
fn place_line(
    text: &str,
    font: &dyn UiFont,
    scale: f32,
    ascent: f32,
    origin: [f32; 2],
) -> Vec<UiPlacedGlyph> {
    let mut glyphs = Vec::with_capacity(text.chars().count());
    let mut pen = origin[0];
    let mut previous: Option<char> = None;

    for ch in text.chars() {
        let Some(glyph) = font.glyph(ch).or_else(|| font.fallback_glyph(ch)) else {
            // No glyph and no fallback: nothing to draw, and no advance, so the
            // rest of the line closes up rather than leaving a hole.
            continue;
        };

        let kern = match previous {
            Some(prev) => font.kerning(prev, ch) * scale,
            None => 0.0,
        };
        pen += kern;

        let x = pen + glyph.offset[0] * scale;
        let y = origin[1] + ascent - glyph.offset[1] * scale;

        glyphs.push(UiPlacedGlyph {
            ch,
            position: [x, y],
            glyph,
        });

        pen += glyph.advance * scale;
        previous = Some(ch);
    }

    glyphs
}

/// A fixed-pitch bitmap font, used to prove the text path end to end without
/// pulling in a font rasteriser.
///
/// Every glyph is a solid block of the same size with no kerning, which is
/// enough to exercise measurement, alignment, wrapping and the atlas lookup.
#[derive(Debug, Clone)]
pub struct UiBlockFont {
    pub natural_size: f32,
    pub ascent: f32,
    pub descent: f32,
    pub advance: f32,
    /// Atlas edge, in texels.
    pub atlas_size: f32,
    /// Set of characters this font covers.
    pub covered: Vec<char>,
}

impl Default for UiBlockFont {
    fn default() -> Self {
        // A dense ASCII range, enough to exercise wrapping.
        let covered = (0x20u8..0x7F).map(char::from).collect();
        Self {
            natural_size: 16.0,
            ascent: 12.0,
            descent: 4.0,
            advance: 8.0,
            atlas_size: 256.0,
            covered,
        }
    }
}

impl UiBlockFont {
    /// Atlas cell for `ch`, if covered.
    fn cell(&self, ch: char) -> Option<f32> {
        self.covered
            .iter()
            .position(|c| *c == ch)
            .map(|i| i as f32)
    }
}

impl UiFont for UiBlockFont {
    fn glyph(&self, ch: char) -> Option<UiGlyph> {
        let cell = self.cell(ch)?;
        // A 16x16 grid over the atlas, each cell one texel.
        let cols = 16.0;
        let u = (cell % cols) / cols;
        let v = (cell / cols).floor() / cols;
        let w = 1.0 / cols;
        Some(UiGlyph {
            atlas_rect: [u, v, u + w, v + w],
            offset: [0.0, 0.0],
            size: [self.advance * 0.75, self.ascent * 0.75],
            advance: self.advance,
        })
    }

    fn natural_size(&self) -> f32 {
        self.natural_size
    }

    fn ascent(&self) -> f32 {
        self.ascent
    }

    fn descent(&self) -> f32 {
        self.descent
    }

    fn fallback_glyph(&self, _ch: char) -> Option<UiGlyph> {
        // The classic tofu box: anything uncovered still occupies space, so
        // missing characters are visible instead of silently closing up.
        Some(UiGlyph {
            atlas_rect: [0.0, 0.0, 1.0 / 16.0, 1.0 / 16.0],
            offset: [1.0, 0.0],
            size: [self.advance * 0.75, self.ascent * 0.75],
            advance: self.advance,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed-width font, no kerning. The block font's natural size is 16, so
    /// `size: 16.0` means scale 1.0 and every advance stays 8.0.
    fn font() -> UiBlockFont {
        UiBlockFont::default()
    }

    fn style() -> UiTextStyle {
        UiTextStyle::default()
    }

    #[test]
    fn an_empty_string_produces_one_empty_line() {
        let f = font();
        let layout = UiTextLayout::layout("", &f, &style(), [0.0, 0.0]);
        assert_eq!(layout.line_count(), 1);
        assert!(layout.is_empty());
        assert_eq!(layout.glyph_count(), 0);
    }

    #[test]
    fn glyphs_advance_left_to_right_at_the_font_advance() {
        let f = font();
        let layout = UiTextLayout::layout("abc", &f, &style(), [0.0, 0.0]);
        let glyphs: Vec<_> = layout.glyphs().collect();
        assert_eq!(glyphs.len(), 3);
        // UiBlockFont draws each glyph with a zero offset, so the quad starts
        // exactly at the pen.
        assert_eq!(glyphs[0].position[0], 0.0);
        assert_eq!(glyphs[1].position[0], 8.0);
        assert_eq!(glyphs[2].position[0], 16.0);
    }

    #[test]
    fn glyphs_sit_on_the_baseline_not_the_line_top() {
        let f = font();
        let layout = UiTextLayout::layout("a", &f, &style(), [0.0, 0.0]);
        // A glyph with a zero y-offset is placed one ascent down, so its top
        // edge is the baseline.
        assert_eq!(layout.glyphs().next().unwrap().position[1], f.ascent);
    }

    #[test]
    fn the_requested_size_scales_the_whole_run() {
        let f = font();
        let s = UiTextStyle {
            size: 32.0, // double the natural 16
            ..style()
        };
        let layout = UiTextLayout::layout("abc", &f, &s, [0.0, 0.0]);
        let glyphs: Vec<_> = layout.glyphs().collect();
        assert_eq!(glyphs[1].position[0], 16.0, "advance did not scale");
        assert_eq!(layout.line_height(), (f.ascent() + f.descent()) * 2.0);
    }

    #[test]
    fn a_nonsensical_size_leaves_the_run_unscaled() {
        // Dividing by a zero natural size would put infinities in every
        // position, which renders nothing and reports nothing.
        let f = font();
        for size in [0.0, -8.0, f32::NAN, f32::INFINITY] {
            let s = UiTextStyle {
                size,
                ..style()
            };
            let layout = UiTextLayout::layout("abc", &f, &s, [0.0, 0.0]);
            for g in layout.glyphs() {
                assert!(
                    g.position.iter().all(|v| v.is_finite()),
                    "size {size} produced a non-finite position"
                );
            }
        }
    }

    #[test]
    fn a_newline_starts_a_new_line_one_line_height_down() {
        let f = font();
        let layout = UiTextLayout::layout("ab\ncd", &f, &style(), [0.0, 0.0]);
        assert_eq!(layout.line_count(), 2);
        let lh = layout.line_height();
        assert_eq!(layout.lines()[0].origin[1], 0.0);
        assert_eq!(layout.lines()[1].origin[1], lh);
    }

    #[test]
    fn the_origin_offsets_every_line() {
        let f = font();
        let layout = UiTextLayout::layout("a\nb", &f, &style(), [100.0, 50.0]);
        assert_eq!(layout.lines()[0].origin[0], 100.0);
        assert_eq!(layout.lines()[0].origin[1], 50.0);
        assert_eq!(layout.lines()[1].origin[0], 100.0);
        assert_eq!(layout.lines()[1].origin[1], 50.0 + layout.line_height());
    }

    #[test]
    fn trailing_spaces_are_not_counted_in_the_line_width() {
        let f = font();
        let narrow = UiTextLayout::layout("ab", &f, &style(), [0.0, 0.0]);
        let padded = UiTextLayout::layout("ab    ", &f, &style(), [0.0, 0.0]);
        assert_eq!(narrow.lines()[0].width, padded.lines()[0].width);
        assert_eq!(narrow.lines()[0].text, "ab");
    }

    #[test]
    fn left_alignment_leaves_the_line_at_the_origin() {
        let f = font();
        let s = UiTextStyle {
            align: UiAlignment::TopLeft,
            max_width: Some(200.0),
            ..style()
        };
        let layout = UiTextLayout::layout("ab", &f, &s, [10.0, 0.0]);
        assert_eq!(layout.lines()[0].origin[0], 10.0);
    }

    #[test]
    fn centre_alignment_shifts_by_half_the_slack() {
        let f = font();
        let s = UiTextStyle {
            align: UiAlignment::TopCenter,
            max_width: Some(100.0),
            ..style()
        };
        let layout = UiTextLayout::layout("ab", &f, &s, [0.0, 0.0]);
        // 2 glyphs at 8.0 = 16.0 wide in a 100.0 box: (100 - 16) / 2.
        assert_eq!(layout.lines()[0].origin[0], 42.0);
    }

    #[test]
    fn right_alignment_shifts_by_the_full_slack() {
        let f = font();
        let s = UiTextStyle {
            align: UiAlignment::TopRight,
            max_width: Some(100.0),
            ..style()
        };
        let layout = UiTextLayout::layout("ab", &f, &s, [0.0, 0.0]);
        assert_eq!(layout.lines()[0].origin[0], 84.0);
    }

    #[test]
    fn a_line_longer_than_the_limit_breaks_at_a_space() {
        let f = font();
        // Glyphs are 8.0 wide: "ab" is 16, "cd" is 16, and joining them costs a
        // further 8 for the space. At 30 there is not enough room for both.
        let s = UiTextStyle {
            max_width: Some(30.0),
            ..style()
        };
        let layout = UiTextLayout::layout("ab cd", &f, &s, [0.0, 0.0]);
        assert_eq!(layout.line_count(), 2);
        assert_eq!(layout.lines()[0].text, "ab");
        assert_eq!(layout.lines()[1].text, "cd");
    }

    #[test]
    fn wrapping_is_greedy_and_fills_the_first_line_it_can() {
        // "ab cd" at exactly 40 fits whole, so no break happens at all.
        let f = font();
        let s = UiTextStyle {
            max_width: Some(40.0),
            ..style()
        };
        let layout = UiTextLayout::layout("ab cd", &f, &s, [0.0, 0.0]);
        assert_eq!(layout.line_count(), 1);
        assert_eq!(layout.lines()[0].text, "ab cd");
    }

    #[test]
    fn a_wrapped_line_does_not_start_with_a_space() {
        let f = font();
        let s = UiTextStyle {
            max_width: Some(40.0),
            ..style()
        };
        let layout = UiTextLayout::layout("ab cd", &f, &s, [0.0, 0.0]);
        for line in layout.lines() {
            assert!(
                !line.text.starts_with(' '),
                "line {:?} starts with a space",
                line.text
            );
        }
    }

    #[test]
    fn a_word_longer_than_the_limit_overflows_instead_of_being_split() {
        // Breaking inside a word would need per-character measurement and
        // hyphenation. The important properties are that it terminates and that
        // the overflow stays visible rather than being silently absorbed.
        let f = font();
        let s = UiTextStyle {
            max_width: Some(8.0), // narrower than the single word "abcdefgh"
            ..style()
        };
        let layout = UiTextLayout::layout("abcdefgh", &f, &s, [0.0, 0.0]);
        assert_eq!(layout.line_count(), 1);
        assert_eq!(layout.lines()[0].text, "abcdefgh");
        assert!(
            layout.lines()[0].width > 8.0,
            "the overflow was hidden"
        );
    }

    #[test]
    fn a_nonsensical_wrap_width_is_treated_as_unbounded() {
        // A zero or negative limit would break every word onto its own line, or
        // loop forever depending on the arithmetic.
        let f = font();
        for limit in [0.0, -10.0, f32::NAN, f32::INFINITY] {
            let s = UiTextStyle {
                max_width: Some(limit),
                ..style()
            };
            let layout = UiTextLayout::layout("ab cd", &f, &s, [0.0, 0.0]);
            assert_eq!(
                layout.line_count(),
                1,
                "limit {limit} produced {} lines",
                layout.line_count()
            );
        }
    }

    #[test]
    fn wrapping_resets_at_a_newline() {
        let f = font();
        let s = UiTextStyle {
            max_width: Some(30.0),
            ..style()
        };
        let layout = UiTextLayout::layout("ab\ncd ef", &f, &s, [0.0, 0.0]);
        // Paragraph one is short enough to stay whole; paragraph two wraps.
        assert_eq!(layout.line_count(), 3);
        assert_eq!(layout.lines()[0].text, "ab");
        assert_eq!(layout.lines()[1].text, "cd");
        assert_eq!(layout.lines()[2].text, "ef");
    }

    #[test]
    fn a_run_of_spaces_collapses_to_one_separator() {
        let f = font();
        let s = UiTextStyle {
            max_width: Some(200.0),
            ..style()
        };
        let layout = UiTextLayout::layout("a     b", &f, &s, [0.0, 0.0]);
        assert_eq!(layout.lines()[0].text, "a b");
        assert_eq!(layout.lines()[0].width, 3.0 * 8.0);
    }

    #[test]
    fn kerning_is_applied_between_glyphs_on_one_line() {
        #[derive(Debug)]
        struct Kerned;
        impl UiFont for Kerned {
            fn glyph(&self, _ch: char) -> Option<UiGlyph> {
                Some(UiGlyph {
                    atlas_rect: [0.0, 0.0, 1.0, 1.0],
                    offset: [0.0, 0.0],
                    size: [8.0, 8.0],
                    advance: 8.0,
                })
            }
            fn natural_size(&self) -> f32 {
                16.0
            }
            fn ascent(&self) -> f32 {
                12.0
            }
            fn descent(&self) -> f32 {
                4.0
            }
            fn kerning(&self, left: char, right: char) -> f32 {
                if left == 'A' && right == 'V' {
                    -2.0
                } else {
                    0.0
                }
            }
        }

        let layout = UiTextLayout::layout("AV", &Kerned, &style(), [0.0, 0.0]);
        let glyphs: Vec<_> = layout.glyphs().collect();
        assert_eq!(glyphs[1].position[0], 6.0, "the -2 kern was not applied");
    }

    #[test]
    fn kerning_is_not_applied_across_a_word_boundary() {
        // The pair is tightened, but a space separates them, so the tightening
        // must not be applied: measurement splits on spaces and measures each
        // word on its own.
        #[derive(Debug)]
        struct Kerned;
        impl UiFont for Kerned {
            fn glyph(&self, _ch: char) -> Option<UiGlyph> {
                Some(UiGlyph {
                    atlas_rect: [0.0, 0.0, 1.0, 1.0],
                    offset: [0.0, 0.0],
                    size: [8.0, 8.0],
                    advance: 8.0,
                })
            }
            fn natural_size(&self) -> f32 {
                16.0
            }
            fn ascent(&self) -> f32 {
                12.0
            }
            fn descent(&self) -> f32 {
                4.0
            }
            fn kerning(&self, left: char, right: char) -> f32 {
                if left == 'A' && right == 'V' {
                    -100.0
                } else {
                    0.0
                }
            }
        }

        let layout = UiTextLayout::layout("A V", &Kerned, &style(), [0.0, 0.0]);
        let glyphs: Vec<_> = layout.glyphs().collect();
        assert_eq!(glyphs.len(), 3, "expected A, space, V");
        // A at 0, then one advance for the space, then V at 16. A -100 kern
        // would have pulled V back to -84.
        assert_eq!(glyphs[2].position[0], 16.0, "kerning leaked across the space");
    }

    #[test]
    fn a_character_with_no_glyph_and_no_fallback_is_dropped() {
        #[derive(Debug)]
        struct Empty;
        impl UiFont for Empty {
            fn glyph(&self, _ch: char) -> Option<UiGlyph> {
                None
            }
            fn natural_size(&self) -> f32 {
                16.0
            }
            fn ascent(&self) -> f32 {
                12.0
            }
            fn descent(&self) -> f32 {
                4.0
            }
        }

        let layout = UiTextLayout::layout("abc", &Empty, &style(), [0.0, 0.0]);
        assert_eq!(layout.glyph_count(), 0);
        assert!(layout.is_empty());
        assert_eq!(layout.lines()[0].width, 0.0);
    }

    #[test]
    fn the_fallback_glyph_keeps_its_advance_so_text_does_not_close_up() {
        // UiBlockFont covers ASCII, so a non-Latin character takes the fallback.
        // The run must stay as wide as if the glyph were real, or a missing
        // character silently eats the space of the ones around it.
        let f = font();
        let with_fallback = UiTextLayout::layout("aЖ", &f, &style(), [0.0, 0.0]);
        assert_eq!(with_fallback.glyph_count(), 2, "the fallback was dropped");
        assert!(with_fallback.lines()[0].width >= 16.0);
    }

    #[test]
    fn the_block_font_reports_distinct_atlas_cells_per_character() {
        // Two glyphs sharing a cell would draw the same shape twice.
        let f = font();
        let a = f.glyph('A').unwrap();
        let b = f.glyph('B').unwrap();
        assert_ne!(a.atlas_rect, b.atlas_rect);
    }

    #[test]
    fn line_spacing_widens_the_block_without_moving_the_first_line() {
        let f = font();
        let plain = UiTextLayout::layout("a\nb", &f, &style(), [0.0, 0.0]);
        let spaced = UiTextLayout::layout(
            "a\nb",
            &f,
            &UiTextStyle {
                line_spacing: 0.5,
                ..style()
            },
            [0.0, 0.0],
        );
        assert_eq!(spaced.lines()[0].origin[1], 0.0, "the first line moved");
        assert!(spaced.line_height() > plain.line_height());
    }

    #[test]
    fn the_reported_height_matches_the_line_count() {
        let f = font();
        let layout = UiTextLayout::layout("a\nb\nc", &f, &style(), [0.0, 0.0]);
        assert_eq!(layout.height(), 3.0 * layout.line_height());
    }

    #[test]
    fn a_glyph_rect_covers_position_plus_size() {
        let f = font();
        let layout = UiTextLayout::layout("a", &f, &style(), [10.0, 20.0]);
        let g = layout.glyphs().next().unwrap();
        let r = g.rect();
        assert_eq!(r[0], g.position[0]);
        assert_eq!(r[1], g.position[1]);
        assert_eq!(r[2], g.position[0] + g.glyph.size[0]);
        assert_eq!(r[3], g.position[1] + g.glyph.size[1]);
    }

    #[test]
    fn measuring_agrees_with_the_placed_run() {
        // These two are computed by different code paths; if they disagree,
        // alignment and wrapping are both computed against the wrong width.
        let f = font();
        for text in ["", "a", "abc", "hello world"] {
            let measured = measure(text, &f, &style());
            let layout = UiTextLayout::layout(text, &f, &style(), [0.0, 0.0]);
            assert!(
                (measured - layout.lines()[0].width).abs() < 1e-4,
                "{text:?}: measured {measured}, placed {}",
                layout.lines()[0].width
            );
        }
    }
}