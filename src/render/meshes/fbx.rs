//! FBX mesh loader
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Parses geometry out of an ASCII-encoded FBX file. The ASCII form is a
//! nested node tree with `a:` property lists, which is enough to recover a
//! position buffer and a triangle index list.
//!
//! Binary FBX (`Kaydara FBX Binary`) is detected and rejected with
//! [`FbxError::BinaryUnsupported`]. That is the point of the error type: the
//! loader used to answer every FBX request with a cube, so an unsupported file
//! was indistinguishable from a successful load.

use super::mesh::{Mesh, MeshBuilder, Vertex};
use glam::Vec3;
use std::path::Path;

/// Why an FBX asset could not be turned into a mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FbxError {
    Io,
    BinaryUnsupported,
    NoVertices,
    NoIndices,
    IndexOutOfRange,
    DegeneratePolygon,
    EmptyGeometry,
}

impl std::fmt::Display for FbxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FbxError::Io => write!(f, "asset could not be read"),
            FbxError::BinaryUnsupported => {
                write!(f, "binary FBX is not supported; export as ASCII FBX")
            }
            FbxError::NoVertices => write!(f, "no Geometry/Vertices block"),
            FbxError::NoIndices => write!(f, "no Geometry/PolygonVertexIndex block"),
            FbxError::IndexOutOfRange => {
                write!(f, "a polygon references a vertex that the file does not define")
            }
            FbxError::DegeneratePolygon => write!(f, "a polygon has fewer than three corners"),
            FbxError::EmptyGeometry => write!(f, "no triangles were produced"),
        }
    }
}

const BINARY_MAGIC: &[u8] = b"Kaydara FBX Binary  ";

/// Load the first `Geometry` node of an ASCII FBX file.
pub fn load(path: &Path) -> Result<Mesh, FbxError> {
    let bytes = std::fs::read(path).map_err(|_| FbxError::Io)?;
    load_from_bytes(&bytes)
}

/// Load from an in-memory FBX document.
pub fn load_from_bytes(bytes: &[u8]) -> Result<Mesh, FbxError> {
    if bytes.starts_with(BINARY_MAGIC) {
        return Err(FbxError::BinaryUnsupported);
    }
    let text = String::from_utf8_lossy(bytes);

    let positions = parse_vertices(&text).ok_or(FbxError::NoVertices)?;
    let polygons = parse_polygon_indices(&text).ok_or(FbxError::NoIndices)?;

    // FBX marks the last corner of a polygon with a negated index (1-based).
    // Split on the sign flips, then fan-triangulate each polygon.
    let mut vertices = Vec::with_capacity(positions.len());
    for p in &positions {
        vertices.push(Vertex::new(Vec3::new(p[0], p[1], p[2])));
    }

    let mut indices: Vec<u32> = Vec::with_capacity(polygons.len() * 3);
    let mut current: Vec<u32> = Vec::new();
    for raw in polygons {
        if raw == 0 {
            continue;
        }
        let (value, terminates) = if raw < 0 {
            ((-raw) as u32 - 1, true)
        } else {
            ((raw as u32) - 1, false)
        };
        // An index past the end of the position buffer means the file
        // contradicts itself. Dropping it would quietly turn a quad into a
        // triangle made of the wrong three vertices, so refuse the file.
        if (value as usize) >= vertices.len() {
            return Err(FbxError::IndexOutOfRange);
        }
        current.push(value);
        if terminates {
            if current.len() < 3 {
                if !current.is_empty() {
                    return Err(FbxError::DegeneratePolygon);
                }
            } else {
                for i in 1..current.len() - 1 {
                    indices.push(current[0]);
                    indices.push(current[i]);
                    indices.push(current[i + 1]);
                }
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        return Err(FbxError::DegeneratePolygon);
    }

    if indices.is_empty() {
        return Err(FbxError::EmptyGeometry);
    }

    assign_face_normals(&mut vertices, &indices);

    Ok(MeshBuilder::new("fbx_mesh")
        .with_vertices(vertices)
        .with_indices(indices)
        .build())
}

/// Give every corner the normal of the face it belongs to.
///
/// The vertex layout here is not shared between faces, so averaging normals
/// across neighbours would mean welding identical positions first. Flat
/// normals are correct for a faceted mesh and never point the wrong way.
fn assign_face_normals(vertices: &mut [Vertex], indices: &[u32]) {
    for tri in indices.chunks(3) {
        let &[a, b, c] = tri else { break };
        let (Some(&va), Some(&vb), Some(&vc)) = (
            vertices.get(a as usize),
            vertices.get(b as usize),
            vertices.get(c as usize),
        ) else {
            continue;
        };
        let normal = (vb.position - va.position).cross(vc.position - va.position);
        let normal = if normal.length_squared() > f32::EPSILON {
            normal.normalize()
        } else {
            Vec3::Z
        };
        for corner in tri {
            if let Some(vertex) = vertices.get_mut(*corner as usize) {
                vertex.normal = normal;
            }
        }
    }
}

/// Pull `Vertices: *N { a: ... }` out of the document.
fn parse_vertices(text: &str) -> Option<Vec<[f32; 3]>> {
    let payload = property_array(text, "Vertices:")?;
    let floats = parse_f64_list(payload)?;
    let mut out = Vec::with_capacity(floats.len() / 3);
    for i in (0..floats.len().saturating_sub(2)).step_by(3) {
        out.push([floats[i] as f32, floats[i + 1] as f32, floats[i + 2] as f32]);
    }
    Some(out)
}

/// Pull `PolygonVertexIndex: *N { a: ... }` out of the document.
fn parse_polygon_indices(text: &str) -> Option<Vec<i64>> {
    let payload = property_array(text, "PolygonVertexIndex:")?;
    let floats = parse_f64_list(payload)?;
    Some(floats.iter().map(|v| *v as i64).collect())
}

/// Return the text between the `{` and `}` that follow `node:`.
fn property_array<'a>(text: &'a str, node: &str) -> Option<&'a str> {
    let start = text.find(node)? + node.len();
    let open = text[start..].find('{')? + start;
    let close = text[open..].find('}')? + open;
    Some(&text[open + 1..close])
}

/// Parse the comma-separated `a:` list FBX uses for flat arrays.
fn parse_f64_list(payload: &str) -> Option<Vec<f64>> {
    let body = payload.trim();
    let body = body.strip_prefix("a:").unwrap_or(body);
    let body = body.trim().trim_end_matches(',');
    if body.is_empty() {
        return Some(Vec::new());
    }
    body.split(',')
        .map(|part| part.trim().parse::<f64>().ok())
        .collect()
}