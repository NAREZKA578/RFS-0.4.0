// Bug №268 (server tick clock) and bug №269 (client world clock).
//
// Both bugs are the same mistake in two places: a tick number that is supposed
// to mean "the world is here" was allowed to move independently of the world.
// These tests pin the two rules that fix them.
use rfs_core::time::{Tick, TICK_DURATION};
use rfs_net::{advance_world_clock, NetServer, ServerConfig};

/// The client-side rule: layers arrive at different rates, so an older tick
/// from a slower layer lands after a newer one from a faster layer all the time.
/// The world clock must ignore it.
#[test]
fn a_slower_layer_cannot_rewind_the_client_world_clock() {
    let mut tick = Tick(0);
    let mut time = 0.0;

    // Layer 2 (players) publishes every tick and gets there first.
    assert!(advance_world_clock(&mut tick, &mut time, 104, 104.0 * 0.0333));
    assert_eq!(tick, Tick(104));

    // Layer 0 (ships) publishes every second tick and is now behind. Before
    // the fix this overwrote the clock back to 100, and the stress harness
    // counts every change of server_tick() as an observed tick — so the rewind
    // was booked as delivery that never happened.
    assert!(!advance_world_clock(&mut tick, &mut time, 100, 100.0 * 0.0333));
    assert_eq!(tick, Tick(104), "an older layer must not rewind the clock");
    assert_eq!(
        time,
        104.0 * 0.0333,
        "the time must not rewind either, and must stay in step with the tick"
    );
}

/// A repeat of the current tick is not a stall and not a rewind: it changes
/// nothing, and in particular must not rewrite the time (a resend of the same
/// tick carries the same state, not a new one).
#[test]
fn the_same_tick_again_changes_nothing() {
    let mut tick = Tick(50);
    let mut time = 1.5;
    assert!(!advance_world_clock(&mut tick, &mut time, 50, 999.0));
    assert_eq!(tick, Tick(50));
    assert_eq!(time, 1.5, "a repeat must not restamp the time");
}

#[tokio::test]
async fn the_server_publishes_the_simulation_tick_and_never_rewinds() {
    let config = ServerConfig {
        // Port 0: let the OS pick, so the test never collides with a real
        // server or with a parallel run.
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        ..Default::default()
    };
    let net = NetServer::new(config).await.expect("bind loopback");

    // The send loop no longer owns a counter, so the only writer is the
    // simulation. Before publishing anything the world sits at tick 0.
    assert_eq!(net.current_tick(), Tick(0));

    // The simulation advances and publishes.
    net.publish_tick(Tick(30));
    assert_eq!(net.current_tick(), Tick(30));
    assert_eq!(
        net.server_time(),
        30.0 * TICK_DURATION.as_secs_f64(),
        "the published time must be derived from the published tick, not kept \
         on a separate cadence"
    );

    // A caller that reports an older tick — a retransmit, a second publisher,
    // anything out of order — must not rewind the world for every client.
    net.publish_tick(Tick(12));
    assert_eq!(net.current_tick(), Tick(30), "an older tick must be ignored");

    // Repeating the current tick is harmless and does not restamp the time.
    let time_before = net.server_time();
    net.publish_tick(Tick(30));
    assert_eq!(net.current_tick(), Tick(30));
    assert_eq!(net.server_time(), time_before);

    // Forward progress still works after a rejected rewind.
    net.publish_tick(Tick(31));
    assert_eq!(net.current_tick(), Tick(31));
    assert_eq!(
        net.server_time(),
        31.0 * TICK_DURATION.as_secs_f64()
    );

    net.stop();
}
