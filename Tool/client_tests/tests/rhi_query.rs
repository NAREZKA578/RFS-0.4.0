// Integration tests for the rhi::query module.

use rhi::query::pool::{QueryPool, QueryPoolDesc, QueryPoolFlags, QueryType};
use rhi::query::results::{PipelineStatistics, QueryResult};

#[test]
fn query_type_variants() {
    assert_eq!(QueryType::default(), QueryType::Occlusion);
    let _ = QueryType::BinaryOcclusion;
    let _ = QueryType::Timestamp;
    let _ = QueryType::PipelineStatistics;
    let _ = QueryType::AccelerationStructureCompactionSize;
}

#[test]
fn query_pool_flags() {
    assert_eq!(QueryPoolFlags::NONE, QueryPoolFlags::empty());
}

#[test]
fn query_pool_desc_creation() {
    let desc = QueryPoolDesc {
        ty: QueryType::Timestamp,
        count: 256,
        flags: QueryPoolFlags::NONE,
    };
    assert_eq!(desc.count, 256);
    assert_eq!(desc.ty, QueryType::Timestamp);
}

#[test]
fn query_pool_new_and_desc() {
    let pool = QueryPool::new(QueryPoolDesc {
        ty: QueryType::Occlusion,
        count: 32,
        flags: QueryPoolFlags::NONE,
    });
    assert_eq!(pool.desc().count, 32);
    assert_eq!(pool.desc().ty, QueryType::Occlusion);
}

#[test]
fn pipeline_statistics_default() {
    let stats = PipelineStatistics::default();
    assert_eq!(stats.input_assembly_vertices, 0);
    assert_eq!(stats.vertex_shader_invocations, 0);
    assert_eq!(stats.compute_shader_invocations, 0);
}

#[test]
fn pipeline_statistics_fields() {
    let stats = PipelineStatistics {
        input_assembly_vertices: 100,
        input_assembly_primitives: 50,
        vertex_shader_invocations: 100,
        fragment_shader_invocations: 100,
        compute_shader_invocations: 10,
    };
    assert_eq!(stats.input_assembly_vertices, 100);
}

#[test]
fn query_result_variants() {
    let q = QueryResult::Occlusion(true);
    assert_eq!(format!("{:?}", q), "Occlusion(true)");

    let q = QueryResult::Timestamp(123);
    assert_eq!(format!("{:?}", q), "Timestamp(123)");

    let q = QueryResult::CompactionSize(4096);
    assert_eq!(format!("{:?}", q), "CompactionSize(4096)");

    let q = QueryResult::PipelineStatistics(PipelineStatistics::default());
    match q {
        QueryResult::PipelineStatistics(stats) => assert_eq!(stats.input_assembly_vertices, 0),
        _ => panic!("expected pipeline statistics"),
    }
}