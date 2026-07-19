//! Tilemap data and sprite expansion for 2D worlds.

use glam::{IVec2, Vec2};

use crate::{Color, Renderer, TextureAtlas};

/// A named grid layer. Tiles are atlas frame indices; `None` is empty space.
#[derive(Debug, Clone)]
pub struct TileLayer {
    pub name: String,
    pub z: f32,
    pub visible: bool,
    tiles: Vec<Option<u32>>,
}

impl TileLayer {
    fn new(name: impl Into<String>, width: u32, height: u32, z: f32) -> Self {
        Self {
            name: name.into(),
            z,
            visible: true,
            tiles: vec![None; (width * height) as usize],
        }
    }
}

/// Grid map authored in game-world coordinates (top-left origin by default).
#[derive(Debug, Clone)]
pub struct TileMap {
    pub width: u32,
    pub height: u32,
    pub tile_size: Vec2,
    pub origin: Vec2,
    layers: Vec<TileLayer>,
}

impl TileMap {
    pub fn new(width: u32, height: u32, tile_size: Vec2) -> Self {
        Self {
            width,
            height,
            tile_size: tile_size.max(Vec2::ONE),
            origin: Vec2::ZERO,
            layers: Vec::new(),
        }
    }

    pub fn add_layer(&mut self, name: impl Into<String>, z: f32) -> usize {
        let index = self.layers.len();
        self.layers
            .push(TileLayer::new(name, self.width, self.height, z));
        index
    }

    pub fn layers(&self) -> &[TileLayer] {
        &self.layers
    }

    pub fn set_tile(&mut self, layer: usize, cell: IVec2, tile: Option<u32>) -> bool {
        let Some(index) = self.cell_index(cell) else {
            return false;
        };
        let Some(layer) = self.layers.get_mut(layer) else {
            return false;
        };
        layer.tiles[index] = tile;
        true
    }

    pub fn tile(&self, layer: usize, cell: IVec2) -> Option<Option<u32>> {
        let index = self.cell_index(cell)?;
        self.layers.get(layer).map(|layer| layer.tiles[index])
    }

    pub fn cell_center(&self, cell: IVec2) -> Option<Vec2> {
        self.cell_index(cell)?;
        Some(self.origin + (cell.as_vec2() + Vec2::splat(0.5)) * self.tile_size)
    }

    /// Queue one visible layer through Aurora's existing sprite renderer.
    pub fn draw_layer(
        &self,
        renderer: &mut Renderer,
        atlas: &TextureAtlas,
        layer: usize,
        tint: Color,
    ) {
        let Some(layer) = self.layers.get(layer).filter(|layer| layer.visible) else {
            return;
        };
        for (index, tile) in layer.tiles.iter().enumerate() {
            let Some(frame) = tile else { continue };
            let cell = IVec2::new(
                (index as u32 % self.width) as i32,
                (index as u32 / self.width) as i32,
            );
            let position = self.origin + (cell.as_vec2() + Vec2::splat(0.5)) * self.tile_size;
            let sprite = atlas
                .sprite(position, self.tile_size, *frame)
                .with_color(tint)
                .with_z(layer.z);
            renderer.draw_sprite(atlas.texture, sprite);
        }
    }

    /// Queue all visible layers, preserving their explicit z values.
    pub fn draw(&self, renderer: &mut Renderer, atlas: &TextureAtlas, tint: Color) {
        for layer in 0..self.layers.len() {
            self.draw_layer(renderer, atlas, layer, tint);
        }
    }

    fn cell_index(&self, cell: IVec2) -> Option<usize> {
        (cell.x >= 0 && cell.y >= 0 && cell.x < self.width as i32 && cell.y < self.height as i32)
            .then_some((cell.y as u32 * self.width + cell.x as u32) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_store_tiles_and_reject_out_of_bounds_cells() {
        let mut map = TileMap::new(3, 2, Vec2::splat(16.0));
        let ground = map.add_layer("ground", -2.0);
        assert!(map.set_tile(ground, IVec2::new(2, 1), Some(7)));
        assert_eq!(map.tile(ground, IVec2::new(2, 1)), Some(Some(7)));
        assert!(!map.set_tile(ground, IVec2::new(3, 1), Some(1)));
        assert_eq!(map.cell_center(IVec2::new(0, 0)), Some(Vec2::splat(8.0)));
    }
}
