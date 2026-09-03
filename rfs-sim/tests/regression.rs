//! Сквозные регрессии форка (дублируют юнит-тесты крейтов на уровне связки).

use glam::Vec3;
use rfs_engine_net::{Delta, InterpolationBuffer, SnapshotView};
use rfs_sim::SessionStore;

#[test]
fn full_tick_cycle_no_wipe_no_starve() {
    let mut view = SnapshotView::default();
    view.apply_snapshot(
        vec![rfs_engine_net::snapshot::Fire {
            id: 1,
            pos: [0.0, 0.0, 0.0],
            intensity: 60.0,
        }],
        vec![],
        vec![],
    );
    let mut interp = InterpolationBuffer::new(0.1);
    interp.push_snapshot(&[(9, Vec3::ZERO)], 100.0);
    // 1 полный + 9 дельт как на сервере 64Hz / FULL_UPDATE_INTERVAL=10.
    for i in 1..=9u64 {
        let t = 100.0 + i as f64 / 64.0;
        view.apply_delta(&Delta::default());
        interp.push_delta(&[(9, Vec3::new(i as f32, 0.0, 0.0))], t);
    }
    assert_eq!(view.fires.len(), 1, "delta must not wipe (219)");
    assert!(interp.sample(9).is_some(), "delta must feed interp (220)");
}

#[test]
fn session_recycle_is_clean() {
    let mut s = SessionStore::new();
    let a = s.connect();
    s.pick_up(3, a);
    s.connect_hydrant(5, a);
    let ev = s.remove_session(a);
    assert_eq!(ev.civilian_drops, vec![3]);
    assert_eq!(ev.hydrant_disconnects, vec![5]);
    let b = s.connect();
    assert_eq!(s.carrier_of(3), None);
    assert_eq!(s.hydrant_owner(5), None);
    let _ = b;
}
