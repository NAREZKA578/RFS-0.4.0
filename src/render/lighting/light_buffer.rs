//! The layout of a light in the GPU's light buffer.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! One definition, because the layout is the kind of thing that is obvious
//! once and then forgotten — and when it is forgotten the picture goes black
//! with no message anywhere. The shader reads the intensity out of the
//! padding, every light has an intensity of zero, and the scene is lit by the
//! ambient term alone: a light that is present in the file and contributes
//! nothing.
//!
//! The layout is the one WGSL gives the struct
//!
//! ```wgsl
//! struct Light {
//!     position: vec3<f32>,
//!     color: vec3<f32>,
//!     intensity: f32,
//!     radius: f32,
//! };
//! ```
//!
//! A `vec3` inside a struct is **16-aligned and 12 bytes long**, so:
//!
//! - `position` is at byte 0, `color` at byte 16 (the next 16-aligned offset),
//! - and the two floats after them are at bytes **28 and 32** — not 32 and 36.
//!
//! The last part is the one that is easy to get wrong. A member is placed at
//! the next offset that satisfies **its own** alignment, and `f32`'s alignment
//! is 4, not the struct's 16. So `intensity` goes at 28, right after `color`
//! ends, and `radius` at 32. The struct is 36 bytes, which rounds up to its
//! 16-byte alignment to 48 — and the array's stride is that 48.
//!
//! Packing the fields tightly, as `position, color, intensity, radius` in
//! consecutive floats, is the mistake this module exists to prevent.
//!
//! **The shader and this module have to agree, and the check is not a
//! comment.** The test `light_layout.rs` reads the numbers back *out of the
//! GPU* through a shader that prints them into the colour, and every shader in
//! the repository that declares a `struct Light` has to declare the canonical
//! one — byte for byte. A comment that disagrees with the code is a trap for
//! the next reader, and the repository had one: it claimed the fields were at
//! 0, 16, 32 and 36.

use glam::Vec3;

/// The stride of one light in the buffer, in bytes.
pub const LIGHT_STRIDE_BYTES: u64 = 48;

/// The stride of one light in the buffer, in floats.
pub const LIGHT_STRIDE_FLOATS: usize = 12;

/// The offsets of the light's fields inside one element, in floats.
pub mod offsets {
    /// `position: vec3<f32>` — at 0.
    pub const POSITION: usize = 0;
    /// `color: vec3<f32>` — at 4 (byte 16).
    pub const COLOR: usize = 4;
    /// `intensity: f32` — at 7 (byte 28).
    pub const INTENSITY: usize = 7;
    /// `radius: f32` — at 8 (byte 32).
    pub const RADIUS: usize = 8;
}

/// The canonical WGSL declaration of a light.
///
/// Every shader that reads a light from the buffer declares exactly this, and
/// the test `light_layout.rs` compares the declarations in the repository with
/// it — so a shader that drifts is caught by a test rather than by a picture
/// that comes out black.
pub const LIGHT_STRUCT_WGSL: &str = "struct Light {\n    position: vec3<f32>,\n    color: vec3<f32>,\n    intensity: f32,\n    radius: f32,\n}";

/// Write one light into the buffer's float array.
///
/// `index` is the light's number in the buffer. Returns `false` when the
/// array is too short for that light, and writes nothing in that case — a
/// light written past the end of the array is a light the shader will read
/// from wherever the memory happens to be.
pub fn pack_light(
    data: &mut [f32],
    index: usize,
    position: Vec3,
    color: Vec3,
    intensity: f32,
    radius: f32,
) -> bool {
    let base = index * LIGHT_STRIDE_FLOATS;
    let Some(slice) = data.get_mut(base..base + LIGHT_STRIDE_FLOATS) else {
        return false;
    };
    slice[offsets::POSITION] = position.x;
    slice[offsets::POSITION + 1] = position.y;
    slice[offsets::POSITION + 2] = position.z;
    slice[offsets::COLOR] = color.x;
    slice[offsets::COLOR + 1] = color.y;
    slice[offsets::COLOR + 2] = color.z;
    // The two floats are at 28 and 32, and **not** at 32 and 36.
    //
    // The four bytes are the whole difference between a light that lights
    // the scene and a light whose intensity is read out of the padding: the
    // shader reads the intensity at 28 and the radius at 32, and a writer that
    // puts them four bytes late writes the intensity where the radius is and
    // the radius past the end of the struct. The shader then reads an
    // intensity of zero — a light that is present in the file and contributes
    // nothing to the picture.
    slice[offsets::INTENSITY] = intensity;
    slice[offsets::RADIUS] = radius;
    true
}

/// The buffer for `count` lights, zeroed.
pub fn buffer_for(count: usize) -> Vec<f32> {
    vec![0.0; count * LIGHT_STRIDE_FLOATS]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offsets are the ones WGSL gives the struct, and they are the ones
    /// the shader reads.
    ///
    /// The test is here rather than only in the description because the four
    /// bytes are the whole difference between a working light and a light that
    /// contributes nothing: `vec3` is 16-aligned and 12 bytes long, so the
    /// vector after it starts at 16 and ends at 28, and the two floats after
    /// *that* are placed by their own alignment — 4, not the struct's 16.
    /// They are at 28 and 32, and a writer that puts them at 32 and 36 makes
    /// the shader read the intensity out of the padding.
    #[test]
    fn the_offsets_are_the_ones_wgsl_gives() {
        assert_eq!(offsets::POSITION, 0, "the position is at byte 0");
        assert_eq!(offsets::COLOR, 4, "the colour is at byte 16");
        assert_eq!(offsets::INTENSITY, 7, "the intensity is at byte 28");
        assert_eq!(offsets::RADIUS, 8, "the radius is at byte 32");
        assert_eq!(LIGHT_STRIDE_FLOATS, 12, "the stride is 48 bytes");
    }

    /// The writer puts the numbers where the shader reads them.
    #[test]
    fn the_writer_puts_the_numbers_where_the_shader_reads_them() {
        let mut data = buffer_for(1);
        assert!(pack_light(
            &mut data,
            0,
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(0.1, 0.2, 0.3),
            4.0,
            30.0,
        ));
        assert_eq!(data[offsets::POSITION], 1.0);
        assert_eq!(data[offsets::POSITION + 1], 2.0);
        assert_eq!(data[offsets::POSITION + 2], 3.0);
        assert_eq!(data[offsets::COLOR], 0.1);
        assert_eq!(data[offsets::COLOR + 1], 0.2);
        assert_eq!(data[offsets::COLOR + 2], 0.3);
        assert_eq!(data[offsets::INTENSITY], 4.0);
        assert_eq!(data[offsets::RADIUS], 30.0);
    }

    /// A light past the end of the buffer is not written.
    ///
    /// A light written past the end is a light the shader will read from
    /// wherever the memory happens to be — and the shader will believe it.
    #[test]
    fn a_light_past_the_end_of_the_buffer_is_not_written() {
        let mut data = buffer_for(1);
        assert!(!pack_light(
            &mut data,
            1,
            Vec3::ZERO,
            Vec3::ONE,
            1.0,
            1.0,
        ));
        assert!(data.iter().all(|value| *value == 0.0));
    }
}
