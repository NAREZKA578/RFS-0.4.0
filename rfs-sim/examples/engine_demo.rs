//! Headless-harness движка: кадр без GPU + сим + сеть.
//!
//! Прогоняет ровно те сценарии, которые в доноре ломались только «в игре»:
//! дельта-вайп (№219), голодание интерполятора (№220), reconnect-утечка (№221).

use glam::Vec3;
use rfs_engine_net::{
    sanitize_chat_msg, ChatLimits, Delta, EventQueue, InterpolationBuffer, SnapshotView,
};
use rfs_engine_render::{Backend, FrameDescriptor, NullBackend};
use rfs_sim::SessionStore;

fn main() {
    // 1. Кадр без GPU: двойной begin обязан быть ошибкой, а не утечкой.
    let backend = NullBackend::new();
    let frame = FrameDescriptor {
        width: 1280,
        height: 720,
        vsync: true,
    };
    backend.begin_frame(&frame).unwrap();
    assert!(backend.begin_frame(&frame).is_err());
    backend.end_frame().unwrap();
    println!(
        "[render] NullBackend frame contract ok ({:?})",
        backend.gpu_info().backend
    );

    // 2. Сеть: полный снапшот, затем 9 пустых дельт — состояние стоит.
    let mut view = SnapshotView::default();
    view.apply_snapshot(
        vec![rfs_engine_net::snapshot::Fire {
            id: 1,
            pos: [10.0, 0.0, 5.0],
            intensity: 80.0,
        }],
        vec![],
        vec![],
    );
    let mut interp = InterpolationBuffer::new(0.1);
    interp.push_snapshot(&[(1, Vec3::new(0.0, 0.0, 0.0))], 1.0);
    for i in 1..=9u64 {
        let t = 1.0 + i as f64 / 64.0;
        view.apply_delta(&Delta::default()); // пустая дельта: ничего не стирает (№219)
        interp.push_delta(&[(1, Vec3::new(i as f32, 0.0, 0.0))], t); // дельта двигает буфер (№220)
    }
    assert_eq!(view.fires.len(), 1);
    println!(
        "[net] delta-merge ok (fires={}), interp t={:.3}, sample={:?}",
        view.fires.len(),
        interp.interp_time,
        interp.sample(1)
    );

    // 3. События: drain возвращает, вызывающий обрабатывает.
    let mut q = EventQueue::default();
    q.push("SoundEvent:fire_crackle@50m");
    let got = q.drain(); // №222: никакого дропа в лог внутри
    println!("[net] events drained to caller: {:?}", got);

    // 4. Сим: reconnect с того же адреса не наследует carry.
    let mut sessions = SessionStore::new();
    let old = sessions.connect();
    sessions.pick_up(7, old);
    let ev = sessions.remove_session(old); // единый путь и для reconnect (№221)
    println!(
        "[sim] reconnect cleanup: drops={:?} hydrants={:?} recycled={:?}",
        ev.civilian_drops, ev.hydrant_disconnects, ev.recycled_id
    );

    // 5. Чат и персист из конфига, не из хардкода.
    let limits = ChatLimits {
        max_name_len: 10,
        max_chat_len: 20,
    };
    let (n, m) = sanitize_chat_msg("Командир_караула", "Принято, выдвигаемся!", &limits); // №223
    println!("[net] chat: {n:?} :: {m:?}");
    let path = std::env::temp_dir().join("rfs-engine-demo.json");
    rfs_sim::atomic_write(&path, br#"{"demo":true}"#).unwrap(); // №224
    println!("[sim] atomic write ok: {}", path.display());

    println!("engine_demo: ALL OK");
}
