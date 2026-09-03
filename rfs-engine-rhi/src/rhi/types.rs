//! Common types used across the RHI.

use std::fmt;

/// 2D extent (width, height).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

impl Extent2D {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

impl fmt::Display for Extent2D {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// 3D extent (width, height, depth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent3D {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

impl Extent3D {
    pub const fn new(width: u32, height: u32, depth: u32) -> Self {
        Self {
            width,
            height,
            depth,
        }
    }
}

/// 2D offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offset2D {
    pub x: i32,
    pub y: i32,
}

impl Offset2D {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// 3D offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offset3D {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Offset3D {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// Rectangle defined by offset and extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.width as i32
            && py >= self.y
            && py < self.y + self.height as i32
    }
}

/// Color with RGBA components.
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

    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const RED: Self = Self::rgb(1.0, 0.0, 0.0);
    pub const GREEN: Self = Self::rgb(0.0, 1.0, 0.0);
    pub const BLUE: Self = Self::rgb(0.0, 0.0, 1.0);
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);

    pub fn as_array(&self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::WHITE
    }
}

/// Vertex attribute format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexFormat {
    /// Single 32-bit float.
    Float1,
    /// Two 32-bit floats.
    Float2,
    /// Three 32-bit floats.
    Float3,
    /// Four 32-bit floats.
    Float4,
    /// Two 16-bit unsigned integers.
    Uint2,
    /// Four 8-bit unsigned integers.
    Uint4,
    /// Four 8-bit signed normalized integers.
    Snorm4,
}

impl VertexFormat {
    /// Returns the size in bytes of this format.
    pub fn size(&self) -> u32 {
        match self {
            Self::Float1 => 4,
            Self::Float2 => 8,
            Self::Float3 => 12,
            Self::Float4 => 16,
            Self::Uint2 => 4,
            Self::Uint4 => 4,
            Self::Snorm4 => 4,
        }
    }
}

/// Vertex attribute description.
#[derive(Debug, Clone)]
pub struct VertexAttribute {
    /// Attribute name in the shader.
    pub name: String,
    /// Attribute format.
    pub format: VertexFormat,
    /// Byte offset within the vertex.
    pub offset: u32,
    /// Attribute binding location.
    pub location: u32,
}

impl VertexAttribute {
    pub fn new(name: &str, format: VertexFormat, offset: u32, location: u32) -> Self {
        Self {
            name: name.to_string(),
            format,
            offset,
            location,
        }
    }
}

/// Vertex layout description.
#[derive(Debug, Clone, Default)]
pub struct VertexLayout {
    /// Stride between vertices in bytes.
    pub stride: u32,
    /// Vertex attributes.
    pub attributes: Vec<VertexAttribute>,
}

impl VertexLayout {
    pub fn new(stride: u32) -> Self {
        Self {
            stride,
            attributes: Vec::new(),
        }
    }

    pub fn add_attribute(
        mut self,
        name: &str,
        format: VertexFormat,
        offset: u32,
        location: u32,
    ) -> Self {
        self.attributes
            .push(VertexAttribute::new(name, format, offset, location));
        self
    }

    pub fn with_position_2d() -> Self {
        Self::new(8).add_attribute("position", VertexFormat::Float2, 0, 0)
    }

    pub fn with_position_3d() -> Self {
        Self::new(12).add_attribute("position", VertexFormat::Float3, 0, 0)
    }

    pub fn with_position_uv() -> Self {
        Self::new(20)
            .add_attribute("position", VertexFormat::Float3, 0, 0)
            .add_attribute("uv", VertexFormat::Float2, 12, 1)
    }

    pub fn with_position_normal_uv() -> Self {
        Self::new(32)
            .add_attribute("position", VertexFormat::Float3, 0, 0)
            .add_attribute("normal", VertexFormat::Float3, 12, 1)
            .add_attribute("uv", VertexFormat::Float2, 24, 2)
    }

    pub fn with_position_color() -> Self {
        Self::new(28)
            .add_attribute("position", VertexFormat::Float3, 0, 0)
            .add_attribute("color", VertexFormat::Float4, 12, 1)
    }
}
