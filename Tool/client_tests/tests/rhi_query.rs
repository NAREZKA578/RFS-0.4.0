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

#[test]
fn query_pool_write_and_get() {
    let mut pool = QueryPool::new(QueryPoolDesc {
        ty: QueryType::Timestamp,
        count: 4,
        flags: QueryPoolFlags::NONE,
    });
    assert_eq!(pool.count(), 4);
    assert!(!pool.is_available(0));

    // Bug №182: a result may only be written for a query that was begun, so
    // the test now brackets it. Previously it wrote straight into slot 0 and
    // the pool accepted a result for a query no GPU work ever issued.
    assert!(pool.begin(0));
    assert!(pool.write(0, QueryResult::Timestamp(12345)));
    assert!(pool.is_available(0));
    assert!(pool.end(0));
    let r = pool.get(0).unwrap();
    match r {
        QueryResult::Timestamp(v) => assert_eq!(*v, 12345),
        _ => panic!("expected timestamp"),
    }
    assert_eq!(pool.get(3), None);
}

#[test]
fn query_pool_take_removes_result() {
    let mut pool = QueryPool::new(QueryPoolDesc {
        ty: QueryType::Occlusion,
        count: 2,
        flags: QueryPoolFlags::NONE,
    });
    assert!(pool.begin(1));
    assert!(pool.write(1, QueryResult::Occlusion(true)));
    assert!(pool.end(1));
    assert!(pool.take(1).is_some());
    assert!(!pool.is_available(1));
    assert_eq!(pool.take(1), None);
}

#[test]
fn query_pool_write_out_of_bounds_is_refused() {
    // Bug №182: this used to be called "..._is_ignored" and asserted nothing
    // about the outcome. An out-of-range write is now reported, not ignored.
    let mut pool = QueryPool::new(QueryPoolDesc {
        ty: QueryType::Timestamp,
        count: 2,
        flags: QueryPoolFlags::NONE,
    });
    assert!(!pool.write(99, QueryResult::Timestamp(1)));
    assert_eq!(pool.get(99), None);
    assert_eq!(pool.rejected_writes(), 1);

    assert!(!pool.write(0, QueryResult::Timestamp(1)), "no query is active");
    assert_eq!(pool.rejected_writes(), 2);
    assert_eq!(pool.get(0), None);
}

#[test]
fn query_pool_reset_clears_results() {
    let mut pool = QueryPool::new(QueryPoolDesc {
        ty: QueryType::AccelerationStructureCompactionSize,
        count: 8,
        flags: QueryPoolFlags::NONE,
    });
    assert!(pool.begin(0));
    assert!(pool.write(0, QueryResult::CompactionSize(4096)));
    assert!(pool.is_available(0));
    pool.reset();
    assert!(!pool.is_available(0));
    assert_eq!(pool.get(0), None);
    // Bug №182: reset also ends every open query, so a stale run cannot
    // contribute a result to the next one.
    assert!(!pool.is_active(0));
    assert!(!pool.write(0, QueryResult::CompactionSize(1)));
}

#[test]
fn query_result_default_for_types() {
    let ts = QueryResult::default_for(QueryType::Timestamp);
    assert!(matches!(ts, QueryResult::Timestamp(0)));
    let occ = QueryResult::default_for(QueryType::Occlusion);
    assert!(matches!(occ, QueryResult::Occlusion(false)));
    let stats = QueryResult::default_for(QueryType::PipelineStatistics);
    assert!(matches!(stats, QueryResult::PipelineStatistics(_)));
}