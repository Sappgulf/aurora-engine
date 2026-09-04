//! Vector font rendering: font loading, glyph atlas rasterization, text
//! layout, and a bridge into the sprite pipeline.
//!
//! The atlas stores image-space UVs (V=0 at the top row of the texture),
//! exactly like [`crate::atlas::TextureAtlas`]. Because
//! [`crate::sprite`] maps image-space UVs onto Y-up world corners
//! (`sprite_corner_uvs`), atlas UVs pass through to [`Sprite`] unchanged and
//! text renders upright.
//!
//! End-to-end flow (GPU setup elided):
//!
//! ```text
//! let font = Font::from_bytes(font_bytes)?;
//! let atlas = GlyphAtlas::build(&font, 32);
//! let texture = Texture::from_rgba(&gpu, atlas.width(), atlas.height(), atlas.pixels(), "font atlas");
//! let handle = renderer.add_texture(&texture);
//! let layout = TextLayout::new(font, atlas).with_align(Align::Left);
//! let glyphs = layout.layout_text("Score: 100", Vec2::new(20.0, 400.0), None);
//! for sprite in glyphs_to_sprites(&glyphs, Color::WHITE, 0.5) {
//!     renderer.draw_sprite(&handle, &sprite);
//! }
//! ```

use std::collections::BTreeMap;
use std::fmt;

use ab_glyph::{Font as _, FontArc, PxScale, ScaleFont};
use glam::Vec2;

use crate::color::Color;
use crate::sprite::Sprite;

/// Line spacing as a multiple of the atlas pixel height.
pub const LINE_HEIGHT_FACTOR: f32 = 1.25;

/// Characters rasterized by [`GlyphAtlas::build`]: printable ASCII plus a few
/// symbols. Order does not matter; glyphs are packed in sorted order.
pub const DEFAULT_CHARSET: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~°±×÷©®éñüöä→★";

/// Padding in pixels kept between glyph cells in the atlas.
const GLYPH_PAD: u32 = 1;

/// Minimum atlas width; grows only when a single glyph is wider.
const ATLAS_MIN_WIDTH: u32 = 256;

/// Error returned when loading font data fails.
#[derive(Debug)]
pub enum FontError {
    /// The provided byte buffer was empty.
    EmptyBytes,
    /// The bytes are not a supported font (TrueType / OpenType).
    InvalidFont(ab_glyph::InvalidFont),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBytes => write!(f, "font data is empty"),
            Self::InvalidFont(e) => write!(f, "invalid font data: {e}"),
        }
    }
}

impl std::error::Error for FontError {}

impl From<ab_glyph::InvalidFont> for FontError {
    fn from(e: ab_glyph::InvalidFont) -> Self {
        Self::InvalidFont(e)
    }
}

/// A parsed vector font wrapping [`ab_glyph::FontArc`].
///
/// Cheap to clone and share; build a [`GlyphAtlas`] once per font + size.
#[derive(Debug, Clone)]
pub struct Font {
    arc: FontArc,
}

impl Font {
    /// Parse font data (`.ttf` / `.otf` bytes) from memory.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, FontError> {
        if data.is_empty() {
            return Err(FontError::EmptyBytes);
        }
        Ok(Self {
            arc: FontArc::try_from_vec(data)?,
        })
    }

    /// The underlying `ab_glyph` font.
    pub fn inner(&self) -> &FontArc {
        &self.arc
    }
}

/// Pixel scale clamped to a sane minimum so zero heights stay deterministic.
fn px_scale(pixel_height: f32) -> PxScale {
    PxScale::from(pixel_height.max(1.0))
}

/// Scaled metrics for one glyph, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMetrics {
    /// Horizontal advance added to the pen when laying out this glyph.
    pub advance: f32,
    /// Raster size in pixels (rounded up).
    pub glyph_size: Vec2,
    /// Pixels from the baseline up to the top row of the raster.
    pub top_bearing: f32,
    /// Pixels from the pen to the left edge of the raster.
    pub left_bearing: f32,
}

/// Supply of glyph metrics, abstracted so layout and packing can be exercised
/// without a real font file (see [`Font`]'s implementation for the real one).
pub trait GlyphSource {
    /// Scaled ascent in pixels: distance from the baseline to the text top.
    fn ascent(&self, pixel_height: f32) -> f32;

    /// Metrics for a char at the given pixel height, or `None` when the
    /// source cannot provide the glyph.
    fn metrics(&self, c: char, pixel_height: f32) -> Option<GlyphMetrics>;

    /// Extra horizontal offset in pixels applied between two glyphs
    /// (may be negative). Defaults to zero.
    fn kerning(&self, _prev: char, _next: char, _pixel_height: f32) -> f32 {
        0.0
    }
}

impl GlyphSource for Font {
    fn ascent(&self, pixel_height: f32) -> f32 {
        self.arc.as_scaled(px_scale(pixel_height)).ascent()
    }

    fn metrics(&self, c: char, pixel_height: f32) -> Option<GlyphMetrics> {
        let scaled = self.arc.as_scaled(px_scale(pixel_height));
        let id = scaled.glyph_id(c);
        let advance = scaled.h_advance(id);
        let Some(outlined) = self
            .arc
            .outline_glyph(id.with_scale(px_scale(pixel_height)))
        else {
            return Some(GlyphMetrics {
                advance,
                glyph_size: Vec2::ZERO,
                top_bearing: 0.0,
                left_bearing: 0.0,
            });
        };
        let bounds = outlined.px_bounds();
        Some(GlyphMetrics {
            advance,
            glyph_size: Vec2::new(bounds.width().ceil(), bounds.height().ceil()),
            top_bearing: bounds.max.y,
            left_bearing: bounds.min.x,
        })
    }

    fn kerning(&self, prev: char, next: char, pixel_height: f32) -> f32 {
        let scaled = self.arc.as_scaled(px_scale(pixel_height));
        scaled.kern(scaled.glyph_id(prev), scaled.glyph_id(next))
    }
}

/// Horizontal text alignment within a block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    /// Lines start at the block origin.
    #[default]
    Left,
    /// Lines are centered on the block origin (or within `max_width`).
    Center,
    /// Lines end at the block origin (or at `origin + max_width`).
    Right,
}

/// One glyph cell placed in the atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphEntry {
    /// UV of the glyph's top-left corner, image-space (V increases down),
    /// inset by half a texel like [`crate::atlas::TextureAtlas`].
    pub uv_min: Vec2,
    /// UV of the glyph's bottom-right corner, image-space.
    pub uv_max: Vec2,
    /// Horizontal advance in pixels.
    pub advance: f32,
    /// Pixels from the baseline up to the top row of the raster.
    pub top_bearing: f32,
    /// Pixels from the pen to the left edge of the raster.
    pub left_bearing: f32,
    /// Raster size in pixels.
    pub glyph_size: Vec2,
}

/// Simple shelf (row) packer for glyph cells. Public so the packing behavior
/// can be tested directly with synthetic sizes.
#[derive(Debug, Clone)]
pub struct ShelfPacker {
    width: u32,
    height: u32,
    pen_x: u32,
    pen_y: u32,
    row_height: u32,
}

impl ShelfPacker {
    /// Start packing into an atlas of the given width; the height grows on
    /// demand and dimensions stay powers of two.
    pub fn new(width: u32) -> Self {
        Self {
            width: width.max(1),
            height: 1,
            pen_x: 0,
            pen_y: 0,
            row_height: 0,
        }
    }

    /// Reserve a `w x h` cell and return its top-left corner in pixels.
    /// Cells handed out never overlap.
    pub fn allocate(&mut self, w: u32, h: u32) -> (u32, u32) {
        let w = w.max(1);
        let h = h.max(1);
        if w > self.width {
            self.width = w.next_power_of_two();
        }
        if self.pen_x + w > self.width {
            self.pen_y += self.row_height;
            self.pen_x = 0;
            self.row_height = 0;
        }
        if self.pen_y + h > self.height {
            self.height = (self.pen_y + h).next_power_of_two();
        }
        let slot = (self.pen_x, self.pen_y);
        self.pen_x += w;
        self.row_height = self.row_height.max(h);
        slot
    }

    /// Current atlas width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Current atlas height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Rasterized glyph atlas: white glyphs with alpha coverage, packed into a
/// single RGBA payload ready for [`crate::texture::Texture::from_rgba`].
#[derive(Clone)]
pub struct GlyphAtlas {
    width: u32,
    height: u32,
    pixel_height: u32,
    ascent: f32,
    entries: BTreeMap<char, GlyphEntry>,
    pixels: Vec<u8>,
}

impl fmt::Debug for GlyphAtlas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlyphAtlas")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixel_height", &self.pixel_height)
            .field("ascent", &self.ascent)
            .field("glyphs", &self.entries.len())
            .field("pixel_bytes", &self.pixels.len())
            .finish()
    }
}

/// Sorted, de-duplicated view of [`DEFAULT_CHARSET`].
pub fn default_charset() -> Vec<char> {
    let mut chars: Vec<char> = DEFAULT_CHARSET.chars().collect();
    chars.sort_unstable();
    chars.dedup();
    chars
}

struct AtlasBuild {
    atlas: GlyphAtlas,
    slots: BTreeMap<char, (u32, u32)>,
}

fn build_slots(source: &dyn GlyphSource, charset: &[char], pixel_height: u32) -> AtlasBuild {
    let px = pixel_height.max(1) as f32;
    let mut chars: Vec<char> = charset.to_vec();
    chars.sort_unstable();
    chars.dedup();

    let mut metrics = Vec::with_capacity(chars.len());
    for c in chars {
        if let Some(m) = source.metrics(c, px) {
            metrics.push((c, m));
        }
    }

    let mut packer = ShelfPacker::new(ATLAS_MIN_WIDTH);
    let mut placed = Vec::with_capacity(metrics.len());
    for (c, m) in &metrics {
        let gw = m.glyph_size.x.ceil().max(1.0) as u32;
        let gh = m.glyph_size.y.ceil().max(1.0) as u32;
        let (sx, sy) = packer.allocate(gw + 2 * GLYPH_PAD, gh + 2 * GLYPH_PAD);
        placed.push((*c, sx + GLYPH_PAD, sy + GLYPH_PAD, gw, gh));
    }

    let (width, height) = (packer.width(), packer.height());
    let mut entries = BTreeMap::new();
    let mut slots = BTreeMap::new();
    for (c, sx, sy, gw, gh) in placed {
        let m = metrics
            .iter()
            .find(|(mc, _)| *mc == c)
            .map(|(_, m)| *m)
            .unwrap_or(GlyphMetrics {
                advance: 0.0,
                glyph_size: Vec2::ZERO,
                top_bearing: 0.0,
                left_bearing: 0.0,
            });
        let uv_min = Vec2::new(
            (sx as f32 + 0.5) / width as f32,
            (sy as f32 + 0.5) / height as f32,
        );
        let uv_max = Vec2::new(
            ((sx + gw) as f32 - 0.5) / width as f32,
            ((sy + gh) as f32 - 0.5) / height as f32,
        );
        entries.insert(
            c,
            GlyphEntry {
                uv_min,
                uv_max,
                advance: m.advance,
                top_bearing: m.top_bearing,
                left_bearing: m.left_bearing,
                glyph_size: m.glyph_size,
            },
        );
        slots.insert(c, (sx, sy));
    }

    AtlasBuild {
        atlas: GlyphAtlas {
            width,
            height,
            pixel_height: pixel_height.max(1),
            ascent: source.ascent(px),
            entries,
            pixels: vec![0; (width as usize) * (height as usize) * 4],
        },
        slots,
    }
}

impl GlyphAtlas {
    /// Rasterize the default charset (`DEFAULT_CHARSET`) at `pixel_height`
    /// into a white-on-transparent RGBA payload.
    pub fn build(font: &Font, pixel_height: u32) -> GlyphAtlas {
        Self::build_with_charset(font, &default_charset(), pixel_height)
    }

    /// Rasterize a custom charset at `pixel_height`.
    pub fn build_with_charset(font: &Font, charset: &[char], pixel_height: u32) -> GlyphAtlas {
        let build = build_slots(font, charset, pixel_height);
        let mut atlas = build.atlas;
        let scale = px_scale(pixel_height.max(1) as f32);
        let (width, height) = (atlas.width, atlas.height);
        for (c, (sx, sy)) in &build.slots {
            let Some(entry) = atlas.entries.get(c) else {
                continue;
            };
            if entry.glyph_size.x < 1.0 || entry.glyph_size.y < 1.0 {
                continue;
            }
            let glyph = font.arc.glyph_id(*c).with_scale(scale);
            let Some(outlined) = font.arc.outline_glyph(glyph) else {
                continue;
            };
            let pixels = &mut atlas.pixels;
            outlined.draw(|x, y, coverage| {
                let a = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
                if a == 0 {
                    return;
                }
                let px_x = sx + x;
                let px_y = sy + y;
                if px_x >= width || px_y >= height {
                    return;
                }
                let i = ((px_y * width + px_x) * 4) as usize;
                pixels[i] = 255;
                pixels[i + 1] = 255;
                pixels[i + 2] = 255;
                pixels[i + 3] = a;
            });
        }
        atlas
    }

    /// Pack glyph cells from any [`GlyphSource`] without rasterizing ink;
    /// the payload stays transparent. Useful for tests and tools.
    pub fn build_with_source(
        source: &dyn GlyphSource,
        charset: &[char],
        pixel_height: u32,
    ) -> GlyphAtlas {
        build_slots(source, charset, pixel_height).atlas
    }

    /// Atlas width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Atlas height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Texture size as a `Vec2`.
    pub fn texture_size(&self) -> Vec2 {
        Vec2::new(self.width as f32, self.height as f32)
    }

    /// Requested glyph pixel height.
    pub fn pixel_height(&self) -> u32 {
        self.pixel_height
    }

    /// Scaled ascent in pixels used to place the first line.
    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    /// Line spacing in pixels: `pixel_height * LINE_HEIGHT_FACTOR`.
    pub fn line_height(&self) -> f32 {
        self.pixel_height as f32 * LINE_HEIGHT_FACTOR
    }

    /// Atlas entry for a char, if the char was rasterized.
    pub fn entry(&self, c: char) -> Option<&GlyphEntry> {
        self.entries.get(&c)
    }

    /// Whether the atlas contains a glyph for this char.
    pub fn contains(&self, c: char) -> bool {
        self.entries.contains_key(&c)
    }

    /// Characters present in the atlas, sorted.
    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        self.entries.keys().copied()
    }

    /// Number of glyphs in the atlas.
    pub fn glyph_count(&self) -> usize {
        self.entries.len()
    }

    /// RGBA payload (white glyph ink, alpha coverage), top row first:
    /// directly compatible with [`crate::texture::Texture::from_rgba`].
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// Total advance width of a single line: sum of atlas advances plus source
/// kerning between glyphs present in the atlas. Unknown chars are skipped.
fn atlas_line_width(atlas: &GlyphAtlas, source: &dyn GlyphSource, text: &str, px: f32) -> f32 {
    let mut width = 0.0;
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        let Some(entry) = atlas.entries.get(&ch) else {
            prev = None;
            continue;
        };
        if let Some(p) = prev {
            width += source.kerning(p, ch, px);
        }
        width += entry.advance;
        prev = Some(ch);
    }
    width
}

/// Width of a line measured from a [`GlyphSource`] directly (no atlas).
pub fn line_width_with(source: &dyn GlyphSource, text: &str, pixel_height: f32) -> f32 {
    let mut width = 0.0;
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        if let Some(p) = prev {
            width += source.kerning(p, ch, pixel_height);
        }
        if let Some(m) = source.metrics(ch, pixel_height) {
            width += m.advance;
            prev = Some(ch);
        } else {
            prev = None;
        }
    }
    width
}

/// Measure a single line of text: `(advance width, line height)`.
/// If `text` contains `'\n'`, the widest segment wins.
pub fn measure_line_with(source: &dyn GlyphSource, text: &str, pixel_height: f32) -> Vec2 {
    let mut widest = 0.0f32;
    for segment in text.split('\n') {
        widest = widest.max(line_width_with(source, segment, pixel_height));
    }
    Vec2::new(widest, pixel_height.max(1.0) * LINE_HEIGHT_FACTOR)
}

/// Greedy word wrap of one paragraph: appends line slices of `para` to `out`,
/// breaking before any word that would push the line past `max_width`.
/// A word wider than `max_width` is emitted alone on its line.
fn push_wrapped_paragraph<'a>(
    width_of: &dyn Fn(&str) -> f32,
    para: &'a str,
    max_width: f32,
    out: &mut Vec<&'a str>,
) {
    if width_of(para) <= max_width {
        out.push(para);
        return;
    }
    let mut line_start = 0usize;
    let mut line_end = 0usize;
    let mut scan = 0usize;
    for word in para.split(' ') {
        let word_start = scan;
        scan += word.len();
        let word_end = scan;
        scan += 1;
        if word_end > line_start && line_end > line_start {
            let candidate = &para[line_start..word_end];
            if width_of(candidate) > max_width {
                out.push(&para[line_start..line_end]);
                line_start = word_start;
            }
        }
        line_end = word_end;
    }
    if line_end > line_start {
        out.push(&para[line_start..line_end]);
    }
}

/// Word-wrap `text` (honoring explicit `'\n'`) into line slices measured via
/// a [`GlyphSource`]. `max_width <= 0` disables wrapping.
pub fn wrap_lines_with<'a>(
    source: &dyn GlyphSource,
    text: &'a str,
    pixel_height: f32,
    max_width: f32,
) -> Vec<&'a str> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        if max_width > 0.0 {
            push_wrapped_paragraph(
                &|line| line_width_with(source, line, pixel_height),
                para,
                max_width,
                &mut lines,
            );
        } else {
            lines.push(para);
        }
    }
    lines
}

fn wrap_for_atlas<'a>(
    atlas: &GlyphAtlas,
    source: &dyn GlyphSource,
    text: &'a str,
    px: f32,
    max_width: Option<f32>,
) -> Vec<&'a str> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        match max_width {
            Some(w) if w > 0.0 => push_wrapped_paragraph(
                &|line| atlas_line_width(atlas, source, line, px),
                para,
                w,
                &mut lines,
            ),
            _ => lines.push(para),
        }
    }
    lines
}

/// X origin for a line of `line_width` pixels within a block starting at
/// `block_x`. With no block width, Center/Right align relative to the origin
/// itself; with one, they align inside `[block_x, block_x + block_width]`.
pub fn align_line_origin_x(
    align: Align,
    block_x: f32,
    line_width: f32,
    block_width: Option<f32>,
) -> f32 {
    match align {
        Align::Left => block_x,
        Align::Center => {
            block_x + block_width.map_or(-line_width * 0.5, |w| (w - line_width) * 0.5)
        }
        Align::Right => block_x + block_width.map_or(-line_width, |w| w - line_width),
    }
}

/// A glyph placed in world space (Y-up), ready to become a [`Sprite`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedGlyph {
    /// Top-left corner of the glyph quad in world space.
    pub screen_pos: Vec2,
    /// Quad size in pixels.
    pub size: Vec2,
    /// Image-space UV of the glyph's top-left corner (V increases down).
    pub uv_min: Vec2,
    /// Image-space UV of the glyph's bottom-right corner.
    pub uv_max: Vec2,
}

/// Lay out `text` against a glyph atlas using `source` for kerning.
///
/// `position` is the top-left of the text block in world space (Y-up).
/// Lines break on `'\n'`, on `max_width` when provided, and are aligned per
/// `align`. Line spacing is `pixel_height * LINE_HEIGHT_FACTOR`. Chars
/// missing from the atlas are skipped without advancing the pen.
pub fn layout_with_source(
    source: &dyn GlyphSource,
    atlas: &GlyphAtlas,
    text: &str,
    position: Vec2,
    align: Align,
    max_width: Option<f32>,
) -> Vec<PositionedGlyph> {
    let px = atlas.pixel_height as f32;
    let lines = wrap_for_atlas(atlas, source, text, px, max_width);
    let mut out = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        let line_width = atlas_line_width(atlas, source, line, px);
        let pen_start_x = align_line_origin_x(align, position.x, line_width, max_width);
        let baseline_y = position.y - atlas.ascent - row as f32 * atlas.line_height();
        let mut pen_x = pen_start_x;
        let mut prev: Option<char> = None;
        for ch in line.chars() {
            let Some(entry) = atlas.entries.get(&ch) else {
                prev = None;
                continue;
            };
            if let Some(p) = prev {
                pen_x += source.kerning(p, ch, px);
            }
            if entry.glyph_size.x > 0.0 && entry.glyph_size.y > 0.0 {
                out.push(PositionedGlyph {
                    screen_pos: Vec2::new(
                        pen_x + entry.left_bearing,
                        baseline_y + entry.top_bearing,
                    ),
                    size: entry.glyph_size,
                    uv_min: entry.uv_min,
                    uv_max: entry.uv_max,
                });
            }
            pen_x += entry.advance;
            prev = Some(ch);
        }
    }
    out
}

/// Font + atlas pair with an alignment, mirroring the measure/draw split the
/// bitmap text system offers. The atlas must be built from the same font and
/// pixel height for kerning and advances to agree.
#[derive(Debug, Clone)]
pub struct TextLayout {
    font: Font,
    atlas: GlyphAtlas,
    align: Align,
}

impl TextLayout {
    /// Pair a font with its rasterized atlas (defaults to [`Align::Left`]).
    pub fn new(font: Font, atlas: GlyphAtlas) -> Self {
        Self {
            font,
            atlas,
            align: Align::default(),
        }
    }

    /// Set horizontal alignment (builder style).
    pub fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Current alignment.
    pub fn align(&self) -> Align {
        self.align
    }

    /// The paired font.
    pub fn font(&self) -> &Font {
        &self.font
    }

    /// The paired atlas.
    pub fn atlas(&self) -> &GlyphAtlas {
        &self.atlas
    }

    /// Line spacing in pixels.
    pub fn line_height(&self) -> f32 {
        self.atlas.line_height()
    }

    /// Measure text: `(widest line advance width, line height)`.
    pub fn measure_line(&self, text: &str) -> Vec2 {
        let px = self.atlas.pixel_height as f32;
        let mut widest = 0.0f32;
        for segment in text.split('\n') {
            widest = widest.max(atlas_line_width(&self.atlas, &self.font, segment, px));
        }
        Vec2::new(widest, self.atlas.line_height())
    }

    /// Lay out text with `position` at the block's top-left (world space,
    /// Y-up) and optional word wrap at `max_width`.
    pub fn layout_text(
        &self,
        text: &str,
        position: Vec2,
        max_width: Option<f32>,
    ) -> Vec<PositionedGlyph> {
        layout_with_source(
            &self.font,
            &self.atlas,
            text,
            position,
            self.align,
            max_width,
        )
    }
}

/// Convert laid-out glyphs into sprites for the renderer.
///
/// `screen_pos` (glyph top-left in world) becomes the sprite's center +
/// half-size, and the image-space atlas UVs pass through unchanged: the
/// engine's sprite corner mapping already flips image-space V-down UVs onto
/// Y-up world corners, so the image's bottom row (`uv_max.y`) samples the
/// bottom edge of the quad and text reads upright.
pub fn glyphs_to_sprites(glyphs: &[PositionedGlyph], color: Color, z: f32) -> Vec<Sprite> {
    glyphs
        .iter()
        .map(|g| Sprite {
            position: g.screen_pos + g.size * 0.5,
            size: g.size,
            rotation: 0.0,
            color,
            z,
            uv_min: g.uv_min,
            uv_max: g.uv_max,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite::sprite_corner_uvs;

    const EPS: f32 = 1e-4;

    fn assert_close(a: f32, b: f32) {
        assert!((a - b).abs() < EPS, "expected {a} to be close to {b}");
    }

    /// Synthetic metrics: 'a'|'b'|'c' advance 6, raster 5x7, bearing 0.5/7;
    /// space advances 4; kerning 'a'+'b' pulls in by 2. Ascent 8.
    #[derive(Debug)]
    struct SynthSource;

    impl GlyphSource for SynthSource {
        fn ascent(&self, _pixel_height: f32) -> f32 {
            8.0
        }

        fn metrics(&self, c: char, _pixel_height: f32) -> Option<GlyphMetrics> {
            match c {
                ' ' => Some(GlyphMetrics {
                    advance: 4.0,
                    glyph_size: Vec2::ZERO,
                    top_bearing: 0.0,
                    left_bearing: 0.0,
                }),
                'a' | 'b' | 'c' => Some(GlyphMetrics {
                    advance: 6.0,
                    glyph_size: Vec2::new(5.0, 7.0),
                    top_bearing: 7.0,
                    left_bearing: 0.5,
                }),
                _ => None,
            }
        }

        fn kerning(&self, prev: char, next: char, _pixel_height: f32) -> f32 {
            if prev == 'a' && next == 'b' {
                -2.0
            } else {
                0.0
            }
        }
    }

    fn synth_atlas() -> GlyphAtlas {
        GlyphAtlas::build_with_source(&SynthSource, &['c', 'a', ' ', 'b'], 16)
    }

    fn rects_overlap(a: (u32, u32, u32, u32), b: (u32, u32, u32, u32)) -> bool {
        let (ax, ay, aw, ah) = a;
        let (bx, by, bw, bh) = b;
        !(ax + aw <= bx || bx + bw <= ax || ay + ah <= by || by + bh <= ay)
    }

    #[test]
    fn shelf_packer_slots_never_overlap() {
        let mut packer = ShelfPacker::new(64);
        let sizes = [
            (10, 5),
            (20, 8),
            (30, 30),
            (5, 40),
            (64, 10),
            (16, 16),
            (48, 12),
            (63, 63),
        ];
        let mut rects = Vec::new();
        for (w, h) in sizes {
            let (x, y) = packer.allocate(w, h);
            rects.push((x, y, w, h));
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!rects_overlap(*a, *b), "slots {a:?} and {b:?} overlap");
            }
        }
        for (x, y, w, h) in rects {
            assert!(x + w <= packer.width(), "slot exceeds atlas width");
            assert!(y + h <= packer.height(), "slot exceeds atlas height");
        }
    }

    #[test]
    fn shelf_packer_dimensions_stay_power_of_two_and_grow() {
        let mut packer = ShelfPacker::new(64);
        assert_eq!(packer.height(), 1);
        packer.allocate(8, 8);
        assert_eq!(packer.height(), 8);
        packer.allocate(64, 8);
        assert_eq!(packer.height(), 16);
        packer.allocate(100, 4);
        assert_eq!(packer.width(), 128);
        assert!(packer.width().is_power_of_two());
        assert!(packer.height().is_power_of_two());
    }

    #[test]
    fn synthetic_atlas_entries_use_half_texel_insets() {
        let atlas = synth_atlas();
        let (w, h) = (atlas.width() as f32, atlas.height() as f32);
        assert_eq!(atlas.glyph_count(), 4);
        assert_eq!(atlas.pixels().len(), (w * h * 4.0) as usize);
        assert_eq!(atlas.pixel_height(), 16);
        assert_close(atlas.line_height(), 16.0 * LINE_HEIGHT_FACTOR);

        let entry = atlas.entry('a').expect("glyph 'a' in atlas");
        assert!(entry.uv_min.x >= 0.5 / w - EPS);
        assert!(entry.uv_min.y >= 0.5 / h - EPS);
        assert!(entry.uv_max.x <= 1.0 - 0.5 / w + EPS);
        assert!(entry.uv_max.y <= 1.0 - 0.5 / h + EPS);
        assert!(entry.uv_min.x < entry.uv_max.x);
        assert!(entry.uv_min.y < entry.uv_max.y);
        assert_eq!(entry.glyph_size, Vec2::new(5.0, 7.0));
        assert_close(entry.advance, 6.0);

        let space = atlas.entry(' ').expect("space in atlas");
        assert_eq!(space.glyph_size, Vec2::ZERO);
        assert!(atlas.entry('x').is_none());
        assert!(!atlas.contains('x'));
    }

    #[test]
    fn measure_line_accumulates_advances_and_kerning() {
        assert_close(measure_line_with(&SynthSource, "abc", 16.0).x, 16.0);
        assert_close(measure_line_with(&SynthSource, "aab", 16.0).x, 16.0);
        assert_close(measure_line_with(&SynthSource, "", 16.0).x, 0.0);
        assert_close(measure_line_with(&SynthSource, "a", 16.0).y, 20.0);
        assert_close(measure_line_with(&SynthSource, "a\nbbb", 16.0).x, 18.0);
    }

    #[test]
    fn alignment_left_center_right_origin_math() {
        let atlas = synth_atlas();
        let pos = Vec2::new(100.0, 50.0);

        let left = layout_with_source(&SynthSource, &atlas, "ab", pos, Align::Left, None);
        assert_close(left[0].screen_pos.x, 100.5);

        let center = layout_with_source(&SynthSource, &atlas, "ab", pos, Align::Center, None);
        assert_close(center[0].screen_pos.x, 100.0 - 5.0 + 0.5);

        let right = layout_with_source(&SynthSource, &atlas, "ab", pos, Align::Right, None);
        assert_close(right[0].screen_pos.x, 100.0 - 10.0 + 0.5);

        let center_block =
            layout_with_source(&SynthSource, &atlas, "ab", pos, Align::Center, Some(20.0));
        assert_close(center_block[0].screen_pos.x, 100.0 + 5.0 + 0.5);

        let right_block =
            layout_with_source(&SynthSource, &atlas, "ab", pos, Align::Right, Some(20.0));
        assert_close(right_block[0].screen_pos.x, 100.0 + 20.0 - 10.0 + 0.5);

        assert_close(
            align_line_origin_x(Align::Left, 10.0, 4.0, Some(20.0)),
            10.0,
        );
    }

    #[test]
    fn word_wrap_breaks_at_max_width() {
        let lines = wrap_lines_with(&SynthSource, "aaa bbb ccc", 16.0, 45.0);
        assert_eq!(lines, vec!["aaa bbb", "ccc"]);

        let atlas = synth_atlas();
        let glyphs = layout_with_source(
            &SynthSource,
            &atlas,
            "aaa bbb ccc",
            Vec2::ZERO,
            Align::Left,
            Some(45.0),
        );
        assert_eq!(glyphs.len(), 9);

        let first_row_top = glyphs[0].screen_pos.y;
        let second_row_top = glyphs[6].screen_pos.y;
        assert_close(first_row_top - second_row_top, 16.0 * LINE_HEIGHT_FACTOR);

        let unwrapped = wrap_lines_with(&SynthSource, "aaa bbb", 16.0, 0.0);
        assert_eq!(unwrapped, vec!["aaa bbb"]);

        let oversized = wrap_lines_with(&SynthSource, "aaaaa", 16.0, 4.0);
        assert_eq!(oversized, vec!["aaaaa"]);
    }

    #[test]
    fn multiline_lines_advance_by_line_height() {
        let atlas = synth_atlas();
        let glyphs = layout_with_source(
            &SynthSource,
            &atlas,
            "a\nb\nc",
            Vec2::new(0.0, 100.0),
            Align::Left,
            None,
        );
        assert_eq!(glyphs.len(), 3);
        assert_close(glyphs[0].screen_pos.y - glyphs[1].screen_pos.y, 20.0);
        assert_close(glyphs[1].screen_pos.y - glyphs[2].screen_pos.y, 20.0);

        let empty_middle = layout_with_source(
            &SynthSource,
            &atlas,
            "a\n\nc",
            Vec2::new(0.0, 100.0),
            Align::Left,
            None,
        );
        assert_eq!(empty_middle.len(), 2);
        assert_close(
            empty_middle[0].screen_pos.y - empty_middle[1].screen_pos.y,
            40.0,
        );
    }

    #[test]
    fn kerning_shifts_following_glyph() {
        let atlas = synth_atlas();
        let glyphs = layout_with_source(&SynthSource, &atlas, "ab", Vec2::ZERO, Align::Left, None);
        assert_close(glyphs[1].screen_pos.x - glyphs[0].screen_pos.x, 4.0);
    }

    #[test]
    fn unknown_chars_are_skipped() {
        let atlas = synth_atlas();
        let glyphs = layout_with_source(&SynthSource, &atlas, "a?b", Vec2::ZERO, Align::Left, None);
        assert_eq!(glyphs.len(), 2);
        assert_close(glyphs[0].screen_pos.x, 0.5);
        assert_close(glyphs[1].screen_pos.x, 6.5);
    }

    #[test]
    fn glyphs_to_sprites_uv_flip_keeps_image_bottom_at_world_bottom() {
        let glyph = PositionedGlyph {
            screen_pos: Vec2::new(10.0, 20.0),
            size: Vec2::new(5.0, 7.0),
            uv_min: Vec2::new(0.10, 0.20),
            uv_max: Vec2::new(0.30, 0.40),
        };
        let sprites = glyphs_to_sprites(&[glyph], Color::WHITE, 3.0);
        assert_eq!(sprites.len(), 1);
        let sprite = &sprites[0];

        assert_close(sprite.position.x, 12.5);
        assert_close(sprite.position.y, 23.5);
        assert_eq!(sprite.size, Vec2::new(5.0, 7.0));
        assert_close(sprite.z, 3.0);

        let corners = sprite_corner_uvs(sprite.uv_min, sprite.uv_max);
        assert_eq!(corners[0], Vec2::new(0.10, 0.40));
        assert_eq!(corners[1], Vec2::new(0.30, 0.40));
        assert_eq!(corners[2], Vec2::new(0.30, 0.20));
        assert_eq!(corners[3], Vec2::new(0.10, 0.20));

        let world_bottom = sprite.position.y - sprite.size.y * 0.5;
        let world_top = sprite.position.y + sprite.size.y * 0.5;
        assert_close(world_bottom, glyph.screen_pos.y);
        assert_close(world_top, glyph.screen_pos.y + glyph.size.y);
        assert_close(corners[0].y, sprite.uv_max.y);
        assert!(corners[0].y > corners[3].y);
        assert!(world_bottom < world_top);
    }

    #[test]
    fn laid_out_glyphs_round_trip_through_sprites_upright() {
        let atlas = synth_atlas();
        let glyphs = layout_with_source(&SynthSource, &atlas, "a", Vec2::ZERO, Align::Left, None);
        let entry = atlas.entry('a').expect("glyph present");
        let sprite = &glyphs_to_sprites(&glyphs, Color::AURORA_TEAL, 1.0)[0];

        assert_eq!(sprite.uv_min, entry.uv_min);
        assert_eq!(sprite.uv_max, entry.uv_max);

        let corners = sprite_corner_uvs(sprite.uv_min, sprite.uv_max);
        assert_close(corners[0].y, entry.uv_max.y);
        assert_close(
            sprite.position.y - sprite.size.y * 0.5,
            glyphs[0].screen_pos.y,
        );
        assert_eq!(sprite.color, Color::AURORA_TEAL);
    }

    #[test]
    fn font_from_bytes_rejects_empty_and_garbage() {
        assert!(matches!(
            Font::from_bytes(Vec::new()),
            Err(FontError::EmptyBytes)
        ));
        assert!(matches!(
            Font::from_bytes(b"definitely not a font".to_vec()),
            Err(FontError::InvalidFont(_))
        ));
        let err = Font::from_bytes(Vec::new()).unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn default_charset_is_sorted_unique_printable_ascii_plus_extras() {
        let chars = default_charset();
        let mut sorted = chars.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(chars, sorted);
        assert!(chars.contains(&' '));
        assert!(chars.contains(&'~'));
        assert!(chars.contains(&'°'));
        assert!(chars.contains(&'→'));
    }
}
