//! UI kit implementing the Aurora Platformer design direction: dark navy
//! translucent panels with teal borders, key-cap hint bars, and stat chips.
//!
//! Rendered with the engine's sprite batcher + bitmap font — no HTML, same
//! visual language as the superdesign draft (panel `rgba(10,16,40,0.85)`,
//! 1px teal border, chips for stats, dim footer hints).

use glam::Vec2;

use aurora_engine::{BitmapText, Camera2D, Color, Renderer, Sprite, TextureHandle};

pub const PANEL_FILL: Color = Color::rgba(0.04, 0.06, 0.16, 0.85);
pub const TEAL: Color = Color::rgb(0.18, 0.85, 0.72);
pub const TEAL_DIM: Color = Color::rgba(0.18, 0.85, 0.72, 0.4);
pub const AMBER: Color = Color::rgb(1.0, 0.72, 0.25);
pub const CYAN: Color = Color::rgb(0.42, 1.0, 0.91);
pub const INK_DIM: Color = Color::rgba(0.85, 0.88, 1.0, 0.55);
pub const INK: Color = Color::rgba(0.92, 0.95, 1.0, 0.92);

/// One pixel of UI border in world units at the demo's standard text scale.
pub const BORDER: f32 = 2.0;

/// Viewport-anchored rectangle in camera space: `anchor` fractions are from
/// the top-left of the viewport, `size` is world units.
pub struct Anchored {
    pub anchor: Vec2,
    pub size: Vec2,
}

impl Anchored {
    pub fn world_rect(&self, camera: &Camera2D) -> aurora_engine::Aabb {
        let view = camera.visible_world_size();
        let top_left = Vec2::new(
            camera.position.x - view.x * 0.5 + view.x * self.anchor.x,
            camera.position.y + view.y * 0.5 - view.y * self.anchor.y,
        );
        aurora_engine::Aabb::new(top_left, top_left + self.size)
    }
}

const SLICE: f32 = 16.0;

/// Nine-slice rounded panel from `panel9_tile` (48x48 source, 16px slices).
pub fn panel9(renderer: &mut Renderer, texture: TextureHandle, rect: aurora_engine::Aabb, z: f32) {
    let s = SLICE;
    let size = rect.size();
    if size.x < s * 2.0 || size.y < s * 2.0 {
        panel(renderer, texture, rect, TEAL, z);
        return;
    }
    let inner_w = size.x - s * 2.0;
    let inner_h = size.y - s * 2.0;
    let x0 = rect.min.x;
    let y0 = rect.min.y;
    let mut put = |cx: f32, cy: f32, sw: f32, sh: f32, sx: f32, sy: f32| {
        let mut sprite = Sprite::new(Vec2::new(cx, cy), Vec2::new(sw, sh)).with_z(z);
        sprite.uv_min = Vec2::new(sx / 48.0, sy / 48.0);
        sprite.uv_max = Vec2::new((sx + s) / 48.0, (sy + s) / 48.0);
        renderer.draw_sprite(texture, sprite);
    };
    // Corners.
    put(x0 + s * 0.5, y0 + s * 0.5, s, s, 0.0, 0.0);
    put(rect.max.x - s * 0.5, y0 + s * 0.5, s, s, 32.0, 0.0);
    put(x0 + s * 0.5, rect.max.y - s * 0.5, s, s, 0.0, 32.0);
    put(rect.max.x - s * 0.5, rect.max.y - s * 0.5, s, s, 32.0, 32.0);
    // Edges.
    put(rect.center().x, y0 + s * 0.5, inner_w, s, 16.0, 0.0);
    put(
        rect.center().x,
        rect.max.y - s * 0.5,
        inner_w,
        s,
        16.0,
        32.0,
    );
    put(x0 + s * 0.5, rect.center().y, s, inner_h, 0.0, 16.0);
    put(
        rect.max.x - s * 0.5,
        rect.center().y,
        s,
        inner_h,
        32.0,
        16.0,
    );
    // Center.
    let mut center = Sprite::new(rect.center(), Vec2::new(inner_w, inner_h)).with_z(z);
    center.uv_min = Vec2::new(16.0 / 48.0, 16.0 / 48.0);
    center.uv_max = Vec2::new(32.0 / 48.0, 32.0 / 48.0);
    renderer.draw_sprite(texture, center);
}

/// Draws a translucent panel with a 1px teal border. Returns the inner rect.
pub fn panel(
    renderer: &mut Renderer,
    texture: TextureHandle,
    rect: aurora_engine::Aabb,
    border: Color,
    z: f32,
) -> aurora_engine::Aabb {
    renderer.draw_sprite(
        texture,
        Sprite::new(rect.center(), rect.size())
            .with_color(PANEL_FILL)
            .with_z(z),
    );
    draw_outline(renderer, texture, rect, border, z + 0.01);
    rect
}

pub fn draw_outline(
    renderer: &mut Renderer,
    texture: TextureHandle,
    rect: aurora_engine::Aabb,
    color: Color,
    z: f32,
) {
    let size = rect.size();
    let thickness = BORDER;
    // Top, bottom, left, right.
    for (center, span) in [
        (
            Vec2::new(rect.center().x, rect.max.y - thickness * 0.5),
            Vec2::new(size.x, thickness),
        ),
        (
            Vec2::new(rect.center().x, rect.min.y + thickness * 0.5),
            Vec2::new(size.x, thickness),
        ),
        (
            Vec2::new(rect.min.x + thickness * 0.5, rect.center().y),
            Vec2::new(thickness, size.y),
        ),
        (
            Vec2::new(rect.max.x - thickness * 0.5, rect.center().y),
            Vec2::new(thickness, size.y),
        ),
    ] {
        renderer.draw_sprite(
            texture,
            Sprite::new(center, span).with_color(color).with_z(z),
        );
    }
}

/// Small stat chip: bordered pill with centered bitmap text.
pub fn chip(
    renderer: &mut Renderer,
    texture: TextureHandle,
    camera: &Camera2D,
    anchor: Vec2,
    text: &str,
    color: Color,
    z: f32,
) {
    let pixel = 2.0;
    let text_width = text.chars().count() as f32 * pixel * 6.0;
    let pad_x = 14.0;
    let rect = aurora_engine::Aabb::new(
        Vec2::ZERO,
        Vec2::new(text_width + pad_x * 2.0, 2.0 * 9.0 + 10.0),
    );
    let view = camera.visible_world_size();
    let top_left = Vec2::new(
        camera.position.x - view.x * 0.5 + view.x * anchor.x,
        camera.position.y + view.y * 0.5 - view.y * anchor.y,
    );
    let rect = aurora_engine::Aabb::new(top_left, top_left + rect.size());
    panel(renderer, texture, rect, color, z);
    let origin = Vec2::new(
        rect.center().x - text_width * 0.5,
        rect.center().y + pixel * 3.5,
    );
    for cell in BitmapText::glyphs(text, origin, pixel) {
        renderer.draw_sprite(
            texture,
            Sprite::new(cell.position, Vec2::splat(cell.size))
                .with_color(INK)
                .with_z(z + 0.02),
        );
    }
}

/// Left-aligned bitmap text at a pixel size, returning the pen end.
pub fn text(
    renderer: &mut Renderer,
    texture: TextureHandle,
    origin: Vec2,
    content: &str,
    pixel: f32,
    color: Color,
    z: f32,
) {
    // Shadow pass keeps HUD text readable over bright glow.
    let shadow_offset = pixel * 0.8;
    for cell in BitmapText::glyphs(
        content,
        origin + Vec2::new(shadow_offset, -shadow_offset),
        pixel,
    ) {
        renderer.draw_sprite(
            texture,
            Sprite::new(cell.position, Vec2::splat(cell.size))
                .with_color(Color::rgba(0.0, 0.0, 0.05, 0.55))
                .with_z(z - 0.01),
        );
    }
    for cell in BitmapText::glyphs(content, origin, pixel) {
        renderer.draw_sprite(
            texture,
            Sprite::new(cell.position, Vec2::splat(cell.size))
                .with_color(color)
                .with_z(z),
        );
    }
}

pub fn text_width(text: &str, pixel: f32) -> f32 {
    text.chars().count() as f32 * pixel * 6.0
}

/// Right-aligned bitmap text against an x coordinate.
#[allow(clippy::too_many_arguments)] // Keeps call sites readable, mirrors engine particle helpers.
pub fn text_right(
    renderer: &mut Renderer,
    texture: TextureHandle,
    right_x: f32,
    y: f32,
    content: &str,
    pixel: f32,
    color: Color,
    z: f32,
) {
    text(
        renderer,
        texture,
        Vec2::new(right_x - text_width(content, pixel), y),
        content,
        pixel,
        color,
        z,
    );
}

/// Centered bitmap text at a viewport fraction.
#[allow(clippy::too_many_arguments)] // Keeps call sites readable, mirrors engine particle helpers.
pub fn text_centered(
    renderer: &mut Renderer,
    texture: TextureHandle,
    camera: &Camera2D,
    view_fraction: Vec2,
    content: &str,
    pixel: f32,
    color: Color,
    z: f32,
) {
    let view = camera.visible_world_size();
    let x = camera.position.x - text_width(content, pixel) * 0.5;
    let y = camera.position.y + view.y * (view_fraction.y - 0.5);
    text(renderer, texture, Vec2::new(x, y), content, pixel, color, z);
}

/// Key-cap hint: `SPACE`-style word rendered inside a small bordered cap.
pub fn key_cap(
    renderer: &mut Renderer,
    texture: TextureHandle,
    origin: Vec2,
    key: &str,
    label: Option<&str>,
    z: f32,
) {
    let pixel = 2.0;
    let key_width = text_width(key, pixel);
    let cap_rect = aurora_engine::Aabb::new(origin, Vec2::new(key_width + 12.0, pixel * 9.0 + 8.0));
    panel(renderer, texture, cap_rect, TEAL_DIM, z);
    text(
        renderer,
        texture,
        Vec2::new(cap_rect.min.x + 6.0, cap_rect.max.y - pixel * 7.0),
        key,
        pixel,
        INK,
        z + 0.02,
    );
    if let Some(label) = label {
        let label_x = cap_rect.max.x + 8.0;
        let label_y = cap_rect.min.y + 6.0;
        text(
            renderer,
            texture,
            Vec2::new(label_x, label_y),
            label,
            pixel,
            INK_DIM,
            z + 0.02,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_math_matches_the_bitmap_advance() {
        assert_eq!(text_width("SPACE", 2.0), 5.0 * 2.0 * 6.0);
        assert_eq!(text_width("", 3.0), 0.0);
        assert_eq!(text_width("HI", 1.0), 12.0);
    }

    #[test]
    fn anchored_rects_track_the_camera() {
        let mut camera = aurora_engine::Camera2D::new(1280.0, 720.0);
        camera.position = Vec2::new(100.0, 50.0);
        let anchored = Anchored {
            anchor: Vec2::new(0.02, 0.05),
            size: Vec2::new(200.0, 40.0),
        };
        let rect = anchored.world_rect(&camera);
        let view = camera.visible_world_size();
        assert!((rect.min.x - (100.0 - view.x * 0.5 + view.x * 0.02)).abs() < 0.01);
        assert!((rect.min.y - (50.0 + view.y * 0.5 - view.y * 0.05)).abs() < 0.01);

        camera.position.x += 40.0;
        let moved = anchored.world_rect(&camera);
        assert!(
            (moved.min.x - rect.min.x - 40.0).abs() < 0.01,
            "tracks the camera"
        );
    }
}
