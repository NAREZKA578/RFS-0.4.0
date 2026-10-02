//! Regression tests for the mesh loaders.
//!
//! These cover the defect that `gltf` and `fbx` answered every request with a
//! cube: a caller could not tell a successful load from an unsupported file.

use rfs_client::render::meshes::fbx::{self, FbxError};
use rfs_client::render::meshes::gltf::{self, GltfError};
use rfs_client::render::meshes::loader::MeshLoader;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- helpers

/// Write `bytes` to a uniquely named temporary file and hand back its path.
fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("rfs_loader_test_{}_{}", std::process::id(), name));
    std::fs::write(&path, bytes).expect("write temp fixture");
    path
}

/// A single triangle: three positions, three normals, three uvs, three indices.
fn triangle_buffer() -> Vec<u8> {
    let mut buf = Vec::new();
    let mut push = |values: &[f32]| {
        for v in values {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    };
    push(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]); // positions
    push(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]); // normals
    push(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]); // uvs
    for i in 0..3u16 {
        buf.extend_from_slice(&i.to_le_bytes());
    }
    buf
}

/// A glTF document whose single buffer is the given bytes, inlined as base64.
fn gltf_with_data_uri(buffer: &[u8], file_name: &str) -> String {
    let encoded = base64_encode(buffer);
    format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "meshes": [{{
    "name": "probe",
    "primitives": [{{
      "attributes": {{
        "POSITION": 0,
        "NORMAL": 1,
        "TEXCOORD_0": 2
      }},
      "indices": 3
    }}]
  }}],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2" }},
    {{ "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0,  "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 36, "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 72, "byteLength": 24 }},
    {{ "buffer": 0, "byteOffset": 96, "byteLength": 6 }}
  ],
  "buffers": [{{
    "byteLength": {len},
    "uri": "data:application/octet-stream;base64,{encoded}"
  }}]
}}"#,
        len = buffer.len(),
        encoded = encoded,
    )
    .replace("probe", file_name)
}

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Wrap a JSON document and a binary chunk into a GLB container.
fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json_chunk = json.as_bytes().to_vec();
    while json_chunk.len() % 4 != 0 {
        json_chunk.push(b' ');
    }
    let mut bin_chunk = bin.to_vec();
    while bin_chunk.len() % 4 != 0 {
        bin_chunk.push(0);
    }

    let total = 12 + 8 + json_chunk.len() + 8 + bin_chunk.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());

    out.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_chunk);

    out.extend_from_slice(&(bin_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin_chunk);
    out
}

// ------------------------------------------------------------------ glTF

#[test]
fn gltf_with_inline_data_uri_yields_the_real_triangle() {
    let buffer = triangle_buffer();
    let path = temp_file(
        "inline.gltf",
        gltf_with_data_uri(&buffer, "inline").as_bytes(),
    );
    let mesh = gltf::load(&path).expect("gltf should load");

    // The point of the test: this is the file's triangle, not a cube.
    assert_eq!(mesh.vertex_count, 3, "expected 3 vertices from the file");
    assert_eq!(mesh.index_count, 3, "expected 3 indices from the file");
    assert_eq!(mesh.name, "inline");
    assert_ne!(mesh.vertex_count, 24, "must not be the 24-vertex cube");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn gltf_positions_survive_the_round_trip() {
    let buffer = triangle_buffer();
    let path = temp_file(
        "positions.gltf",
        gltf_with_data_uri(&buffer, "positions").as_bytes(),
    );
    let mesh = gltf::load(&path).expect("gltf should load");
    let positions: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position.to_array()).collect();

    assert_eq!(positions[0], [0.0, 0.0, 0.0]);
    assert_eq!(positions[1], [1.0, 0.0, 0.0]);
    assert_eq!(positions[2], [0.0, 1.0, 0.0]);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn glb_container_is_parsed_like_the_json_form() {
    let buffer = triangle_buffer();
    let json = gltf_with_data_uri(&buffer, "container");
    // GLB buffers are the binary chunk, so the document must not carry a URI.
    let json = json.replace(
        &format!("\"uri\": \"data:application/octet-stream;base64,{}\"", base64_encode(&buffer)),
        &format!("\"byteLength\": {}", buffer.len()),
    );
    let path = temp_file("container.glb", &glb(&json, &buffer));

    let mesh = gltf::load(&path).expect("glb should load");
    assert_eq!(mesh.vertex_count, 3);
    assert_ne!(mesh.vertex_count, 24, "must not be the cube");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_interleaved_buffer_is_read_with_its_stride() {
    // One view holding position, normal and texcoord per vertex: 32 bytes per
    // vertex. A reader that steps by 12 instead of by the declared stride gets
    // the normal's bytes as a position, which is wrong geometry rather than an
    // error — the failure this test exists to prevent.
    let mut buffer = Vec::new();
    for (p, n, uv) in [
        ([0.0f32, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0f32, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0]),
    ] {
        for v in [p[0], p[1], p[2], n[0], n[1], n[2], uv[0], uv[1]] {
            buffer.extend_from_slice(&v.to_le_bytes());
        }
    }
    for i in 0..3u16 {
        buffer.extend_from_slice(&i.to_le_bytes());
    }

    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "meshes": [{{
    "name": "interleaved",
    "primitives": [{{
      "attributes": {{ "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }},
      "indices": 3
    }}]
  }}],
  "accessors": [
    {{ "bufferView": 0, "byteOffset": 0,  "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 0, "byteOffset": 24, "componentType": 5126, "count": 3, "type": "VEC2" }},
    {{ "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": {len}, "byteStride": 32 }},
    {{ "buffer": 0, "byteOffset": {len}, "byteLength": 6 }}
  ],
  "buffers": [{{
    "byteLength": {total},
    "uri": "data:application/octet-stream;base64,{encoded}"
  }}]
}}"#,
        len = buffer.len() - 6,
        total = buffer.len(),
        encoded = base64_encode(&buffer),
    );

    let path = temp_file("interleaved.gltf", json.as_bytes());
    let mesh = gltf::load(&path).expect("interleaved gltf should load");

    assert_eq!(mesh.vertex_count, 3);
    let first = mesh.vertices[0];
    assert_eq!(
        first.position.to_array(),
        [0.0, 0.0, 0.0],
        "position must come from the first 12 bytes of each 32-byte stride"
    );
    assert_eq!(first.normal.to_array(), [0.0, 0.0, 1.0]);
    assert_eq!(first.tex_coord.to_array(), [0.0, 0.0]);
    assert_eq!(
        mesh.vertices[2].position.to_array(),
        [0.0, 1.0, 0.0],
        "the third vertex must not be read from the wrong stride"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_gltf_without_buffers_is_an_error_not_a_cube() {
    let path = temp_file(
        "empty.gltf",
        br#"{ "asset": { "version": "2.0" }, "meshes": [] }"#,
    );
    assert_eq!(gltf::load(&path).err(), Some(GltfError::NoBuffers));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_gltf_without_meshes_is_an_error() {
    let path = temp_file(
        "nomesh.gltf",
        br#"{ "asset": { "version": "2.0" }, "buffers": [] }"#,
    );
    assert_eq!(gltf::load(&path).err(), Some(GltfError::NoBuffers));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_missing_gltf_file_is_an_io_error() {
    assert_eq!(
        gltf::load(Path::new("definitely-not-here-12345.gltf")).err(),
        Some(GltfError::Io)
    );
}

// ------------------------------------------------------------------- FBX

/// A single quad as four vertices, triangulated into two triangles.
///
/// FBX polygon indices are 1-based and the last corner of a polygon is
/// negated, so a quad is `1,2,3,-4`.
fn ascii_fbx() -> String {
    "FBXHeaderExtension:  {\n\
     \tFBXVersion: 7400\n\
     }\n\
     Objects:  {\n\
     \tGeometry: 1000, \"Geometry::\", \"Mesh\" {\n\
     \t\tVertices: *12 { a: 0.0,0.0,0.0, 1.0,0.0,0.0, 1.0,1.0,0.0, 0.0,1.0,0.0 }\n\
     \t\tPolygonVertexIndex: *4 { a: 1,2,3,-4 }\n\
     \t}\n\
     }\n"
        .to_string()
}

#[test]
fn ascii_fbx_yields_the_real_quad() {
    let path = temp_file("quad.fbx", ascii_fbx().as_bytes());
    let mesh = fbx::load(&path).expect("ascii fbx should load");

    assert_eq!(mesh.vertex_count, 4, "expected the file's 4 vertices");
    assert_eq!(mesh.index_count, 6, "a quad triangulates to 2 triangles");
    assert_ne!(mesh.vertex_count, 24, "must not be the cube");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn ascii_fbx_face_normals_point_out_of_the_winding() {
    let path = temp_file("normals.fbx", ascii_fbx().as_bytes());
    let mesh = fbx::load(&path).expect("ascii fbx should load");

    // The quad is wound counter-clockwise in the XY plane, so +Z is outward.
    for v in &mesh.vertices {
        assert!(
            v.normal.z > 0.9,
            "expected +Z normals, got {:?}",
            v.normal.to_array()
        );
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_degenerate_polygon_is_rejected() {
    let text = "Objects:  {\n\
     \tGeometry: 1000, \"Geometry::\", \"Mesh\" {\n\
     \t\tVertices: *9 { a: 0.0,0.0,0.0, 1.0,0.0,0.0, 0.0,1.0,0.0 }\n\
     \t\tPolygonVertexIndex: *2 { a: 1,-2 }\n\
     \t}\n\
     }\n";
    let path = temp_file("degenerate.fbx", text.as_bytes());
    assert_eq!(fbx::load(&path).err(), Some(FbxError::DegeneratePolygon));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fbx_without_vertices_is_an_error() {
    let path = temp_file("noverts.fbx", b"FBXHeaderExtension:  { FBXVersion: 7400 }\n");
    assert_eq!(fbx::load(&path).err(), Some(FbxError::NoVertices));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_index_past_the_vertex_buffer_is_refused() {
    // Four corners claimed, three defined. Dropping the fourth would yield a
    // triangle built from the wrong vertices — wrong geometry, no error.
    let text = "Objects:  {\n\
     \tGeometry: 1000, \"Geometry::\", \"Mesh\" {\n\
     \t\tVertices: *9 { a: 0.0,0.0,0.0, 1.0,0.0,0.0, 0.0,1.0,0.0 }\n\
     \t\tPolygonVertexIndex: *4 { a: 1,2,3,-4 }\n\
     \t}\n\
     }\n";
    let path = temp_file("outofrange.fbx", text.as_bytes());
    assert_eq!(fbx::load(&path).err(), Some(FbxError::IndexOutOfRange));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn binary_fbx_is_refused_rather_than_answered_with_a_cube() {
    let mut bytes = b"Kaydara FBX Binary  ".to_vec();
    bytes.extend_from_slice(&[0u8; 32]);
    let path = temp_file("binary.fbx", &bytes);

    // This is the behaviour change that matters: an unsupported container is now
    // visible, where before it silently produced a cube.
    assert_eq!(fbx::load(&path).err(), Some(FbxError::BinaryUnsupported));

    // And through the public loader, which keeps its Option signature.
    let loader = MeshLoader::new();
    assert!(loader.load(&path).is_none());

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------- loader

#[test]
fn the_loader_no_longer_substitutes_a_cube_for_broken_assets() {
    let path = temp_file("broken.gltf", b"{ not json at all");

    let loader = MeshLoader::new();
    assert!(loader.load(&path).is_none(), "broken gltf must not load");
    assert_eq!(
        loader.load_gltf_reporting(&path).err(),
        Some(GltfError::NotGltf),
        "and the reason must be reported"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_loader_still_reads_obj() {
    let obj = "v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 0.0 1.0 0.0\nf 1 2 3\n";
    let path = temp_file("tri.obj", obj.as_bytes());

    let loader = MeshLoader::new();
    let mesh = loader.load(&path).expect("obj should still load");
    assert_eq!(mesh.index_count, 3);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_unknown_extension_loads_nothing() {
    let path = temp_file("thing.xyz", b"whatever");
    let loader = MeshLoader::new();
    assert!(loader.load(&path).is_none());
    let _ = std::fs::remove_file(&path);
}