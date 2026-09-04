//! Procedural art for the platformer demo: a little animated lumen sprite,
//! checkpoint flag, beveled terrain tiles, ferry plating, and clouds.
//!
//! Everything is generated at startup into RGBA buffers — no binary assets,
//! no licenses, deterministic output. The engine stays generic; this module
//! is game-owned presentation.

use aurora_engine::Color;

/// Tiny RGBA canvas with the handful of shape helpers the art needs.
pub struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    fn blend(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let index = ((y as u32 * self.width + x as u32) * 4) as usize;
        let alpha = color.a.clamp(0.0, 1.0);
        let inv = 1.0 - alpha;
        let dst = &mut self.pixels[index..index + 4];
        if dst[3] == 0 {
            dst[0] = (color.r * 255.0).round() as u8;
            dst[1] = (color.g * 255.0).round() as u8;
            dst[2] = (color.b * 255.0).round() as u8;
            dst[3] = (alpha * 255.0).round() as u8;
        } else {
            // Cheap "over" blend in sRGB space; good enough for glow shapes.
            dst[0] = (dst[0] as f32 * inv + color.r * 255.0 * alpha).round() as u8;
            dst[1] = (dst[1] as f32 * inv + color.g * 255.0 * alpha).round() as u8;
            dst[2] = (dst[2] as f32 * inv + color.b * 255.0 * alpha).round() as u8;
            dst[3] = ((dst[3] as f32 / 255.0 + alpha).min(1.0) * 255.0).round() as u8;
        }
    }

    pub fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.blend(x, y, color);
            }
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        let r2 = radius * radius;
        let x0 = (cx - radius - 1.0).floor() as i32;
        let x1 = (cx + radius + 1.0).ceil() as i32;
        let y0 = (cy - radius - 1.0).floor() as i32;
        let y1 = (cy + radius + 1.0).ceil() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let distance2 = dx * dx + dy * dy;
                if distance2 <= r2 {
                    // 1px feather on the rim keeps circles from aliasing hard.
                    let edge = (r2 - distance2).sqrt() / (radius.max(1.0));
                    let coverage = (edge * 4.0).clamp(0.0, 1.0);
                    self.blend(
                        x,
                        y,
                        Color::rgba(color.r, color.g, color.b, color.a * coverage),
                    );
                }
            }
        }
    }

    /// Vertical rounded capsule between two heights with a given half width.
    fn fill_capsule(&mut self, cx: f32, top: f32, bottom: f32, half_width: f32, color: Color) {
        self.fill_rect(
            (cx - half_width).floor() as i32,
            (top + half_width).floor() as i32,
            (cx + half_width).ceil() as i32,
            (bottom - half_width).ceil() as i32,
            color,
        );
        self.fill_circle(cx, top + half_width, half_width, color);
        self.fill_circle(cx, bottom - half_width, half_width, color);
    }
}

const BODY: Color = Color::rgb(0.96, 0.8, 0.42);
const BODY_DARK: Color = Color::rgb(0.72, 0.55, 0.22);
const BELLY: Color = Color::rgb(1.0, 0.93, 0.7);
const EYE_WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
const EYE_PUPIL: Color = Color::rgb(0.12, 0.16, 0.24);
const TEAL: Color = Color::rgb(0.18, 0.85, 0.72);
const OUTLINE: Color = Color::rgb(0.2, 0.24, 0.38);

/// Draws one frame of the lumen sprite into a 64x64 cell, origin bottom-center
/// of the body at (32, 54). `pose` drives the animation: run phase, air state.
fn draw_character(canvas: &mut Canvas, pose: CharacterPose) {
    let CharacterPose {
        run_phase,
        airborne,
        rising,
    } = pose;

    let cx = 32.0_f32;
    let ground = 54.0_f32;
    let bob = if airborne {
        0.0
    } else {
        (run_phase * std::f32::consts::TAU * 2.0).sin() * 1.5
    };
    let lean = if airborne {
        if rising {
            -3.0
        } else {
            3.0
        }
    } else {
        (run_phase * std::f32::consts::TAU).cos() * 2.5
    };

    let body_top = ground - 34.0 + bob;
    let body_bottom = ground - 8.0 + bob;
    let half_width = 13.0;

    // Feet: swinging stubs during run, tucked when rising, splayed when falling.
    let stride = if airborne {
        0.0
    } else {
        (run_phase * std::f32::consts::TAU).sin() * 7.0
    };
    let foot_y = ground - 4.0;
    let (foot1_dx, foot2_dx, foot_dy) = if airborne {
        if rising {
            (3.0, -3.0, -3.0)
        } else {
            (6.0, -6.0, 1.0)
        }
    } else {
        (stride, -stride, 0.0)
    };
    for (dx, dy) in [(foot1_dx, foot_dy), (foot2_dx, foot_dy)] {
        canvas.fill_circle(
            cx + dx + lean * 0.3,
            foot_y + dy,
            4.5,
            Color::rgb(BODY_DARK.r, BODY_DARK.g, BODY_DARK.b),
        );
    }

    // Body capsule with a thin dark outline (drawn as a slightly bigger
    // capsule underneath).
    canvas.fill_capsule(
        cx,
        body_top - 1.5,
        body_bottom + 1.5,
        half_width + 1.5,
        OUTLINE,
    );
    canvas.fill_capsule(cx, body_top, body_bottom, half_width, BODY);
    // Belly patch.
    canvas.fill_capsule(
        cx + lean * 0.2,
        body_top + 12.0,
        body_bottom - 3.0,
        half_width - 4.5,
        BELLY,
    );

    // Eyes sit toward the facing side (sprite faces right); pupils track.
    let eye_y = body_top + 11.0 + bob;
    let eye_dx = 4.5 + lean * 0.35;
    for sign in [-1.0_f32, 1.0_f32] {
        let ex = cx + lean * 0.35 + sign * eye_dx + 1.5;
        canvas.fill_circle(ex, eye_y, 3.4, EYE_WHITE);
        let pupil_shift = if airborne {
            if rising {
                -0.8
            } else {
                0.8
            }
        } else {
            0.0
        };
        canvas.fill_circle(ex + 1.0, eye_y + pupil_shift, 1.7, EYE_PUPIL);
    }

    // Cheek dots for warmth.
    for sign in [-1.0_f32, 1.0_f32] {
        canvas.fill_circle(
            cx + lean * 0.35 + sign * 8.5,
            eye_y + 6.0,
            1.6,
            Color::rgba(0.95, 0.55, 0.45, 0.7),
        );
    }

    // Antenna with a teal bulb that lags the motion.
    let stem_top = body_top + bob;
    canvas.fill_rect(
        (cx - 1.0) as i32,
        (stem_top - 9.0) as i32,
        (cx + 1.0) as i32,
        (stem_top + 2.0) as i32,
        BODY_DARK,
    );
    let bulb_sway = if airborne {
        -lean * 0.8
    } else {
        -(run_phase * std::f32::consts::TAU * 2.0).sin() * 2.0
    };
    canvas.fill_circle(cx + bulb_sway, stem_top - 11.0, 3.4, TEAL);
}

/// One animation frame description.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterPose {
    /// Run cycle phase in `[0, 1)`; ignored while airborne.
    pub run_phase: f32,
    pub airborne: bool,
    /// Airborne and moving up (jump) vs down (fall).
    pub rising: bool,
}

/// Frame order: [idle 0, idle 1, run 0..=5, jump, fall].
pub const CHARACTER_FRAMES: usize = 10;
const CHARACTER_CELL: u32 = 64;

pub fn character_frame(pose: CharacterPose) -> usize {
    if pose.airborne {
        if pose.rising {
            8
        } else {
            9
        }
    } else if pose.run_phase > 0.0 {
        2 + (pose.run_phase * 6.0).floor() as usize % 6
    } else {
        ((pose.run_phase * 2.0).floor() as usize) % 2
    }
}

/// Builds the character strip: `CHARACTER_FRAMES` cells of 64x64 laid out
/// horizontally. Returns (pixels, width, height, cell_width).
pub fn character_strip() -> (Vec<u8>, u32, u32, u32) {
    let mut canvas = Canvas::new(CHARACTER_CELL * CHARACTER_FRAMES as u32, CHARACTER_CELL);
    for frame in 0..CHARACTER_FRAMES {
        let pose = match frame {
            0 => CharacterPose {
                run_phase: 0.0,
                airborne: false,
                rising: false,
            },
            1 => CharacterPose {
                run_phase: 0.5,
                airborne: false,
                rising: false,
            },
            2..=7 => CharacterPose {
                run_phase: (frame - 2) as f32 / 6.0,
                airborne: false,
                rising: false,
            },
            8 => CharacterPose {
                run_phase: 0.0,
                airborne: true,
                rising: true,
            },
            _ => CharacterPose {
                run_phase: 0.0,
                airborne: true,
                rising: false,
            },
        };
        let mut cell = Canvas::new(CHARACTER_CELL, CHARACTER_CELL);
        draw_character(&mut cell, pose);
        let pixels = cell.into_pixels();
        let x0 = (frame as u32 * CHARACTER_CELL) as usize;
        for y in 0..CHARACTER_CELL as usize {
            let src = y * CHARACTER_CELL as usize * 4;
            let dst = (y * canvas.width as usize + x0) * 4;
            canvas.pixels[dst..dst + CHARACTER_CELL as usize * 4]
                .copy_from_slice(&pixels[src..src + CHARACTER_CELL as usize * 4]);
        }
    }
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h, CHARACTER_CELL)
}

/// Checkpoint flag: gray pole, amber waving cloth. Two frames.
pub fn flag_pair() -> (Vec<u8>, u32, u32, u32) {
    const CELL: u32 = 48;
    let mut canvas = Canvas::new(CELL * 2, 72);
    for frame in 0..2 {
        let mut cell = Canvas::new(CELL, 72);
        // Pole.
        cell.fill_rect(10, 6, 13, 66, Color::rgb(0.62, 0.66, 0.75));
        cell.fill_rect(10, 6, 11, 66, Color::rgb(0.4, 0.44, 0.55));
        // Base nub.
        cell.fill_circle(11.5, 65.0, 5.0, Color::rgb(0.35, 0.4, 0.52));
        // Waving cloth: two horizontal bands with per-column wave offset.
        let phase = frame as f32 * std::f32::consts::PI * 0.5;
        for column in 0..30 {
            let t = column as f32 / 30.0;
            let wave = (t * 4.0 + phase).sin() * 2.5 * t;
            let x = 13 + column;
            let top = 10.0 + wave;
            let bottom = 34.0 + wave * 1.2 + t * 4.0;
            let fade = 1.0 - t * 0.35;
            cell.fill_rect(
                x,
                top as i32,
                x + 1,
                bottom as i32,
                Color::rgba(1.0, 0.72, 0.25, fade),
            );
            cell.fill_rect(
                x,
                (top + 4.0) as i32,
                x + 1,
                (bottom - 4.0) as i32,
                Color::rgba(1.0, 0.85, 0.45, fade * 0.9),
            );
        }
        let pixels = cell.into_pixels();
        let x0 = (frame as u32 * CELL) as usize;
        for y in 0..72_usize {
            let src = y * CELL as usize * 4;
            let dst = (y * canvas.width as usize + x0) * 4;
            canvas.pixels[dst..dst + CELL as usize * 4]
                .copy_from_slice(&pixels[src..src + CELL as usize * 4]);
        }
    }
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h, CELL)
}

/// Hostile walker: squat violet blob with angry eyes, two wobble frames.
pub fn walker_pair() -> (Vec<u8>, u32, u32, u32) {
    const CELL: u32 = 48;
    let mut canvas = Canvas::new(CELL * 2, CELL);
    for frame in 0..2 {
        let mut cell = Canvas::new(CELL, CELL);
        let body = Color::rgb(0.5, 0.32, 0.72);
        let body_dark = Color::rgb(0.3, 0.18, 0.48);
        let cx = 24.0_f32;
        let ground = 40.0_f32;
        let wobble = if frame == 0 { -1.5 } else { 1.5 };
        // Feet.
        for sign in [-1.0_f32, 1.0_f32] {
            cell.fill_circle(cx + sign * 9.0, ground - 3.0 + wobble * 0.5, 4.0, body_dark);
        }
        // Squat body.
        cell.fill_capsule(cx, ground - 30.0 + wobble, ground - 6.0, 13.0, OUTLINE);
        cell.fill_capsule(cx, ground - 28.5 + wobble, ground - 7.5, 11.5, body);
        // Brow ridge: angry tilt.
        cell.fill_rect(
            (cx - 10.0) as i32,
            (ground - 24.0 + wobble) as i32,
            (cx + 10.0) as i32,
            (ground - 21.0 + wobble) as i32,
            body_dark,
        );
        // Eyes with glower.
        for sign in [-1.0_f32, 1.0_f32] {
            cell.fill_circle(cx + sign * 5.0, ground - 17.0 + wobble, 2.6, EYE_WHITE);
            cell.fill_circle(cx + sign * 5.0, ground - 16.4 + wobble, 1.3, EYE_PUPIL);
        }
        // Zigzag mouth.
        for step in 0..3 {
            let x = (cx - 5.0 + step as f32 * 4.0) as i32;
            let y = (ground - 11.5 + wobble) as i32;
            cell.fill_rect(x, y, x + 2, y + 2, body_dark);
            cell.fill_rect(x + 2, y + 2, x + 4, y + 4, body_dark);
        }
        let pixels = cell.into_pixels();
        let x0 = (frame as u32 * CELL) as usize;
        for y in 0..CELL as usize {
            let src = y * CELL as usize * 4;
            let dst = (y * canvas.width as usize + x0) * 4;
            canvas.pixels[dst..dst + CELL as usize * 4]
                .copy_from_slice(&pixels[src..src + CELL as usize * 4]);
        }
    }
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h, CELL)
}

/// Spike strip tile, 64x24: three gray steel triangles on a base bar.
pub fn spike_tile() -> (Vec<u8>, u32, u32) {
    let mut canvas = Canvas::new(64, 24);
    let base = Color::rgb(0.24, 0.28, 0.4);
    let steel = Color::rgb(0.62, 0.68, 0.8);
    let steel_dark = Color::rgb(0.4, 0.45, 0.58);
    canvas.fill_rect(0, 20, 64, 24, base);
    for spike in 0..4 {
        let base_x = spike * 16;
        for row in 0..18 {
            let t = row as f32 / 18.0;
            let half = (8.0 * (1.0 - t)) as i32;
            let y = 20 - row;
            let color = if row % 4 < 2 { steel } else { steel_dark };
            canvas.fill_rect(base_x + 8 - half, y, base_x + 8 + half, y + 1, color);
        }
    }
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h)
}

/// Rounded panel tile for nine-slice UI: 48x48, 12px corner radius,
/// 2px teal border, translucent fill. Corners/edges/center are sliced.
pub fn panel9_tile() -> (Vec<u8>, u32, u32) {
    let mut canvas = Canvas::new(48, 48);
    let fill = Color::rgba(0.04, 0.06, 0.16, 0.88);
    let border = Color::rgb(0.18, 0.85, 0.72);
    // Fill whole area first.
    canvas.fill_rect(0, 0, 48, 48, fill);
    // Carve the rounded corners by blending over with clear-ish dark: we
    // emulate transparency outside the radius using low-alpha background
    // tone rather than true alpha erase (sprites blend additively enough).
    let radius = 12.0_f32;
    let corners = [
        (radius, radius),
        (47.0 - radius, radius),
        (radius, 47.0 - radius),
        (47.0 - radius, 47.0 - radius),
    ];
    for (cx, cy) in corners {
        // Dim the outside-of-radius region toward the page background tone.
        for y in 0..48_i32 {
            for x in 0..48_i32 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let in_quadrant_x = (cx < 24.0 && x < 12) || (cx >= 24.0 && x >= 36);
                let in_quadrant_y = (cy < 24.0 && y < 12) || (cy >= 24.0 && y >= 36);
                if in_quadrant_x && in_quadrant_y && dx * dx + dy * dy > radius * radius {
                    canvas.blend(x, y, Color::rgba(0.0, 0.0, 0.0, 0.0));
                }
            }
        }
    }
    // Border: 2px inset ring (straight edges only; corners approximate).
    canvas.fill_rect(0, 0, 48, 2, border);
    canvas.fill_rect(0, 46, 48, 48, border);
    canvas.fill_rect(0, 0, 2, 48, border);
    canvas.fill_rect(46, 0, 48, 48, border);
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h)
}

/// Beveled stone tile, 64x64, reads as terrain when tiled.
pub fn stone_tile() -> (Vec<u8>, u32, u32) {
    let mut canvas = Canvas::new(64, 64);
    let base = Color::rgb(0.16, 0.2, 0.32);
    let top_light = Color::rgba(0.42, 0.52, 0.75, 0.9);
    let left_light = Color::rgba(0.32, 0.4, 0.62, 0.7);
    let bottom_dark = Color::rgba(0.08, 0.1, 0.18, 0.9);
    let right_dark = Color::rgba(0.1, 0.13, 0.22, 0.8);
    canvas.fill_rect(0, 0, 64, 64, base);
    canvas.fill_rect(0, 0, 64, 3, top_light);
    canvas.fill_rect(0, 0, 3, 64, left_light);
    canvas.fill_rect(0, 61, 64, 64, bottom_dark);
    canvas.fill_rect(61, 0, 64, 64, right_dark);
    // Sparse mineral speckles, deterministic.
    let speckles = [(14, 22), (40, 12), (52, 38), (24, 46), (34, 30)];
    for (x, y) in speckles {
        canvas.fill_circle(x as f32, y as f32, 1.8, Color::rgba(0.55, 0.65, 0.9, 0.35));
    }
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h)
}

/// Grass-capped ledge tile, 64x20.
pub fn ledge_tile() -> (Vec<u8>, u32, u32) {
    let mut canvas = Canvas::new(64, 20);
    let soil = Color::rgb(0.2, 0.26, 0.4);
    let grass = Color::rgb(0.32, 0.78, 0.52);
    let grass_dark = Color::rgb(0.22, 0.6, 0.4);
    canvas.fill_rect(0, 6, 64, 20, soil);
    canvas.fill_rect(0, 2, 64, 8, grass);
    canvas.fill_rect(0, 2, 64, 3, Color::rgba(0.55, 0.95, 0.7, 0.9));
    // Grass fringe hangs below the cap.
    for column in (0..64).step_by(4) {
        let depth = 8 + ((column * 7) % 5);
        canvas.fill_rect(column, 8, column + 2, depth, grass_dark);
    }
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h)
}

/// Ferry plating, 96x26: violet metal with rivets and a top highlight.
pub fn ferry_tile() -> (Vec<u8>, u32, u32) {
    let mut canvas = Canvas::new(96, 26);
    let plate = Color::rgb(0.42, 0.32, 0.6);
    canvas.fill_rect(0, 0, 96, 26, plate);
    canvas.fill_rect(0, 0, 96, 3, Color::rgba(0.72, 0.6, 0.95, 0.9));
    canvas.fill_rect(0, 23, 96, 26, Color::rgba(0.18, 0.13, 0.3, 0.9));
    for x in [8, 88] {
        for y in [8, 18] {
            canvas.fill_circle(x as f32, y as f32, 2.2, Color::rgb(0.75, 0.68, 0.9));
            canvas.fill_circle(
                x as f32 + 0.6,
                y as f32 + 0.6,
                1.1,
                Color::rgb(0.3, 0.22, 0.45),
            );
        }
    }
    canvas.fill_rect(46, 6, 50, 20, Color::rgba(0.2, 0.85, 0.75, 0.55));
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h)
}

/// Soft cloud puff, 160x64.
pub fn cloud() -> (Vec<u8>, u32, u32) {
    let mut canvas = Canvas::new(160, 64);
    let puff = Color::rgba(0.75, 0.82, 1.0, 0.16);
    let core = Color::rgba(0.85, 0.9, 1.0, 0.12);
    for (cx, cy, r) in [
        (40.0, 40.0, 20.0),
        (76.0, 32.0, 26.0),
        (116.0, 42.0, 20.0),
        (58.0, 44.0, 18.0),
        (98.0, 46.0, 17.0),
    ] {
        canvas.fill_circle(cx, cy, r, puff);
    }
    for (cx, cy, r) in [(70.0, 38.0, 16.0), (92.0, 40.0, 14.0)] {
        canvas.fill_circle(cx, cy, r, core);
    }
    let (w, h) = canvas.size();
    (canvas.into_pixels(), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_canvases_have_opaque_enough_content() {
        let (pixels, w, h, cell) = character_strip();
        assert_eq!(w, CHARACTER_CELL * CHARACTER_FRAMES as u32);
        assert_eq!(h, CHARACTER_CELL);
        assert_eq!(cell, CHARACTER_CELL);
        assert!(pixels.iter().any(|&byte| byte != 0), "character has ink");

        let (pixels, w, h, cell) = flag_pair();
        assert_eq!(w, 96);
        assert_eq!(h, 72);
        assert_eq!(cell, 48);
        assert!(pixels.iter().any(|&byte| byte != 0), "flag has ink");

        let (pixels, w, _h, cell) = walker_pair();
        assert_eq!(w, 96);
        assert_eq!(cell, 48);
        assert!(pixels.iter().any(|&byte| byte != 0), "walker has ink");

        for (name, output) in [
            ("stone", stone_tile()),
            ("ledge", ledge_tile()),
            ("ferry", ferry_tile()),
            ("cloud", cloud()),
            ("spike", spike_tile()),
            ("panel9", panel9_tile()),
        ] {
            let (pixels, w, h) = output;
            assert!(w > 0 && h > 0, "{name} has size");
            assert_eq!(pixels.len(), (w * h * 4) as usize, "{name} buffer size");
            assert!(pixels.iter().any(|&byte| byte != 0), "{name} has ink");
        }
    }

    #[test]
    fn frame_selector_covers_the_strip() {
        assert_eq!(
            character_frame(CharacterPose {
                run_phase: 0.0,
                airborne: false,
                rising: false
            }),
            0
        );
        assert_eq!(
            character_frame(CharacterPose {
                run_phase: 0.2,
                airborne: false,
                rising: false
            }),
            3
        );
        assert_eq!(
            character_frame(CharacterPose {
                run_phase: 0.99,
                airborne: false,
                rising: false
            }),
            7
        );
        assert_eq!(
            character_frame(CharacterPose {
                run_phase: 0.0,
                airborne: true,
                rising: true
            }),
            8
        );
        assert_eq!(
            character_frame(CharacterPose {
                run_phase: 0.0,
                airborne: true,
                rising: false
            }),
            9
        );
    }
}
