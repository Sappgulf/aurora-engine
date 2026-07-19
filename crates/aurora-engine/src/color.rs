//! Color utilities.

/// Linear RGBA color in 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    pub const AURORA_NIGHT: Self = Self::rgb(0.04, 0.05, 0.12);
    pub const AURORA_TEAL: Self = Self::rgb(0.15, 0.85, 0.72);
    pub const AURORA_VIOLET: Self = Self::rgb(0.55, 0.25, 0.95);
    pub const AURORA_MAGENTA: Self = Self::rgb(0.95, 0.2, 0.55);

    /// Smooth HSV-style rainbow for demos and clear-color animation.
    pub fn from_hue(hue: f32) -> Self {
        let h = hue.rem_euclid(1.0) * 6.0;
        let x = 1.0 - (h % 2.0 - 1.0).abs();
        let (r, g, b) = match h as u32 {
            0 => (1.0, x, 0.0),
            1 => (x, 1.0, 0.0),
            2 => (0.0, 1.0, x),
            3 => (0.0, x, 1.0),
            4 => (x, 0.0, 1.0),
            _ => (1.0, 0.0, x),
        };
        Self::rgb(r, g, b)
    }

    pub fn to_wgpu(self) -> wgpu::Color {
        wgpu::Color {
            r: self.r as f64,
            g: self.g as f64,
            b: self.b as f64,
            a: self.a as f64,
        }
    }

    /// Darken toward night for atmospheric clears.
    pub fn night_blend(self, amount: f32) -> Self {
        let t = amount.clamp(0.0, 1.0);
        Self {
            r: self.r * (1.0 - t) * 0.25 + Color::AURORA_NIGHT.r * t,
            g: self.g * (1.0 - t) * 0.25 + Color::AURORA_NIGHT.g * t,
            b: self.b * (1.0 - t) * 0.25 + Color::AURORA_NIGHT.b * t,
            a: self.a,
        }
    }
}
