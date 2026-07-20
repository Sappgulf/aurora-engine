//! Texture atlases and frame animations.

use glam::Vec2;

use crate::renderer::TextureHandle;
use crate::sprite::Sprite;

/// Grid atlas over a texture (row-major frames).
#[derive(Debug, Clone)]
pub struct TextureAtlas {
    /// Texture handle in the renderer.
    pub texture: TextureHandle,
    pub columns: u32,
    pub rows: u32,
    /// Full texture pixel size (for docs / tooling).
    pub texture_size: Vec2,
}

impl TextureAtlas {
    pub fn new(texture: TextureHandle, columns: u32, rows: u32, texture_size: Vec2) -> Self {
        Self {
            texture,
            columns: columns.max(1),
            rows: rows.max(1),
            texture_size,
        }
    }

    pub fn frame_count(&self) -> u32 {
        self.columns * self.rows
    }

    /// UV min/max for frame index (wraps), using the source image's top row
    /// as atlas row zero. The renderer maps this image-space rectangle onto
    /// the engine's Y-up world quad in `sprite_corner_uvs`.
    pub fn uv_rect(&self, frame: u32) -> (Vec2, Vec2) {
        let count = self.frame_count();
        let frame = frame % count;
        let col = frame % self.columns;
        let row = frame / self.columns;
        let fw = 1.0 / self.columns as f32;
        let fh = 1.0 / self.rows as f32;
        // Image rows top→bottom; V increases down in UV.
        let u0 = col as f32 * fw;
        let v0 = row as f32 * fh;
        let u1 = u0 + fw;
        let v1 = v0 + fh;

        // Keep linear filtering inside the selected frame. This matters for
        // generated strips such as Warden's 6×1 sheet, which intentionally
        // omit gutters to preserve their large source silhouette. A zero or
        // invalid texture size is retained as an explicit compatibility path
        // for callers that only need normalized UVs in tests/tools.
        if self.texture_size.x.is_finite()
            && self.texture_size.y.is_finite()
            && self.texture_size.x > 0.0
            && self.texture_size.y > 0.0
        {
            let inset = Vec2::new(0.5 / self.texture_size.x, 0.5 / self.texture_size.y);
            (
                Vec2::new(u0 + inset.x, v0 + inset.y),
                Vec2::new(u1 - inset.x, v1 - inset.y),
            )
        } else {
            (Vec2::new(u0, v0), Vec2::new(u1, v1))
        }
    }

    pub fn apply_frame(&self, sprite: &mut Sprite, frame: u32) {
        let (min, max) = self.uv_rect(frame);
        sprite.uv_min = min;
        sprite.uv_max = max;
    }

    pub fn sprite(&self, position: Vec2, size: Vec2, frame: u32) -> Sprite {
        let (uv_min, uv_max) = self.uv_rect(frame);
        let mut s = Sprite::new(position, size);
        s.uv_min = uv_min;
        s.uv_max = uv_max;
        s
    }
}

/// Plays a sequence of atlas frames over time.
#[derive(Debug, Clone)]
pub struct Animation {
    pub frames: Vec<u32>,
    pub fps: f32,
    pub looping: bool,
    time: f32,
    finished: bool,
}

/// A reusable animation clip. Atlases are shared; each rendered entity owns an
/// [`AnimationPlayer`] so squads do not advance in artificial lockstep.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationClip {
    pub name: String,
    pub frames: Vec<u32>,
    pub fps: f32,
    pub looping: bool,
}

impl AnimationClip {
    pub fn looping(name: impl Into<String>, frames: impl Into<Vec<u32>>, fps: f32) -> Self {
        Self {
            name: name.into(),
            frames: frames.into(),
            fps: fps.max(0.01),
            looping: true,
        }
    }

    pub fn once(name: impl Into<String>, frames: impl Into<Vec<u32>>, fps: f32) -> Self {
        let mut clip = Self::looping(name, frames, fps);
        clip.looping = false;
        clip
    }
}

/// Per-entity playback state for switching between named animation clips.
#[derive(Debug, Clone, Default)]
pub struct AnimationPlayer {
    clip: Option<AnimationClip>,
    time: f32,
    finished: bool,
}

impl AnimationPlayer {
    /// Return to an unanimated state so a one-shot clip can be replayed later.
    pub fn clear(&mut self) {
        self.clip = None;
        self.time = 0.0;
        self.finished = false;
    }

    pub fn play(&mut self, clip: AnimationClip) {
        if self
            .clip
            .as_ref()
            .is_some_and(|active| active.name == clip.name)
        {
            return;
        }
        self.clip = Some(clip);
        self.time = 0.0;
        self.finished = false;
    }

    pub fn tick(&mut self, dt: f32) {
        let Some(clip) = &self.clip else { return };
        if clip.frames.is_empty() || self.finished {
            return;
        }
        self.time += dt.max(0.0);
        let duration = clip.frames.len() as f32 / clip.fps;
        if self.time >= duration {
            if clip.looping {
                self.time %= duration;
            } else {
                self.time = (duration - 1e-4).max(0.0);
                self.finished = true;
            }
        }
    }

    pub fn frame(&self) -> u32 {
        let Some(clip) = &self.clip else { return 0 };
        if clip.frames.is_empty() {
            return 0;
        }
        let index = (self.time * clip.fps).floor() as usize;
        clip.frames[index.min(clip.frames.len() - 1)]
    }

    pub fn clip_name(&self) -> Option<&str> {
        self.clip.as_ref().map(|clip| clip.name.as_str())
    }

    pub fn finished(&self) -> bool {
        self.finished
    }
}

impl Animation {
    pub fn new(frames: impl Into<Vec<u32>>, fps: f32) -> Self {
        Self {
            frames: frames.into(),
            fps: fps.max(0.01),
            looping: true,
            time: 0.0,
            finished: false,
        }
    }

    pub fn once(frames: impl Into<Vec<u32>>, fps: f32) -> Self {
        let mut a = Self::new(frames, fps);
        a.looping = false;
        a
    }

    pub fn tick(&mut self, dt: f32) {
        if self.frames.is_empty() || self.finished {
            return;
        }
        self.time += dt;
        let duration = self.frames.len() as f32 / self.fps;
        if self.time >= duration {
            if self.looping {
                self.time %= duration;
            } else {
                self.time = duration - 1e-4;
                self.finished = true;
            }
        }
    }

    pub fn frame(&self) -> u32 {
        if self.frames.is_empty() {
            return 0;
        }
        let idx = (self.time * self.fps).floor() as usize;
        self.frames[idx.min(self.frames.len() - 1)]
    }

    pub fn reset(&mut self) {
        self.time = 0.0;
        self.finished = false;
    }

    pub fn finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_wraps_frames_and_animation_finishes_once() {
        let atlas = TextureAtlas::new(TextureHandle::default(), 2, 2, Vec2::splat(64.0));
        assert_eq!(atlas.uv_rect(5), atlas.uv_rect(1));
        let mut animation = Animation::once(vec![3, 4], 10.0);
        animation.tick(1.0);
        assert!(animation.finished());
        assert_eq!(animation.frame(), 4);
    }

    #[test]
    fn atlas_rows_are_top_origin_before_world_uv_mapping() {
        let atlas = TextureAtlas::new(TextureHandle::default(), 2, 2, Vec2::splat(64.0));
        let inset = 0.5 / 64.0;
        assert_eq!(
            atlas.uv_rect(0),
            (Vec2::new(inset, inset), Vec2::new(0.5 - inset, 0.5 - inset))
        );
        assert_eq!(
            atlas.uv_rect(2),
            (
                Vec2::new(inset, 0.5 + inset),
                Vec2::new(0.5 - inset, 1.0 - inset)
            )
        );
    }

    #[test]
    fn atlas_uvs_inset_each_frame_to_prevent_linear_bleed() {
        let atlas = TextureAtlas::new(TextureHandle::default(), 6, 1, Vec2::new(2172.0, 724.0));
        let (first_min, first_max) = atlas.uv_rect(0);
        let (second_min, second_max) = atlas.uv_rect(1);
        let texel_x = 0.5 / 2172.0;
        assert_eq!(first_min.x, texel_x);
        assert_eq!(first_max.x, 1.0 / 6.0 - texel_x);
        assert_eq!(second_min.x, 1.0 / 6.0 + texel_x);
        assert_eq!(second_max.x, 2.0 / 6.0 - texel_x);
        assert!(first_max.x < second_min.x);
    }

    #[test]
    fn animation_players_switch_clips_without_restarting_the_active_clip() {
        let idle = AnimationClip::looping("idle", [0, 1], 2.0);
        let move_clip = AnimationClip::looping("move", [4, 5, 6], 3.0);
        let mut player = AnimationPlayer::default();
        player.play(idle.clone());
        player.tick(0.6);
        assert_eq!(player.frame(), 1);
        player.play(idle);
        assert_eq!(player.frame(), 1);
        player.play(move_clip);
        assert_eq!(player.clip_name(), Some("move"));
        assert_eq!(player.frame(), 4);
    }

    #[test]
    fn animation_player_can_replay_a_cleared_one_shot() {
        let mut player = AnimationPlayer::default();
        player.play(AnimationClip::once("hit", [0, 1, 2, 3], 12.0));
        player.tick(1.0);
        assert!(player.finished());
        assert_eq!(player.frame(), 3);

        player.clear();
        player.play(AnimationClip::once("hit", [0, 1, 2, 3], 12.0));
        assert!(!player.finished());
        assert_eq!(player.frame(), 0);
    }
}
