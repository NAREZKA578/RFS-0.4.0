//! Mesh
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::scene::culling::Aabb;
use crate::rhi::{Buffer, BufferUsage};
use glam::{Vec2, Vec3, Vec4};
use std::sync::Arc;

/// Vertex attribute semantic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexAttribute {
    Position,
    Normal,
    Tangent,
    Bitangent,
    TexCoord0,
    TexCoord1,
    TexCoord2,
    TexCoord3,
    Color0,
    Color1,
    Color2,
    Color3,
    BoneIndices,
    BoneWeights,
}

/// Vertex struct
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub tangent: Vec4,
    pub tex_coord: Vec2,
    pub color: Vec4,
}

impl Vertex {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            normal: Vec3::Z,
            tangent: Vec4::new(1.0, 0.0, 0.0, 1.0),
            tex_coord: Vec2::ZERO,
            color: Vec4::ONE,
        }
    }

    pub fn with_normal(mut self, normal: Vec3) -> Self {
        self.normal = normal;
        self
    }

    pub fn with_tangent(mut self, tangent: Vec4) -> Self {
        self.tangent = tangent;
        self
    }

    pub fn with_tex_coord(mut self, tex_coord: Vec2) -> Self {
        self.tex_coord = tex_coord;
        self
    }

    pub fn with_color(mut self, color: Vec4) -> Self {
        self.color = color;
        self
    }
}

impl Default for Vertex {
    fn default() -> Self {
        Self::new(Vec3::ZERO)
    }
}

/// Index type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum IndexType {
    U8,
    U16,
    #[default]
    U32,
}

impl IndexType {
    pub fn size(&self) -> usize {
        match self {
            IndexType::U8 => 1,
            IndexType::U16 => 2,
            IndexType::U32 => 4,
        }
    }
}


/// Primitive type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum PrimitiveType {
    Points,
    Lines,
    LineStrip,
    #[default]
    Triangles,
    TriangleStrip,
    TriangleFan,
}


/// Mesh flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshFlags {
    pub has_positions: bool,
    pub has_normals: bool,
    pub has_tangents: bool,
    pub has_tex_coords: bool,
    pub has_colors: bool,
    pub is_indexed: bool,
    pub is_static: bool,
    pub is_skinned: bool,
}

impl Default for MeshFlags {
    fn default() -> Self {
        Self {
            has_positions: true,
            has_normals: false,
            has_tangents: false,
            has_tex_coords: false,
            has_colors: false,
            is_indexed: false,
            is_static: true,
            is_skinned: false,
        }
    }
}

/// Mesh struct
#[derive(Debug, Clone)]
pub struct Mesh {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub vertex_buffer: Option<Arc<Buffer>>,
    pub index_buffer: Option<Arc<Buffer>>,
    pub vertex_count: u32,
    pub index_count: u32,
    pub bounding_box: Aabb,
    pub bounding_sphere_radius: f32,
    pub primitive_type: PrimitiveType,
    pub index_type: IndexType,
    pub flags: MeshFlags,
}

impl Mesh {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            vertices: Vec::new(),
            indices: Vec::new(),
            vertex_buffer: None,
            index_buffer: None,
            vertex_count: 0,
            index_count: 0,
            bounding_box: Aabb::new(Vec3::ZERO, Vec3::ZERO),
            bounding_sphere_radius: 0.0,
            primitive_type: PrimitiveType::Triangles,
            index_type: IndexType::U32,
            flags: MeshFlags::default(),
        }
    }

    pub fn with_vertices(mut self, vertices: Vec<Vertex>) -> Self {
        let vertex_count = vertices.len() as u32;
        self.vertices = vertices;
        self.vertex_count = vertex_count;
        self.update_bounding_box();
        self
    }

    pub fn with_indices(mut self, indices: Vec<u32>) -> Self {
        let index_count = indices.len() as u32;
        self.indices = indices;
        self.index_count = index_count;
        self.flags.is_indexed = true;
        self
    }

    pub fn with_primitive_type(mut self, primitive_type: PrimitiveType) -> Self {
        self.primitive_type = primitive_type;
        self
    }

    pub fn with_index_type(mut self, index_type: IndexType) -> Self {
        self.index_type = index_type;
        self
    }

    /// Update bounding box based on vertices
    fn update_bounding_box(&mut self) {
        if self.vertices.is_empty() {
            self.bounding_box = Aabb::new(Vec3::ZERO, Vec3::ZERO);
            self.bounding_sphere_radius = 0.0;
            return;
        }

        let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

        for vertex in &self.vertices {
            min = min.min(vertex.position);
            max = max.max(vertex.position);
        }

        self.bounding_box = Aabb::new(min, max);
        self.bounding_sphere_radius = (max - min).length() / 2.0;
    }

    /// Create the GPU vertex buffer and upload the geometry.
    ///
    /// This used to take the device as `_device` and ignore it, building a
    /// CPU-only `Buffer` that had no GPU backing and never received the vertex
    /// data. The result looked like a live buffer — it reported the right size
    /// — while `has_gpu_backing()` was false and any `BindVertexBuffers` would
    /// have bound handle 0. Geometry therefore never reached the GPU at all.
    pub fn create_vertex_buffer(&mut self, device: &crate::rhi::Device) -> crate::rhi::RhiResult<()> {
        if self.vertices.is_empty() {
            return Ok(());
        }

        let size = (std::mem::size_of::<Vertex>() * self.vertices.len()) as u64;
        let buffer = device.create_buffer(
            size,
            BufferUsage::VERTEX | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            false,
        );
        // Vertex data is written once, so a one-shot staging upload is enough;
        // a persistently mapped buffer would only add a flush to maintain.
        // A failed upload must not leave a buffer installed: the draw would then
        // bind a buffer holding nothing and the mesh would look loaded.
        device.upload_buffer(&buffer, &self.vertices)?;
        debug_assert!(
            buffer.has_gpu_backing(),
            "vertex buffer must have GPU backing, otherwise a draw binds handle 0"
        );

        self.vertex_buffer = Some(Arc::new(buffer));
        Ok(())
    }

    /// Create the GPU index buffer and upload the indices.
    ///
    /// Same defect as `create_vertex_buffer`: the device was ignored and no
    /// data was uploaded, so an indexed draw had nothing to read.
    ///
    /// The declared index type is widened to `U32` when the mesh does not fit
    /// it. Narrowing used to be done with `as`, so a mesh with more than 65536
    /// vertices carrying `U16` indices was uploaded with wrapped indices: the
    /// buffer was the right size, the draw succeeded, and the geometry was
    /// wrong with nothing to report it.
    pub fn create_index_buffer(&mut self, device: &crate::rhi::Device) -> crate::rhi::RhiResult<()> {
        if self.indices.is_empty() {
            return Ok(());
        }

        let largest = self.indices.iter().copied().max().unwrap_or(0);
        let needed = if largest <= u8::MAX as u32 {
            IndexType::U8
        } else if largest <= u16::MAX as u32 {
            IndexType::U16
        } else {
            IndexType::U32
        };
        if needed.size() > self.index_type.size() {
            self.index_type = needed;
        }

        let size = self.index_type.size() as u64 * self.indices.len() as u64;
        let buffer = device.create_buffer(
            size,
            BufferUsage::INDEX | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            false,
        );
        match self.index_type {
            IndexType::U16 => {
                // The pool stores `u32` indices; narrow them to what the
                // format actually holds rather than uploading 4 bytes each.
                // Safe only because the widening check above proved every
                // index fits.
                let narrow: Vec<u16> = self.indices.iter().map(|i| *i as u16).collect();
                device.upload_buffer(&buffer, &narrow)?;
            }
            IndexType::U8 => {
                let narrow: Vec<u8> = self.indices.iter().map(|i| *i as u8).collect();
                device.upload_buffer(&buffer, &narrow)?;
            }
            IndexType::U32 => device.upload_buffer(&buffer, &self.indices)?,
        }
        debug_assert!(
            buffer.has_gpu_backing(),
            "index buffer must have GPU backing, otherwise an indexed draw binds handle 0"
        );

        self.index_buffer = Some(Arc::new(buffer));
        Ok(())
    }

    /// Create buffers (vertex and index)
    pub fn create_buffers(&mut self, device: &crate::rhi::Device) -> crate::rhi::RhiResult<()> {
        self.create_vertex_buffer(device)?;
        self.create_index_buffer(device)
    }

    /// Get vertex buffer
    pub fn vertex_buffer(&self) -> Option<&Arc<Buffer>> {
        self.vertex_buffer.as_ref()
    }

    /// Get index buffer
    pub fn index_buffer(&self) -> Option<&Arc<Buffer>> {
        self.index_buffer.as_ref()
    }

    /// Get bounding box
    pub fn bounding_box(&self) -> Aabb {
        self.bounding_box
    }

    /// Get bounding sphere radius
    pub fn bounding_sphere_radius(&self) -> f32 {
        self.bounding_sphere_radius
    }

    /// Check if mesh has indices
    pub fn has_indices(&self) -> bool {
        self.flags.is_indexed
    }

    /// Get vertex count
    pub fn vertex_count(&self) -> u32 {
        self.vertex_count
    }

    /// Get index count
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    /// Get primitive type
    pub fn primitive_type(&self) -> PrimitiveType {
        self.primitive_type
    }

    /// Get index type
    pub fn index_type(&self) -> IndexType {
        self.index_type
    }

    /// Draw the mesh
    pub fn draw(&self, _encoder: &mut crate::rhi::CommandEncoder) {
    }
}

impl Default for Mesh {
    fn default() -> Self {
        Self::new("default")
    }
}

/// Mesh builder
pub struct MeshBuilder {
    mesh: Mesh,
}

impl MeshBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            mesh: Mesh::new(name),
        }
    }

    pub fn with_vertices(mut self, vertices: Vec<Vertex>) -> Self {
        self.mesh = self.mesh.with_vertices(vertices);
        self
    }

    pub fn with_indices(mut self, indices: Vec<u32>) -> Self {
        self.mesh = self.mesh.with_indices(indices);
        self
    }

    pub fn with_primitive_type(mut self, primitive_type: PrimitiveType) -> Self {
        self.mesh = self.mesh.with_primitive_type(primitive_type);
        self
    }

    pub fn with_index_type(mut self, index_type: IndexType) -> Self {
        self.mesh = self.mesh.with_index_type(index_type);
        self
    }

    pub fn build(self) -> Mesh {
        self.mesh
    }
}

/// Common mesh primitives
impl Mesh {
    /// Create a cube mesh
    pub fn cube(name: &str, size: f32) -> Self {
        let half_size = size / 2.0;

        let vertices = vec![
            // Front face
            Vertex::new(Vec3::new(-half_size, -half_size, half_size))
                .with_normal(Vec3::Z)
                .with_tex_coord(Vec2::new(0.0, 0.0)),
            Vertex::new(Vec3::new(half_size, -half_size, half_size))
                .with_normal(Vec3::Z)
                .with_tex_coord(Vec2::new(1.0, 0.0)),
            Vertex::new(Vec3::new(half_size, half_size, half_size))
                .with_normal(Vec3::Z)
                .with_tex_coord(Vec2::new(1.0, 1.0)),
            Vertex::new(Vec3::new(-half_size, half_size, half_size))
                .with_normal(Vec3::Z)
                .with_tex_coord(Vec2::new(0.0, 1.0)),
            // Back face
            Vertex::new(Vec3::new(-half_size, -half_size, -half_size))
                .with_normal(Vec3::NEG_Z)
                .with_tex_coord(Vec2::new(1.0, 0.0)),
            Vertex::new(Vec3::new(half_size, -half_size, -half_size))
                .with_normal(Vec3::NEG_Z)
                .with_tex_coord(Vec2::new(0.0, 0.0)),
            Vertex::new(Vec3::new(half_size, half_size, -half_size))
                .with_normal(Vec3::NEG_Z)
                .with_tex_coord(Vec2::new(0.0, 1.0)),
            Vertex::new(Vec3::new(-half_size, half_size, -half_size))
                .with_normal(Vec3::NEG_Z)
                .with_tex_coord(Vec2::new(1.0, 1.0)),
            // Top face
            Vertex::new(Vec3::new(-half_size, half_size, half_size))
                .with_normal(Vec3::Y)
                .with_tex_coord(Vec2::new(0.0, 1.0)),
            Vertex::new(Vec3::new(half_size, half_size, half_size))
                .with_normal(Vec3::Y)
                .with_tex_coord(Vec2::new(1.0, 1.0)),
            Vertex::new(Vec3::new(half_size, half_size, -half_size))
                .with_normal(Vec3::Y)
                .with_tex_coord(Vec2::new(1.0, 0.0)),
            Vertex::new(Vec3::new(-half_size, half_size, -half_size))
                .with_normal(Vec3::Y)
                .with_tex_coord(Vec2::new(0.0, 0.0)),
            // Bottom face
            Vertex::new(Vec3::new(-half_size, -half_size, half_size))
                .with_normal(Vec3::NEG_Y)
                .with_tex_coord(Vec2::new(1.0, 0.0)),
            Vertex::new(Vec3::new(half_size, -half_size, half_size))
                .with_normal(Vec3::NEG_Y)
                .with_tex_coord(Vec2::new(0.0, 0.0)),
            Vertex::new(Vec3::new(half_size, -half_size, -half_size))
                .with_normal(Vec3::NEG_Y)
                .with_tex_coord(Vec2::new(0.0, 1.0)),
            Vertex::new(Vec3::new(-half_size, -half_size, -half_size))
                .with_normal(Vec3::NEG_Y)
                .with_tex_coord(Vec2::new(1.0, 1.0)),
            // Right face
            Vertex::new(Vec3::new(half_size, -half_size, half_size))
                .with_normal(Vec3::X)
                .with_tex_coord(Vec2::new(0.0, 0.0)),
            Vertex::new(Vec3::new(half_size, -half_size, -half_size))
                .with_normal(Vec3::X)
                .with_tex_coord(Vec2::new(1.0, 0.0)),
            Vertex::new(Vec3::new(half_size, half_size, -half_size))
                .with_normal(Vec3::X)
                .with_tex_coord(Vec2::new(1.0, 1.0)),
            Vertex::new(Vec3::new(half_size, half_size, half_size))
                .with_normal(Vec3::X)
                .with_tex_coord(Vec2::new(0.0, 1.0)),
            // Left face
            Vertex::new(Vec3::new(-half_size, -half_size, -half_size))
                .with_normal(Vec3::NEG_X)
                .with_tex_coord(Vec2::new(0.0, 0.0)),
            Vertex::new(Vec3::new(-half_size, -half_size, half_size))
                .with_normal(Vec3::NEG_X)
                .with_tex_coord(Vec2::new(1.0, 0.0)),
            Vertex::new(Vec3::new(-half_size, half_size, half_size))
                .with_normal(Vec3::NEG_X)
                .with_tex_coord(Vec2::new(1.0, 1.0)),
            Vertex::new(Vec3::new(-half_size, half_size, -half_size))
                .with_normal(Vec3::NEG_X)
                .with_tex_coord(Vec2::new(0.0, 1.0)),
        ];

        let indices = vec![
            // Front face
            0, 1, 2, 0, 2, 3, // Back face
            4, 6, 5, 4, 7, 6, // Top face
            8, 9, 10, 8, 10, 11, // Bottom face
            12, 14, 13, 12, 15, 14, // Right face
            16, 17, 18, 16, 18, 19, // Left face
            20, 22, 21, 20, 23, 22,
        ];

        MeshBuilder::new(name)
            .with_vertices(vertices)
            .with_indices(indices)
            .build()
    }

    /// Create a sphere mesh
    pub fn sphere(name: &str, radius: f32, stacks: usize, sectors: usize) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for i in 0..=stacks {
            let phi = std::f32::consts::PI * i as f32 / stacks as f32;
            for j in 0..=sectors {
                let theta = 2.0 * std::f32::consts::PI * j as f32 / sectors as f32;

                let x = radius * theta.sin() * phi.cos();
                let y = radius * phi.sin();
                let z = radius * theta.cos() * phi.cos();

                let nx = x / radius;
                let ny = y / radius;
                let nz = z / radius;

                let u = j as f32 / sectors as f32;
                let v = i as f32 / stacks as f32;

                vertices.push(
                    Vertex::new(Vec3::new(x, y, z))
                        .with_normal(Vec3::new(nx, ny, nz))
                        .with_tex_coord(Vec2::new(u, v)),
                );
            }
        }

        for i in 0..stacks {
            for j in 0..sectors {
                let first = i * (sectors + 1) + j;
                let second = first + sectors + 1;

                indices.push(first as u32);
                indices.push(second as u32);
                indices.push((first + 1) as u32);

                indices.push((first + 1) as u32);
                indices.push(second as u32);
                indices.push((second + 1) as u32);
            }
        }

        MeshBuilder::new(name)
            .with_vertices(vertices)
            .with_indices(indices)
            .build()
    }

    /// Create a plane mesh
    pub fn plane(name: &str, width: f32, height: f32) -> Self {
        let half_width = width / 2.0;
        let half_height = height / 2.0;

        let vertices = vec![
            Vertex::new(Vec3::new(-half_width, 0.0, -half_height))
                .with_normal(Vec3::Y)
                .with_tex_coord(Vec2::new(0.0, 0.0)),
            Vertex::new(Vec3::new(half_width, 0.0, -half_height))
                .with_normal(Vec3::Y)
                .with_tex_coord(Vec2::new(1.0, 0.0)),
            Vertex::new(Vec3::new(half_width, 0.0, half_height))
                .with_normal(Vec3::Y)
                .with_tex_coord(Vec2::new(1.0, 1.0)),
            Vertex::new(Vec3::new(-half_width, 0.0, half_height))
                .with_normal(Vec3::Y)
                .with_tex_coord(Vec2::new(0.0, 1.0)),
        ];

        // Bug №186: the winding was 0-1-2 / 0-2-3, whose geometric normal is
        // (v1-v0) x (v2-v0) = -Y — the triangles faced *down* while the vertex
        // normals claimed +Y. With back-face culling the plane was therefore
        // invisible from above, which is the only side anyone looks at it
        // from. The winding is reversed so the geometric normal agrees with the
        // shading normal.
        let indices = vec![0, 2, 1, 0, 3, 2];

        MeshBuilder::new(name)
            .with_vertices(vertices)
            .with_indices(indices)
            .build()
    }

    /// Create a cylinder mesh
    pub fn cylinder(name: &str, radius: f32, height: f32, segments: usize) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        // Top and bottom centers
        vertices.push(Vertex::new(Vec3::new(0.0, height / 2.0, 0.0)).with_normal(Vec3::Y));
        vertices.push(Vertex::new(Vec3::new(0.0, -height / 2.0, 0.0)).with_normal(Vec3::NEG_Y));

        // Side vertices
        for i in 0..=segments {
            let theta = 2.0 * std::f32::consts::PI * i as f32 / segments as f32;
            let x = radius * theta.cos();
            let z = radius * theta.sin();

            // Top vertex
            vertices.push(
                Vertex::new(Vec3::new(x, height / 2.0, z))
                    .with_normal(Vec3::new(x, 0.0, z).normalize()),
            );

            // Bottom vertex
            vertices.push(
                Vertex::new(Vec3::new(x, -height / 2.0, z))
                    .with_normal(Vec3::new(x, 0.0, z).normalize()),
            );
        }

        // Top cap indices
        for i in 0..segments {
            indices.push(0);
            indices.push((2 + i * 2 + 1) as u32);
            indices.push((2 + ((i + 1) % segments) * 2 + 1) as u32);
        }

        // Bottom cap indices
        for i in 0..segments {
            indices.push(1);
            indices.push((2 + ((i + 1) % segments) * 2) as u32);
            indices.push((2 + i * 2) as u32);
        }

        // Side indices
        for i in 0..segments {
            let next = (i + 1) % segments;

            indices.push((2 + i * 2) as u32);
            indices.push((2 + next * 2) as u32);
            indices.push((2 + i * 2 + 1) as u32);

            indices.push((2 + i * 2 + 1) as u32);
            indices.push((2 + next * 2) as u32);
            indices.push((2 + next * 2 + 1) as u32);
        }

        MeshBuilder::new(name)
            .with_vertices(vertices)
            .with_indices(indices)
            .build()
    }

    /// Create a full-screen quad mesh
    pub fn fullscreen_quad(name: &str) -> Self {
        let vertices = vec![
            Vertex::new(Vec3::new(-1.0, -1.0, 0.0)).with_tex_coord(Vec2::new(0.0, 0.0)),
            Vertex::new(Vec3::new(1.0, -1.0, 0.0)).with_tex_coord(Vec2::new(1.0, 0.0)),
            Vertex::new(Vec3::new(1.0, 1.0, 0.0)).with_tex_coord(Vec2::new(1.0, 1.0)),
            Vertex::new(Vec3::new(-1.0, 1.0, 0.0)).with_tex_coord(Vec2::new(0.0, 1.0)),
        ];

        let indices = vec![0, 1, 2, 0, 2, 3];

        MeshBuilder::new(name)
            .with_vertices(vertices)
            .with_indices(indices)
            .build()
    }
}
