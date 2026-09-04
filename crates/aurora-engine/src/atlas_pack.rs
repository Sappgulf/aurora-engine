//! Runtime texture atlas packing: shelf-packs RGBA8 sources into one
//! power-of-two image ready for [`crate::texture::Texture::from_rgba`].

use std::fmt;

use glam::Vec2;

/// Maximum width or height, in texels, of a single atlas entry.
pub const MAX_ENTRY_SIZE: u32 = 2048;

/// Failure modes of [`PackedAtlas::pack`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtlasPackError {
    /// No entries were supplied.
    Empty,
    /// A single entry exceeds [`MAX_ENTRY_SIZE`] in either dimension.
    TooLarge {
        name: String,
        width: u32,
        height: u32,
    },
}

impl fmt::Display for AtlasPackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "cannot pack an empty atlas entry list"),
            Self::TooLarge {
                name,
                width,
                height,
            } => write!(
                f,
                "atlas entry '{name}' ({width}x{height}) exceeds the \
                 {MAX_ENTRY_SIZE} texel maximum dimension"
            ),
        }
    }
}

impl std::error::Error for AtlasPackError {}

/// One RGBA8 source placed inside a packed atlas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedEntry {
    /// Caller-supplied name, echoed back for lookups.
    pub name: String,
    /// Left edge of the placed rect, in texels.
    pub x: u32,
    /// Top edge of the placed rect, in texels (image rows run top to bottom).
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl PackedEntry {
    /// UV min/max for this entry inside an atlas of the given size.
    ///
    /// Applies the engine's half-texel inset (see `TextureAtlas::uv_rect`)
    /// so linear filtering cannot bleed across the 1 px packing padding.
    pub fn uv(&self, atlas_width: u32, atlas_height: u32) -> (Vec2, Vec2) {
        let width = atlas_width.max(1) as f32;
        let height = atlas_height.max(1) as f32;
        let inset_x = 0.5 / width;
        let inset_y = 0.5 / height;
        let u0 = self.x as f32 / width + inset_x;
        let v0 = self.y as f32 / height + inset_y;
        let u1 = (self.x + self.w) as f32 / width - inset_x;
        let v1 = (self.y + self.h) as f32 / height - inset_y;
        (Vec2::new(u0, v0), Vec2::new(u1, v1))
    }
}

/// A deterministically shelf-packed RGBA8 atlas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedAtlas {
    /// Flattened RGBA8 texels, rows top to bottom, `width * height * 4` long.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Placed entries in input order.
    pub entries: Vec<PackedEntry>,
}

impl PackedAtlas {
    /// Shelf-packs `entries` (name, RGBA8 bytes, width, height) in input
    /// order into one square power-of-two image with 1 px padding between
    /// entries. Identical inputs always produce an identical atlas, so the
    /// result can be cached or baked reproducibly.
    ///
    /// Each source slice is expected to be tightly packed
    /// `width * height * 4` RGBA8 texels; short slices are zero-filled and
    /// surplus bytes ignored so a bad payload cannot panic the caller.
    pub fn pack(entries: &[(&str, &[u8], u32, u32)]) -> Result<Self, AtlasPackError> {
        if entries.is_empty() {
            return Err(AtlasPackError::Empty);
        }
        for (name, _, width, height) in entries {
            if *width > MAX_ENTRY_SIZE || *height > MAX_ENTRY_SIZE {
                return Err(AtlasPackError::TooLarge {
                    name: (*name).to_owned(),
                    width: *width,
                    height: *height,
                });
            }
        }

        let mut size = initial_size(entries);
        loop {
            if let Some(placed) = try_shelf_pack(entries, size) {
                return Ok(Self::build(placed, entries, size));
            }
            size *= 2;
        }
    }

    /// Rasterizes a successful layout: zero-filled canvas plus one row copy
    /// per entry row.
    fn build(placed: Vec<PackedEntry>, entries: &[(&str, &[u8], u32, u32)], size: u64) -> Self {
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        for (index, entry) in placed.iter().enumerate() {
            let data = entries[index].1;
            let row_bytes = entry.w as usize * 4;
            for row in 0..entry.h {
                let source = row as usize * row_bytes;
                let target = ((entry.y + row) as usize * size as usize + entry.x as usize) * 4;
                let copy = row_bytes.min(data.len().saturating_sub(source));
                pixels[target..target + copy].copy_from_slice(&data[source..source + copy]);
            }
        }
        Self {
            pixels,
            width: size as u32,
            height: size as u32,
            entries: placed,
        }
    }
}

/// Smallest square side worth trying first: a power of two covering the
/// total padded area, never below the largest single entry dimension.
fn initial_size(entries: &[(&str, &[u8], u32, u32)]) -> u64 {
    let mut area: u64 = 0;
    let mut max_dimension: u64 = 1;
    for (_, _, width, height) in entries {
        area += (u64::from(*width) + 1) * (u64::from(*height) + 1);
        max_dimension = max_dimension.max(u64::from(*width)).max(u64::from(*height));
    }
    ((area as f64).sqrt().ceil() as u64)
        .next_power_of_two()
        .max(max_dimension)
        .max(16)
}

/// One next-fit shelf pass at `size`. Returns the placed rects in input
/// order, or `None` when any entry would spill past the bottom edge; the
/// caller retries with a larger square.
fn try_shelf_pack(entries: &[(&str, &[u8], u32, u32)], size: u64) -> Option<Vec<PackedEntry>> {
    let mut placed = Vec::with_capacity(entries.len());
    let mut cursor_x: u64 = 0;
    let mut cursor_y: u64 = 0;
    let mut shelf_height: u64 = 0;

    for (name, _, width, height) in entries {
        let (w, h) = (u64::from(*width), u64::from(*height));
        if w > size {
            return None;
        }
        if cursor_x + w > size {
            cursor_y += shelf_height + 1;
            cursor_x = 0;
            shelf_height = 0;
        }
        if cursor_y + h > size {
            return None;
        }
        placed.push(PackedEntry {
            name: (*name).to_owned(),
            x: cursor_x as u32,
            y: cursor_y as u32,
            w: *width,
            h: *height,
        });
        shelf_height = shelf_height.max(h);
        cursor_x += w + 1;
    }
    Some(placed)
}

#[cfg(test)]
mod tests {
    use super::{AtlasPackError, PackedAtlas, MAX_ENTRY_SIZE};
    use glam::Vec2;

    fn solid_pixels(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            pixels.extend_from_slice(&rgba);
        }
        pixels
    }

    #[test]
    fn packed_entries_never_overlap_and_keep_one_texel_of_padding() {
        let sources = [
            ("red", solid_pixels(8, 6, [255, 0, 0, 255]), 8u32, 6u32),
            ("green", solid_pixels(5, 5, [0, 255, 0, 255]), 5u32, 5u32),
            ("blue", solid_pixels(9, 4, [0, 0, 255, 255]), 9u32, 4u32),
            ("wide", solid_pixels(12, 2, [255, 255, 0, 255]), 12u32, 2u32),
            ("tall", solid_pixels(2, 11, [0, 255, 255, 255]), 2u32, 11u32),
        ];
        let refs: Vec<(&str, &[u8], u32, u32)> = sources
            .iter()
            .map(|(name, data, w, h)| (*name, data.as_slice(), *w, *h))
            .collect();

        let atlas = PackedAtlas::pack(&refs).expect("assorted entries pack");
        assert_eq!(atlas.entries.len(), refs.len(), "every entry is placed");
        for (entry, (name, _, w, h)) in atlas.entries.iter().zip(&sources) {
            assert_eq!(entry.name, *name, "input order is preserved");
            assert_eq!((entry.w, entry.h), (*w, *h));
            assert!(entry.x + entry.w <= atlas.width);
            assert!(entry.y + entry.h <= atlas.height);
        }

        // Padding-aware disjointness: no two placed rects may overlap or
        // share an edge texel.
        for (i, a) in atlas.entries.iter().enumerate() {
            for b in &atlas.entries[i + 1..] {
                let separated =
                    a.x + a.w < b.x || b.x + b.w < a.x || a.y + a.h < b.y || b.y + b.h < a.y;
                assert!(
                    separated,
                    "entries '{}' and '{}' overlap or touch",
                    a.name, b.name
                );
            }
        }
    }

    #[test]
    fn packed_output_is_a_square_power_of_two() {
        let sources = [
            ("a", solid_pixels(3, 3, [1, 2, 3, 255]), 3u32, 3u32),
            ("b", solid_pixels(5, 7, [4, 5, 6, 255]), 5u32, 7u32),
        ];
        let refs: Vec<(&str, &[u8], u32, u32)> = sources
            .iter()
            .map(|(name, data, w, h)| (*name, data.as_slice(), *w, *h))
            .collect();

        let atlas = PackedAtlas::pack(&refs).expect("small entries pack");
        assert_eq!(atlas.width, atlas.height);
        assert!(atlas.width.is_power_of_two());
        assert!(atlas.height.is_power_of_two());
        assert_eq!(
            atlas.pixels.len(),
            (atlas.width * atlas.height * 4) as usize
        );
    }

    #[test]
    fn uv_rects_map_back_to_each_entrys_own_pixels() {
        let sources = [
            ("red", solid_pixels(8, 6, [255, 0, 0, 255]), 8u32, 6u32),
            ("green", solid_pixels(5, 5, [0, 255, 0, 255]), 5u32, 5u32),
            ("blue", solid_pixels(9, 4, [0, 0, 255, 255]), 9u32, 4u32),
        ];
        let refs: Vec<(&str, &[u8], u32, u32)> = sources
            .iter()
            .map(|(name, data, w, h)| (*name, data.as_slice(), *w, *h))
            .collect();

        let atlas = PackedAtlas::pack(&refs).expect("entries pack");
        for (entry, (name, data, _, _)) in atlas.entries.iter().zip(&sources) {
            let (uv_min, uv_max) = entry.uv(atlas.width, atlas.height);

            // The half-texel inset keeps the rect strictly inside the entry.
            let min_texel = (uv_min * Vec2::new(atlas.width as f32, atlas.height as f32)).floor();
            assert_eq!(
                min_texel,
                Vec2::new(entry.x as f32, entry.y as f32),
                "uv_min of '{name}' leaves its rect"
            );
            let max_texel = (uv_max * Vec2::new(atlas.width as f32, atlas.height as f32)).ceil();
            assert_eq!(
                max_texel,
                Vec2::new((entry.x + entry.w) as f32, (entry.y + entry.h) as f32),
                "uv_max of '{name}' leaves its rect"
            );

            // The rect center samples this entry's own distinctive color.
            let center_uv = (uv_min + uv_max) * 0.5;
            let px = (center_uv.x * atlas.width as f32).round() as usize;
            let py = (center_uv.y * atlas.height as f32).round() as usize;
            let index = (py * atlas.width as usize + px) * 4;
            assert_eq!(
                &atlas.pixels[index..index + 4],
                &data[0..4],
                "uv center of '{name}' hit foreign pixels"
            );
        }
    }

    #[test]
    fn packing_is_deterministic_for_identical_input() {
        let sources = [
            ("one", solid_pixels(7, 3, [10, 20, 30, 255]), 7u32, 3u32),
            ("two", solid_pixels(3, 9, [40, 50, 60, 255]), 3u32, 9u32),
        ];
        let refs: Vec<(&str, &[u8], u32, u32)> = sources
            .iter()
            .map(|(name, data, w, h)| (*name, data.as_slice(), *w, *h))
            .collect();

        let first = PackedAtlas::pack(&refs).expect("first pack");
        let second = PackedAtlas::pack(&refs).expect("second pack");
        assert_eq!(first, second);
    }

    #[test]
    fn pack_rejects_empty_lists_and_oversized_entries() {
        assert_eq!(PackedAtlas::pack(&[]), Err(AtlasPackError::Empty));

        let too_wide = [("", &[0u8; 4][..], MAX_ENTRY_SIZE + 1, 1)];
        assert_eq!(
            PackedAtlas::pack(&too_wide),
            Err(AtlasPackError::TooLarge {
                name: String::new(),
                width: MAX_ENTRY_SIZE + 1,
                height: 1,
            })
        );

        let too_tall = [("tall", &[0u8; 4][..], 1, MAX_ENTRY_SIZE + 1)];
        assert!(matches!(
            PackedAtlas::pack(&too_tall),
            Err(AtlasPackError::TooLarge { .. })
        ));
    }
}
