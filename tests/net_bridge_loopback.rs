//! The bridge's acceptance tests: a real `NetServer` on the loopback, driven
//! by the same `NetBridge` the render loop owns.
//!
//! # Why this test exists
//!
//! `src/net.rs`'s unit tests prove the event mapping — what the loop does
//! with a `ClientEvent` — but they run against made-up events. The net
//! crate's own suite proves the protocol, but it never leaves that crate.
//! The seam between the two is exactly where integration bugs live: a
//! worker thread that panics outside a runtime, a report channel that
//! never answers, a shutdown that leaves a session registered on the
//! server. These tests start the bridge the way `main`'s Play item does —
//! a synchronous `start()` that spawns the "rfs-net" thread — and let the
//! wire answer.
//!
//! The server side is a bare `NetServer` on an OS-assigned loopback port,
//! the same helper pattern the net crate's `disconnect_tests` and
//! `auth_pair_tests` use. It accepts handshakes and frees slots, which is
//! all a handshake test needs; snapshot broadcasting and input acks live
//! in the server crate's tick loop above the net layer, so a bare server
//! is honest about what it can prove. The full input-ack loop is exercised
//! against the real server binary in the manual smoke run.
//!
//! # Runtime layout
//!
//! The test's `#[tokio::test]` current-thread reactor hosts the server's
//! tasks; the bridge runs on its own worker thread and its own runtime,
//! exactly as it does under `main`. Nothing here reaches into the bridge's
//! internals — only the public surface the render loop uses: `start`,
//! `poll`, `is_connected`, `shutdown`, `client`.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rfs_client::net::{NetBridge, NetBridgeConfig, NetUpdate};
use rfs_net::{NetServer, ServerConfig};

/// How long a test waits for the wire to answer before declaring it dead.
/// The client's own connect timeout is 5 s, so a hung attempt cannot be
/// waited out faster than that; the deadline exists to fail loudly instead
/// of hanging the suite.
const WIRE_DEADLINE: Duration = Duration::from_secs(6);

/// The poll cadence: fine enough that a loopback handshake is seen within
/// a few rounds, coarse enough not to spin the test thread bare.
const POLL_PAUSE: Duration = Duration::from_millis(5);

/// Start a bare net server on an OS-assigned loopback port, run it, and
/// return it with its address.
async fn bare_server() -> (NetServer, SocketAddr) {
    let server = NetServer::new(ServerConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        ..ServerConfig::default()
    })
    .await
    .expect("server bind");
    let addr = server.local_addr().expect("server local_addr");
    server.start();
    (server, addr)
}

/// Start the bridge against `addr`, the way `main`'s Play does: the config
/// the environment would have produced, no token (the unsigned dialect the
/// shipped `server_config.json` accepts).
fn bridge_to(addr: SocketAddr) -> NetBridge {
    NetBridge::start(NetBridgeConfig {
        server_addr: addr,
        player_name: "bridge-test".to_string(),
        auth_token: None,
    })
    .expect("bridge start")
}

/// Poll the bridge until an update satisfies `want`, or fail at the
/// deadline. Returns the first matching update.
///
/// The pauses are `tokio::time::sleep`, not `std::thread::sleep`, on
/// purpose: the test's reactor hosts the server's tasks, and a blocked
/// test body would park them too — the handshake could never complete and
/// every deadline test would fail against a healthy wire.
async fn poll_until(
    bridge: &mut NetBridge,
    want: impl Fn(&NetUpdate) -> bool,
    note: &str,
) -> NetUpdate {
    let deadline = Instant::now() + WIRE_DEADLINE;
    loop {
        assert!(Instant::now() < deadline, "{note}: timed out");
        for update in bridge.poll() {
            if want(&update) {
                return update;
            }
        }
        tokio::time::sleep(POLL_PAUSE).await;
    }
}

/// Wait until the server has registered its side of the handshake. The
/// session appears only after the Accept round-trips, so the tests wait
/// instead of assuming — the same pattern the net crate's disconnect
/// tests use.
async fn wait_for_session(server: &NetServer, note: &str) {
    let deadline = Instant::now() + WIRE_DEADLINE;
    while server.connection_count() == 0 {
        assert!(
            Instant::now() < deadline,
            "{note}: the server never registered the session"
        );
        tokio::time::sleep(POLL_PAUSE).await;
    }
}

/// Wait until the server holds no session. Bug №68 on the server side: the
/// slot is freed by the Disconnect datagram itself, not by the timeout
/// sweep, so a clean leave is a short wait.
async fn wait_for_empty(server: &NetServer, note: &str) {
    let deadline = Instant::now() + WIRE_DEADLINE;
    while server.connection_count() != 0 {
        assert!(
            Instant::now() < deadline,
            "{note}: the server kept a slot after the bridge left"
        );
        tokio::time::sleep(POLL_PAUSE).await;
    }
}

#[tokio::test]
async fn the_bridge_reaches_a_live_server_and_reports_connected() {
    let (server, addr) = bare_server().await;
    let mut bridge = bridge_to(addr);

    let connected = poll_until(
        &mut bridge,
        |u| matches!(u, NetUpdate::Connected { .. }),
        "Connected",
    )
    .await;
    let NetUpdate::Connected { server_tick } = connected else {
        unreachable!("poll_until returned the matched update");
    };
    // The bare server has no match loop above it, so its tick clock does
    // not advance on its own; a zero is a correct answer from this server,
    // and the field itself is what the loop needs to start its own clock.
    let _ = server_tick;
    assert!(
        bridge.is_connected(),
        "the bridge reports its own link state"
    );

    wait_for_session(&server, "Connected").await;

    // `shutdown` is what the menu's Quit to Menu calls. It must join its
    // worker before returning — after it, no thread of this bridge lives.
    bridge.shutdown();
    assert!(!bridge.is_connected(), "the bridge is down after shutdown");

    wait_for_empty(&server, "shutdown").await;
    server.stop();
}

#[tokio::test]
async fn backing_out_of_a_connecting_attempt_leaves_no_session_behind() {
    // The Back item on the connecting panel can fire before the handshake
    // has finished — on a remote server that is the normal case, a
    // loopback one may already be connected by the time the command
    // arrives. Both outcomes must end the same way: the worker joined, no
    // session left registered. This test leans on the timing being
    // unfixed: whichever side of the handshake shutdown lands on, the
    // assertions below hold.
    let (server, addr) = bare_server().await;
    let mut bridge = bridge_to(addr);

    bridge.shutdown();
    assert!(
        !bridge.is_connected(),
        "the bridge is down after backing out"
    );

    wait_for_empty(&server, "backed out").await;
    server.stop();
}

#[tokio::test]
async fn a_second_shutdown_is_a_no_op() {
    let (server, addr) = bare_server().await;
    let mut bridge = bridge_to(addr);
    poll_until(
        &mut bridge,
        |u| matches!(u, NetUpdate::Connected { .. }),
        "Connected",
    )
    .await;

    bridge.shutdown();
    // Idempotence is a documented promise (the menu may cancel, the player
    // may quit to menu, Drop runs it again): a second call must not panic
    // and must not resurrect anything.
    bridge.shutdown();

    wait_for_empty(&server, "double shutdown").await;
    server.stop();
}
