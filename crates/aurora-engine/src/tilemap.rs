//! Tilemap data and sprite expansion for 2D worlds.

use glam::{IVec2, Vec2};

use crate::{Aabb, Color, Renderer, TextureAtlas};

fn checked_layer_size(width: u32, height: u32, what: &str) -> usize {
    let Some(area) = u64::from(width).checked_mul(u64::from(height)) else {
        panic!("{what} dimensions overflow: {width}x{height}");
    };
    usize::try_from(area).unwrap_or_else(|_| {
        panic!("{what} area exceeds addressable memory: {width}x{height}");
    })
}

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
            tiles: vec![None; checked_layer_size(width, height, "tile layer")],
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
    solid: Vec<bool>,
    triggers: Vec<TileTrigger>,
}

/// A named rectangular gameplay region, independent of the map's render layers.
#[derive(Debug, Clone, PartialEq)]
pub struct TileTrigger {
    pub id: String,
    pub bounds: Aabb,
}

impl TileMap {
    pub fn new(width: u32, height: u32, tile_size: Vec2) -> Self {
        Self {
            width,
            height,
            tile_size: tile_size.max(Vec2::ONE),
            origin: Vec2::ZERO,
            layers: Vec::new(),
            solid: vec![false; checked_layer_size(width, height, "tilemap")],
            triggers: Vec::new(),
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

    /// Return the grid cell containing a world-space point.
    pub fn world_to_cell(&self, world: Vec2) -> Option<IVec2> {
        let local = (world - self.origin) / self.tile_size;
        let cell = local.floor().as_ivec2();
        self.cell_index(cell).map(|_| cell)
    }

    /// World-space bounds of a cell.
    pub fn cell_bounds(&self, cell: IVec2) -> Option<Aabb> {
        self.cell_index(cell)?;
        let min = self.origin + cell.as_vec2() * self.tile_size;
        Some(Aabb::new(min, min + self.tile_size))
    }

    /// Mark a cell as blocking for simple world collision.
    pub fn set_solid(&mut self, cell: IVec2, solid: bool) -> bool {
        let Some(index) = self.cell_index(cell) else {
            return false;
        };
        self.solid[index] = solid;
        true
    }

    /// Whether a cell is marked as blocking. Out-of-bounds cells are not solid.
    pub fn is_solid(&self, cell: IVec2) -> bool {
        self.cell_index(cell).is_some_and(|index| self.solid[index])
    }

    /// Return blocking cells touched by a world-space AABB.
    pub fn solid_cells_intersecting(&self, bounds: Aabb) -> Vec<IVec2> {
        if self.width == 0 || self.height == 0 {
            return Vec::new();
        }

        let local_min = (bounds.min - self.origin) / self.tile_size;
        let local_max = (bounds.max - self.origin) / self.tile_size;
        let min = local_min.floor().as_ivec2();
        // AABB max values are inclusive in Aurora, so keep an edge contact visible.
        let max = local_max.floor().as_ivec2();
        let mut cells = Vec::new();
        for y in min.y.max(0)..=max.y.min(self.height as i32 - 1) {
            for x in min.x.max(0)..=max.x.min(self.width as i32 - 1) {
                let cell = IVec2::new(x, y);
                if self.is_solid(cell)
                    && self
                        .cell_bounds(cell)
                        .is_some_and(|tile| tile.intersects(bounds))
                {
                    cells.push(cell);
                }
            }
        }
        cells
    }

    pub fn intersects_solid(&self, bounds: Aabb) -> bool {
        !self.solid_cells_intersecting(bounds).is_empty()
    }

    /// Add a named gameplay region such as an exit, checkpoint, or damage zone.
    pub fn add_trigger(&mut self, id: impl Into<String>, bounds: Aabb) -> usize {
        let index = self.triggers.len();
        self.triggers.push(TileTrigger {
            id: id.into(),
            bounds,
        });
        index
    }

    pub fn triggers(&self) -> &[TileTrigger] {
        &self.triggers
    }

    pub fn triggers_at(&self, point: Vec2) -> Vec<&TileTrigger> {
        self.triggers
            .iter()
            .filter(|trigger| trigger.bounds.contains_point(point))
            .collect()
    }

    pub fn triggers_intersecting(&self, bounds: Aabb) -> Vec<&TileTrigger> {
        self.triggers
            .iter()
            .filter(|trigger| trigger.bounds.intersects(bounds))
            .collect()
    }

    /// Queue one visible layer through Aurora's existing sprite renderer.
    pub fn draw_layer(
        &self,
        renderer: &mut Renderer,
        atlas: &TextureAtlas,
        layer: usize,
        tint: Color,
    ) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let Some(layer) = self.layers.get(layer).filter(|layer| layer.visible) else {
            return;
        };
        for (index, tile) in layer.tiles.iter().enumerate() {
            let Some(frame) = tile else { continue };
            let index = index as u32;
            let cell = IVec2::new(
                (index % self.width) as i32,
                (index / self.width) as i32,
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
            .then_some(
                u64::from(cell.y as u32)
                    .checked_mul(u64::from(self.width))?
                    .checked_add(u64::from(cell.x as u32))
                    .and_then(|index| usize::try_from(index).ok())?,
            )
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

    #[test]
    fn solid_cells_and_triggers_use_world_space() {
        let mut map = TileMap::new(4, 3, Vec2::splat(10.0));
        map.origin = Vec2::new(100.0, 50.0);
        assert!(map.set_solid(IVec2::new(1, 1), true));
        assert_eq!(
            map.world_to_cell(Vec2::new(115.0, 65.0)),
            Some(IVec2::new(1, 1))
        );
        let touching = Aabb::from_center_size(Vec2::new(115.0, 65.0), Vec2::splat(8.0));
        assert_eq!(
            map.solid_cells_intersecting(touching),
            vec![IVec2::new(1, 1)]
        );
        assert!(map.intersects_solid(touching));

        map.add_trigger(
            "exit",
            Aabb::new(Vec2::new(120.0, 50.0), Vec2::new(130.0, 60.0)),
        );
        assert_eq!(map.triggers_at(Vec2::new(125.0, 55.0))[0].id, "exit");
        assert_eq!(map.triggers_intersecting(touching).len(), 0);
    }
}
