//! Mesh Loader
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Loads meshes from various file formats.

use super::mesh::Mesh;
use std::path::Path;
use std::sync::Arc;

/// Mesh loader
pub struct MeshLoader {
    /// Device reference
    device: Option<Arc<crate::rhi::Device>>,
}

impl MeshLoader {
    /// Create a new mesh loader
    pub fn new() -> Self {
        Self { device: None }
    }

    /// Set the device
    pub fn set_device(&mut self, device: Arc<crate::rhi::Device>) {
        self.device = Some(device);
    }

    /// Load a mesh from a file
    pub fn load(&self, path: &Path) -> Option<Mesh> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "obj" => self.load_obj(path),
            "gltf" | "glb" => self.load_gltf(path),
            "fbx" => self.load_fbx(path),
            _ => None,
        }
    }

    /// Load a mesh from OBJ file
    fn load_obj(&self, path: &Path) -> Option<Mesh> {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);

        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut tex_coords: Vec<[f32; 2]> = Vec::new();
        let mut vertices: Vec<super::mesh::Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for line in reader.lines() {
            let line = line.ok()?;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();
            let keyword = parts.next()?;

            match keyword {
                "v" => {
                    let x: f32 = parts.next()?.parse().ok()?;
                    let y: f32 = parts.next()?.parse().ok()?;
                    let z: f32 = parts.next()?.parse().ok()?;
                    positions.push([x, y, z]);
                }
                "vn" => {
                    let x: f32 = parts.next()?.parse().ok()?;
                    let y: f32 = parts.next()?.parse().ok()?;
                    let z: f32 = parts.next()?.parse().ok()?;
                    normals.push([x, y, z]);
                }
                "vt" => {
                    let u: f32 = parts.next()?.parse().ok()?;
                    let v: f32 = parts.next()?.parse().ok()?;
                    tex_coords.push([u, v]);
                }
                "f" => {
                    let face_vertices: Vec<&str> = parts.collect();
                    let mut face_indices: Vec<u32> = Vec::new();

                    for fv in &face_vertices {
                        let mut components = fv.split('/');
                        let pos_idx: i32 = components.next()?.parse().ok()?;
                        let tex_idx = components.next().and_then(|s| if s.is_empty() { None } else { s.parse::<i32>().ok() });
                        let norm_idx = components.next().and_then(|s| if s.is_empty() { None } else { s.parse::<i32>().ok() });

                        let pos_idx = if pos_idx > 0 {
                            (pos_idx - 1) as usize
                        } else {
                            (positions.len() as i32 + pos_idx) as usize
                        };

                        let position = positions.get(pos_idx)?;
                        let normal = norm_idx.and_then(|i| {
                            let idx = if i > 0 {
                                (i - 1) as usize
                            } else {
                                (normals.len() as i32 + i) as usize
                            };
                            normals.get(idx).copied()
                        }).unwrap_or([0.0, 0.0, 1.0]);
                        let tex_coord = tex_idx.and_then(|i| {
                            let idx = if i > 0 {
                                (i - 1) as usize
                            } else {
                                (tex_coords.len() as i32 + i) as usize
                            };
                            tex_coords.get(idx).copied()
                        }).unwrap_or([0.0, 0.0]);

                        vertices.push(super::mesh::Vertex::new(
                            glam::Vec3::new(position[0], position[1], position[2]),
                        )
                        .with_normal(glam::Vec3::new(normal[0], normal[1], normal[2]))
                        .with_tex_coord(glam::Vec2::new(tex_coord[0], tex_coord[1])));

                        face_indices.push((vertices.len() - 1) as u32);
                    }

                    for i in 1..face_indices.len() - 1 {
                        indices.push(face_indices[0]);
                        indices.push(face_indices[i]);
                        indices.push(face_indices[i + 1]);
                    }
                }
                _ => {}
            }
        }

        if vertices.is_empty() {
            return None;
        }

        Some(
            super::mesh::MeshBuilder::new("obj_mesh")
                .with_vertices(vertices)
                .with_indices(indices)
                .build(),
        )
    }

    /// Load a mesh from glTF file
    ///
    /// Returns `None` when the asset cannot be read. The reason is available
    /// from [`Self::load_gltf_reporting`]; this wrapper keeps the old
    /// `Option` signature for callers that do not care.
    fn load_gltf(&self, path: &Path) -> Option<Mesh> {
        self.load_gltf_reporting(path).ok()
    }

    /// Load a mesh from glTF file, reporting why it failed.
    ///
    /// This used to answer every request with `Mesh::cube`, which made a broken
    /// or unsupported asset look exactly like a successful load.
    pub fn load_gltf_reporting(&self, path: &Path) -> Result<Mesh, super::gltf::GltfError> {
        super::gltf::load(path)
    }

    /// Load a mesh from FBX file
    fn load_fbx(&self, path: &Path) -> Option<Mesh> {
        self.load_fbx_reporting(path).ok()
    }

    /// Load a mesh from FBX file, reporting why it failed.
    ///
    /// ASCII FBX is parsed for real. Binary FBX is detected and refused with
    /// [`super::fbx::FbxError::BinaryUnsupported`] rather than being answered
    /// with a cube.
    pub fn load_fbx_reporting(&self, path: &Path) -> Result<Mesh, super::fbx::FbxError> {
        super::fbx::load(path)
    }

    /// Load a mesh with buffers
    ///
    /// A mesh whose buffers could not be uploaded is not returned. Returning it
    /// would hand back something that looks loaded and draws nothing, which is
    /// the failure this loader spent its whole life producing in another form.
    pub fn load_with_buffers(&self, path: &Path) -> Option<Mesh> {
        let mut mesh = self.load(path)?;

        if let Some(device) = &self.device {
            mesh.create_buffers(device).ok()?;
        }

        Some(mesh)
    }
}

impl Default for MeshLoader {
    fn default() -> Self {
        Self::new()
    }
}
