//! Integration tests for the render::meshes module.
//!
//! Covers: Vertex (and builder), Mesh, MeshBuilder, common primitives (cube,
//! plane, sphere, cylinder, fullscreen quad), IndexType, PrimitiveType,
//! MeshFlags and MeshLibrary.

use glam::{Vec2, Vec3, Vec4};
use rfs_client::render::meshes::mesh::MeshBuilder;
use rfs_client::render::meshes::{IndexType, Mesh, MeshFlags, MeshLibrary, PrimitiveType, Vertex};

/// Mesh buffers must be real GPU buffers with the geometry actually in them.
///
/// `create_vertex_buffer`/`create_index_buffer` used to take the device as
/// `_device`, ignore it, and build a CPU-only `Buffer` that reported the right
/// size while having no GPU backing. A draw would then have bound handle 0.
/// This drives the real Vulkan device and checks the backing, the size and the
/// bytes read back.
#[test]
fn mesh_buffers_reach_the_gpu_with_their_geometry() {
    use rhi::backend::create_backend;
    use rhi::config::RhiConfig;
    use rhi::core::device::DeviceDesc;
    use rhi::types::Features;

    let backend = create_backend(&RhiConfig::default()).expect("create backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    if devices.is_empty() {
        panic!("no Vulkan device available");
    }
    let device = backend
        .create_device(&devices[0], &DeviceDesc { features: Features::default(), ..Default::default() })
        .expect("create device");

    let mut mesh = Mesh::cube("cube", 1.0);
    let vertex_count = mesh.vertices.len();
    let index_count = mesh.indices.len();
    assert!(vertex_count > 0 && index_count > 0, "the primitive should have geometry");

    mesh.create_buffers(&device);

    let vertex_buffer = mesh.vertex_buffer().expect("vertex buffer");
    let index_buffer = mesh.index_buffer().expect("index buffer");
    assert!(vertex_buffer.has_gpu_backing(), "vertex buffer has no GPU backing");
    assert!(index_buffer.has_gpu_backing(), "index buffer has no GPU backing");

    // Sizes must match the data actually submitted, not a guess.
    assert_eq!(
        vertex_buffer.size() as usize,
        std::mem::size_of::<Vertex>() * vertex_count,
        "vertex buffer size does not match the geometry"
    );
    assert_eq!(
        index_buffer.size() as usize,
        4 * index_count,
        "index buffer size does not match the geometry"
    );

    // The bytes have to be there, not just a handle. A buffer that was created
    // but never uploaded reads back as zeros.
    let read_back = device
        .download_buffer(index_buffer, 0, index_buffer.size())
        .expect("read back the index buffer");
    assert_eq!(read_back.len(), 4 * index_count);
    assert!(
        read_back.iter().any(|b| *b != 0),
        "the index buffer is all zeros: the upload never happened"
    );
}

const EPSILON: f32 = 1e-4;
const SQRT_3: f32 = 1.73205;

#[test]
fn vertex_builder_and_defaults() {
    let vertex = Vertex::new(Vec3::new(1.0, 2.0, 3.0))
        .with_normal(Vec3::Y)
        .with_tangent(Vec4::new(1.0, 0.0, 0.0, 0.0))
        .with_tex_coord(Vec2::new(0.5, 0.25))
        .with_color(Vec4::new(0.1, 0.2, 0.3, 1.0));

    assert_eq!(vertex.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(vertex.normal, Vec3::Y);
    assert_eq!(vertex.tangent, Vec4::new(1.0, 0.0, 0.0, 0.0));
    assert_eq!(vertex.tex_coord, Vec2::new(0.5, 0.25));
    assert_eq!(vertex.color, Vec4::new(0.1, 0.2, 0.3, 1.0));

    let default = Vertex::default();
    assert_eq!(default.position, Vec3::ZERO);
    assert_eq!(default.normal, Vec3::Z);
    assert_eq!(default.tex_coord, Vec2::ZERO);
    assert_eq!(default.color, Vec4::ONE);
}

#[test]
fn index_type_sizes() {
    assert_eq!(IndexType::U8.size(), 1);
    assert_eq!(IndexType::U16.size(), 2);
    assert_eq!(IndexType::U32.size(), 4);
    assert_eq!(IndexType::default(), IndexType::U32);
}

#[test]
fn primitive_and_flag_defaults() {
    assert_eq!(PrimitiveType::default(), PrimitiveType::Triangles);
    let flags = MeshFlags::default();
    assert!(flags.has_positions);
    assert!(!flags.has_normals);
    assert!(!flags.is_indexed);
    assert!(flags.is_static);
}

#[test]
fn mesh_new_defaults() {
    let mesh = Mesh::new("empty");
    assert_eq!(mesh.name, "empty");
    assert_eq!(mesh.vertex_count(), 0);
    assert_eq!(mesh.index_count(), 0);
    assert!(!mesh.has_indices());
    assert_eq!(mesh.index_type(), IndexType::U32);
    assert_eq!(mesh.primitive_type(), PrimitiveType::Triangles);
    assert_eq!(mesh.bounding_box().min, Vec3::ZERO);
    assert_eq!(mesh.bounding_box().max, Vec3::ZERO);
    assert_eq!(mesh.bounding_sphere_radius(), 0.0);
}

#[test]
fn cube_primitives_counts_and_bounds() {
    let cube = Mesh::cube("cube", 2.0);
    assert_eq!(cube.name, "cube");
    assert_eq!(cube.vertex_count(), 24);
    assert_eq!(cube.index_count(), 36);
    assert_eq!(cube.index_count() / 3, 12, "cube has 12 triangles");
    assert!(cube.has_indices());

    let bounds = cube.bounding_box();
    assert!((bounds.min.x - -1.0).abs() < EPSILON);
    assert!((bounds.min.y - -1.0).abs() < EPSILON);
    assert!((bounds.min.z - -1.0).abs() < EPSILON);
    assert!((bounds.max.x - 1.0).abs() < EPSILON);
    assert!((bounds.max.y - 1.0).abs() < EPSILON);
    assert!((bounds.max.z - 1.0).abs() < EPSILON);
    assert!((cube.bounding_sphere_radius() - SQRT_3).abs() < EPSILON);
}

#[test]
fn plane_counts_and_bounds() {
    let plane = Mesh::plane("plane", 10.0, 20.0);
    assert_eq!(plane.vertex_count(), 4);
    assert_eq!(plane.index_count(), 6);
    assert_eq!(plane.index_count() / 3, 2);

    let bounds = plane.bounding_box();
    assert!((bounds.min.x - -5.0).abs() < EPSILON);
    assert!((bounds.max.x - 5.0).abs() < EPSILON);
    assert!((bounds.min.z - -10.0).abs() < EPSILON);
    assert!((bounds.max.z - 10.0).abs() < EPSILON);
    assert!((bounds.min.y).abs() < EPSILON);
    assert!((bounds.max.y).abs() < EPSILON);
}

/// Bug №186: the plane's triangle winding produced a geometric normal of -Y
/// while the vertex normals claimed +Y, so back-face culling made the plane
/// invisible from above. The counts-and-bounds test above passed regardless —
/// it never looked at the winding — so the defect went unnoticed.
#[test]
fn plane_triangles_face_the_same_way_as_their_vertex_normals() {
    let plane = Mesh::plane("plane", 10.0, 20.0);
    let verts = &plane.vertices;
    let idx = &plane.indices;

    for tri in idx.chunks(3) {
        let v0 = verts[tri[0] as usize].position;
        let v1 = verts[tri[1] as usize].position;
        let v2 = verts[tri[2] as usize].position;

        // Geometric normal from the winding: (v1 - v0) x (v2 - v0).
        let a = v1 - v0;
        let b = v2 - v0;
        let n = a.cross(b);
        assert!(
            n.length() > EPSILON,
            "triangle {tri:?} is degenerate, its winding defines no facing"
        );
        let n = n.normalize();

        // Must point the same way as the shading normal, otherwise back-face
        // culling discards the surface even though it is lit as front-facing.
        let shading = verts[tri[0] as usize].normal;
        assert!(
            n.dot(shading) > 0.0,
            "triangle {tri:?} has geometric normal {n:?} but shading normal {shading:?} \
             — back-face culling would discard it"
        );
        // And the plane's shading normal is +Y, so it must be visible from above.
        assert!(
            n.y > 0.0,
            "the plane must face up, got {n:?}"
        );
    }
}

#[test]
fn sphere_counts_and_radius() {
    let sphere = Mesh::sphere("sphere", 1.0, 8, 6);
    assert_eq!(sphere.vertex_count(), 63);
    assert_eq!(sphere.index_count(), 288);
    assert_eq!(sphere.index_count() / 3, 96);

    // With sectors=6 the x samples only reach sin(60deg)=0.866; phi spans
    // 0..=PI so y stays in [0, 1], while z reaches +/-1.0.
    let bounds = sphere.bounding_box();
    let hx = 3.0_f32.sqrt() / 2.0;
    assert!((bounds.min.x + hx).abs() < 0.001);
    assert!((bounds.max.x - hx).abs() < 0.001);
    assert!((bounds.min.y).abs() < 0.001);
    assert!((bounds.max.y - 1.0).abs() < 0.001);
    assert!((bounds.min.z + 1.0).abs() < 0.001);
    assert!((bounds.max.z - 1.0).abs() < 0.001);

    // bounding_sphere_radius is half the AABB diagonal = sqrt(8)/2.
    assert!((sphere.bounding_sphere_radius() - 2.0_f32.sqrt()).abs() < 0.001);
}

#[test]
fn cylinder_counts_and_bounds() {
    let cylinder = Mesh::cylinder("cylinder", 1.0, 2.0, 8);
    assert_eq!(cylinder.vertex_count(), 20);
    assert_eq!(cylinder.index_count(), 96);
    assert_eq!(cylinder.index_count() / 3, 32);

    let bounds = cylinder.bounding_box();
    assert!((bounds.min.x + 1.0).abs() < 0.01);
    assert!((bounds.max.x - 1.0).abs() < 0.01);
    assert!((bounds.min.y + 1.0).abs() < 0.01);
    assert!((bounds.max.y - 1.0).abs() < 0.01);
    assert!((bounds.min.z + 1.0).abs() < 0.01);
    assert!((bounds.max.z - 1.0).abs() < 0.01);
}

#[test]
fn fullscreen_quad_counts() {
    let quad = Mesh::fullscreen_quad("quad");
    assert_eq!(quad.vertex_count(), 4);
    assert_eq!(quad.index_count(), 6);
}

#[test]
fn mesh_own_builder_chain() {
    let mesh = Mesh::new("triangle")
        .with_vertices(vec![
            Vertex::new(Vec3::new(0.0, 0.0, 0.0)),
            Vertex::new(Vec3::new(1.0, 0.0, 0.0)),
            Vertex::new(Vec3::new(0.0, 1.0, 0.0)),
        ])
        .with_indices(vec![0, 1, 2])
        .with_index_type(IndexType::U16);

    assert_eq!(mesh.vertex_count(), 3);
    assert_eq!(mesh.index_count(), 3);
    assert!(mesh.has_indices());
    assert_eq!(mesh.index_type(), IndexType::U16);

    let bounds = mesh.bounding_box();
    assert_eq!(bounds.min, Vec3::ZERO);
    assert_eq!(bounds.max, Vec3::new(1.0, 1.0, 0.0));
    assert!(mesh.bounding_sphere_radius() > 0.0);
}

#[test]
fn mesh_builder_builds_mesh() {
    let mesh = MeshBuilder::new("custom")
        .with_vertices(vec![Vertex::default(); 4])
        .with_indices(vec![0, 1, 2, 0, 2, 3])
        .with_primitive_type(PrimitiveType::TriangleStrip)
        .with_index_type(IndexType::U8)
        .build();

    assert_eq!(mesh.name, "custom");
    assert_eq!(mesh.vertex_count(), 4);
    assert_eq!(mesh.index_count(), 6);
    assert_eq!(mesh.primitive_type(), PrimitiveType::TriangleStrip);
    assert_eq!(mesh.index_type(), IndexType::U8);
}

#[test]
fn mesh_library_crud() {
    let mut library = MeshLibrary::new();
    assert_eq!(library.mesh_count(), 0);

    library.add("cube", Mesh::cube("cube", 1.0));
    library.add("plane", Mesh::plane("plane", 2.0, 2.0));
    assert_eq!(library.mesh_count(), 2);

    let cube = library.get("cube").expect("cube exists");
    assert_eq!(cube.vertex_count(), 24);
    assert!(library.get("missing").is_none());

    let names = library.mesh_names();
    assert!(names.contains(&"cube".to_string()));
    assert!(names.contains(&"plane".to_string()));

    assert!(library.remove("cube").is_some());
    assert_eq!(library.mesh_count(), 1);
    assert!(library.get("cube").is_none());

    library.clear();
    assert_eq!(library.mesh_count(), 0);
}

#[test]
fn mesh_library_create_primitives() {
    let mut library = MeshLibrary::new();
    library.create_primitives();
    assert_eq!(library.mesh_count(), 5);
    let names = library.mesh_names();
    assert!(names.contains(&"sphere".to_string()));
    assert!(names.contains(&"cylinder".to_string()));
    assert!(names.contains(&"fullscreen_quad".to_string()));
}