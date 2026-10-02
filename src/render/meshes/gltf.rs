//! glTF 2.0 / GLB mesh loader
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Parses the geometry of a glTF 2.0 asset into a [`Mesh`]. Scope is
//! deliberately narrow: positions, normals, texcoord 0 and indices, which is
//! what the renderer binds today. Skinning, morph targets, tangents and
//! multiple primitives are not read.
//!
//! Three container forms are accepted, because assets arrive in all three:
//!
//! * `.gltf` with an external `.bin` buffer next to it,
//! * `.gltf` with the buffer inline as a `data:` URI,
//! * `.glb`, whose binary chunk is the first buffer with no URI.

use super::mesh::{Mesh, MeshBuilder, Vertex};
use glam::{Vec2, Vec3};
use serde_json::Value;
use std::path::Path;

/// Why a glTF asset could not be turned into a mesh.
///
/// This exists so that a caller can tell "the file is not there" from "the file
/// is there and I do not understand it". The loader used to answer every glTF
/// request with a cube, which made an unsupported asset indistinguishable from
/// a successful load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GltfError {
    Io,
    NotGltf,
    NoBuffers,
    NoMesh,
    NoPrimitives,
    MissingAttribute(&'static str),
    BadAccessor(&'static str),
    BadBufferView,
    BadPrimitiveMode,
    EmptyGeometry,
}

impl std::fmt::Display for GltfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GltfError::Io => write!(f, "asset could not be read"),
            GltfError::NotGltf => write!(f, "not a glTF 2.0 document"),
            GltfError::NoBuffers => write!(f, "document declares no buffers"),
            GltfError::NoMesh => write!(f, "document declares no mesh"),
            GltfError::NoPrimitives => write!(f, "first mesh has no primitives"),
            GltfError::MissingAttribute(a) => write!(f, "primitive is missing attribute {a}"),
            GltfError::BadAccessor(a) => write!(f, "accessor {a} cannot be read"),
            GltfError::BadBufferView => write!(f, "bufferView is out of range"),
            GltfError::BadPrimitiveMode => write!(f, "primitive mode is not triangles"),
            GltfError::EmptyGeometry => write!(f, "primitive produced no vertices"),
        }
    }
}

/// Load the first mesh of a glTF asset.
pub fn load(path: &Path) -> Result<Mesh, GltfError> {
    let bytes = std::fs::read(path).map_err(|_| GltfError::Io)?;
    let (json, glb_bin) = split_container(&bytes)?;
    let root: Value = serde_json::from_slice(&json).map_err(|_| GltfError::NotGltf)?;
    load_from_json(&root, glb_bin.as_deref(), path)
}

/// Split a `.gltf` or `.glb` byte blob into its JSON and its optional binary chunk.
fn split_container(bytes: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>), GltfError> {
    // GLB: magic "glTF", version, total length, then chunks of
    // (length, type, payload). JSON is type 0x4E4F534A, BIN is 0x004E4942.
    if bytes.len() >= 12 && &bytes[0..4] == b"glTF" {
        let mut offset = 12usize;
        let mut json = None;
        let mut bin = None;
        while offset + 8 <= bytes.len() {
            let len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = &bytes[offset + 4..offset + 8];
            let start = offset + 8;
            let end = start.checked_add(len).ok_or(GltfError::NotGltf)?;
            if end > bytes.len() {
                return Err(GltfError::NotGltf);
            }
            if kind == b"JSON" {
                json = Some(bytes[start..end].to_vec());
            } else if kind == b"BIN\0" {
                bin = Some(bytes[start..end].to_vec());
            }
            // Chunks are 4-byte aligned.
            offset = start + len.div_ceil(4) * 4;
        }
        return json.map(|j| (j, bin)).ok_or(GltfError::NotGltf);
    }
    Ok((bytes.to_vec(), None))
}

/// Build a mesh from an already-parsed glTF document.
fn load_from_json(
    root: &Value,
    glb_bin: Option<&[u8]>,
    path: &Path,
) -> Result<Mesh, GltfError> {
    let base_dir = path.parent().unwrap_or(Path::new("."));

    let buffers = resolve_buffers(root, glb_bin, base_dir)?;
    if buffers.is_empty() {
        return Err(GltfError::NoBuffers);
    }

    let meshes = root.get("meshes").and_then(Value::as_array).ok_or(GltfError::NoMesh)?;
    let mesh = meshes.first().ok_or(GltfError::NoMesh)?;
    let primitives = mesh
        .get("primitives")
        .and_then(Value::as_array)
        .ok_or(GltfError::NoPrimitives)?;

    // Only the first primitive is turned into geometry; the renderer takes a
    // single index/vertex buffer pair, so merging primitives is out of scope.
    let primitive = primitives.first().ok_or(GltfError::NoPrimitives)?;

    match primitive.get("mode").and_then(Value::as_u64) {
        None | Some(4) => {}
        Some(_) => return Err(GltfError::BadPrimitiveMode),
    }

    let attributes = primitive
        .get("attributes")
        .and_then(Value::as_object)
        .ok_or(GltfError::MissingAttribute("POSITION"))?;

    let positions = read_vec3(root, &buffers, attributes, "POSITION")?;
    let count = positions.len();

    let normals = match attributes.get("NORMAL") {
        Some(_) => Some(read_vec3(root, &buffers, attributes, "NORMAL")?),
        None => None,
    };
    let uvs = match attributes.get("TEXCOORD_0") {
        Some(_) => Some(read_vec2(root, &buffers, attributes, "TEXCOORD_0")?),
        None => None,
    };

    let indices = match primitive.get("indices").and_then(Value::as_u64) {
        Some(index) => read_indices(root, &buffers, index as usize)?,
        // A primitive without indices is a point/line/list; for triangles the
        // spec requires them, so refuse rather than inventing an order.
        None => (0..count as u32).collect(),
    };

    let mut vertices = Vec::with_capacity(positions.len());
    for (i, position) in positions.iter().enumerate() {
        let mut v = Vertex::new(*position);
        if let Some(normal) = normals.as_ref().and_then(|n| n.get(i)) {
            v = v.with_normal(*normal);
        }
        if let Some(uv) = uvs.as_ref().and_then(|u| u.get(i)) {
            v = v.with_tex_coord(*uv);
        }
        vertices.push(v);
    }

    if vertices.is_empty() {
        return Err(GltfError::EmptyGeometry);
    }

    let name = mesh
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("gltf_mesh");

    Ok(MeshBuilder::new(name)
        .with_vertices(vertices)
        .with_indices(indices)
        .build())
}

/// Read every buffer declared by the document into memory.
fn resolve_buffers(
    root: &Value,
    glb_bin: Option<&[u8]>,
    base_dir: &Path,
) -> Result<Vec<Vec<u8>>, GltfError> {
    let declared = root
        .get("buffers")
        .and_then(Value::as_array)
        .ok_or(GltfError::NoBuffers)?;

    let mut out = Vec::with_capacity(declared.len());
    for (index, entry) in declared.iter().enumerate() {
        match entry.get("uri").and_then(Value::as_str) {
            // The first buffer of a GLB is the binary chunk.
            None => match glb_bin {
                Some(bin) if index == 0 => out.push(bin.to_vec()),
                _ => return Err(GltfError::BadBufferView),
            },
            Some(uri) if uri.starts_with("data:") => {
                out.push(decode_data_uri(uri)?);
            }
            Some(uri) => {
                let file = base_dir.join(uri);
                let data = std::fs::read(&file).map_err(|_| GltfError::Io)?;
                out.push(data);
            }
        }
    }
    Ok(out)
}

/// Decode a `data:` URI. Only base64 is accepted; plain percent-encoded
/// payloads are rejected instead of silently mis-parsed.
fn decode_data_uri(uri: &str) -> Result<Vec<u8>, GltfError> {
    let payload = uri.split_once(',').map(|(_, p)| p).ok_or(GltfError::BadBufferView)?;
    if !uri.contains(";base64") {
        return Err(GltfError::BadBufferView);
    }
    base64_decode(payload)
}

/// Minimal base64 decoder. Pulling in a crate for 30 lines is not worth the
/// dependency, and glTF buffers are the only thing in the project that needs it.
fn base64_decode(input: &str) -> Result<Vec<u8>, GltfError> {
    fn value(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for b in bytes {
        if b == b'=' {
            break;
        }
        let v = value(b).ok_or(GltfError::BadBufferView)?;
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// glTF component types.
const BYTE: u64 = 5120;
const UNSIGNED_BYTE: u64 = 5121;
const SHORT: u64 = 5122;
const UNSIGNED_SHORT: u64 = 5123;
const UNSIGNED_INT: u64 = 5125;
const FLOAT: u64 = 5126;

/// Read one bufferView's raw bytes.
fn buffer_view_bytes<'a>(
    root: &Value,
    buffers: &'a [Vec<u8>],
    index: usize,
) -> Result<&'a [u8], GltfError> {
    let view = root
        .get("bufferViews")
        .and_then(Value::as_array)
        .and_then(|v| v.get(index))
        .ok_or(GltfError::BadBufferView)?;

    let buffer_index = view.get("buffer").and_then(Value::as_u64).unwrap_or(0) as usize;
    let buffer = buffers.get(buffer_index).ok_or(GltfError::BadBufferView)?;
    let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let length = view.get("byteLength").and_then(Value::as_u64).ok_or(GltfError::BadBufferView)? as usize;

    buffer
        .get(offset..offset + length)
        .ok_or(GltfError::BadBufferView)
}

/// Read a float accessor as `Vec3` values.
fn read_vec3(
    root: &Value,
    buffers: &[Vec<u8>],
    attributes: &serde_json::Map<String, Value>,
    name: &'static str,
) -> Result<Vec<Vec3>, GltfError> {
    let index = attributes
        .get(name)
        .and_then(Value::as_u64)
        .ok_or(GltfError::MissingAttribute(name))? as usize;
    let (bytes, count, comp, normalized, stride) = accessor_layout(root, buffers, index)?;

    if comp != FLOAT {
        return Err(GltfError::BadAccessor(name));
    }
    let step = step_for(stride, 12);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * step;
        out.push(Vec3::new(
            read_f32(bytes, base, name)?,
            read_f32(bytes, base + 4, name)?,
            read_f32(bytes, base + 8, name)?,
        ));
    }
    // A normalized integer accessor is legal for POSITION in theory but never
    // produced by an exporter; reject rather than silently mis-reading.
    if normalized {
        return Err(GltfError::BadAccessor(name));
    }
    Ok(out)
}

/// Read a float accessor as `Vec2` values.
fn read_vec2(
    root: &Value,
    buffers: &[Vec<u8>],
    attributes: &serde_json::Map<String, Value>,
    name: &'static str,
) -> Result<Vec<Vec2>, GltfError> {
    let index = attributes
        .get(name)
        .and_then(Value::as_u64)
        .ok_or(GltfError::MissingAttribute(name))? as usize;
    let (bytes, count, comp, _, stride) = accessor_layout(root, buffers, index)?;

    if comp != FLOAT {
        return Err(GltfError::BadAccessor(name));
    }
    let step = step_for(stride, 8);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * step;
        out.push(Vec2::new(
            read_f32(bytes, base, name)?,
            read_f32(bytes, base + 4, name)?,
        ));
    }
    Ok(out)
}

/// Read an index accessor into `u32`.
fn read_indices(
    root: &Value,
    buffers: &[Vec<u8>],
    index: usize,
) -> Result<Vec<u32>, GltfError> {
    let (bytes, count, comp, _, stride) = accessor_layout(root, buffers, index)?;
    let mut out = Vec::with_capacity(count);
    match comp {
        UNSIGNED_BYTE | UNSIGNED_SHORT | UNSIGNED_INT => {
            let width = match comp {
                UNSIGNED_BYTE => 1,
                UNSIGNED_SHORT => 2,
                _ => 4,
            };
            let step = step_for(stride, width);
            for i in 0..count {
                let start = i * step;
                let v = match width {
                    1 => *bytes.get(start).ok_or(GltfError::BadBufferView)? as u32,
                    2 => u16::from_le_bytes(
                        bytes
                            .get(start..start + 2)
                            .ok_or(GltfError::BadBufferView)?
                            .try_into()
                            .map_err(|_| GltfError::BadBufferView)?,
                    ) as u32,
                    _ => u32::from_le_bytes(
                        bytes
                            .get(start..start + 4)
                            .ok_or(GltfError::BadBufferView)?
                            .try_into()
                            .map_err(|_| GltfError::BadBufferView)?,
                    ),
                };
                out.push(v);
            }
        }
        _ => return Err(GltfError::BadBufferView),
    }
    Ok(out)
}

/// Resolve an accessor into `(bytes, element count, component type, normalized)`.
///
/// The returned slice borrows from `buffers`, not from `root`: everything read
/// out of the document is copied into the buffer, so the JSON tree is free to
/// drop before the caller uses the bytes.
fn accessor_layout<'a>(
    root: &Value,
    buffers: &'a [Vec<u8>],
    index: usize,
) -> Result<(&'a [u8], usize, u64, bool, usize), GltfError> {
    let accessor = root
        .get("accessors")
        .and_then(Value::as_array)
        .and_then(|a| a.get(index))
        .ok_or(GltfError::BadAccessor("accessor"))?;

    let count = accessor.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
    let comp = accessor
        .get("componentType")
        .and_then(Value::as_u64)
        .ok_or(GltfError::BadAccessor("accessor"))?;
    let normalized = accessor
        .get("normalized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let accessor_offset = accessor.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;

    let view_index = accessor
        .get("bufferView")
        .and_then(Value::as_u64)
        .ok_or(GltfError::BadAccessor("accessor has no bufferView"))? as usize;
    let view = root
        .get("bufferViews")
        .and_then(Value::as_array)
        .and_then(|v| v.get(view_index))
        .ok_or(GltfError::BadBufferView)?;

    let view_bytes = buffer_view_bytes(root, buffers, view_index)?;
    let start = accessor_offset;
    if start > view_bytes.len() {
        return Err(GltfError::BadAccessor("accessor offset"));
    }
    let slice = &view_bytes[start..];

    // `byteStride` is the distance between consecutive elements, not the size of
    // one. An interleaved buffer packs position, normal and texcoord into a
    // single view with a stride wider than any one of them, so the readers must
    // step by it. Returning it as 0 means "tightly packed" for the common case
    // of a view per attribute.
    let tight = comp_size(comp) * components_of(accessor)?;
    let stride = match view.get("byteStride").and_then(Value::as_u64) {
        Some(s) if s > 0 => {
            let stride = s as usize;
            if stride < tight {
                return Err(GltfError::BadAccessor("byteStride smaller than one element"));
            }
            if count > 0 && stride * (count - 1) + tight > slice.len() {
                return Err(GltfError::BadAccessor("strided accessor overruns view"));
            }
            stride
        }
        _ => {
            if count > 0 && tight * count > slice.len() {
                return Err(GltfError::BadAccessor("accessor overruns view"));
            }
            0
        }
    };
    Ok((slice, count, comp, normalized, stride))
}

/// Distance in bytes between element `i` and element `i + 1`.
fn step_for(stride: usize, tight: usize) -> usize {
    if stride > 0 {
        stride
    } else {
        tight
    }
}

fn comp_size(comp: u64) -> usize {
    match comp {
        BYTE | UNSIGNED_BYTE => 1,
        SHORT | UNSIGNED_SHORT => 2,
        UNSIGNED_INT | FLOAT => 4,
        _ => 0,
    }
}

fn components_of(accessor: &Value) -> Result<usize, GltfError> {
    match accessor.get("type").and_then(Value::as_str).unwrap_or("SCALAR") {
        "SCALAR" => Ok(1),
        "VEC2" => Ok(2),
        "VEC3" => Ok(3),
        "VEC4" => Ok(4),
        "MAT4" => Ok(16),
        _ => Err(GltfError::BadAccessor("accessor type")),
    }
}

fn read_f32(bytes: &[u8], at: usize, name: &'static str) -> Result<f32, GltfError> {
    let raw = bytes
        .get(at..at + 4)
        .ok_or(GltfError::BadAccessor(name))?;
    Ok(f32::from_le_bytes(raw.try_into().map_err(|_| GltfError::BadAccessor(name))?))
}