//! The bridge between the game loop and the server protocol stack.
//!
//! The client's render loop is a single winit thread that never awaits. The
//! server protocol stack (`rfs_net`) is async: its client runs three tasks
//! (receive, send, outbox writer) on a tokio reactor. This module is the
//! seam between the two:
//!
//! ```text
//! menu Play -> NetBridge::start(config)
//!                |  a "rfs-net" worker thread owns a current-thread runtime,
//!                |  binds the UDP socket, calls connect(), and reports the
//!                |  live client back to the render thread through a channel
//!                v
//! render loop: poll() drains ClientEvents and maps them to NetUpdates
//!              send_input() once per observed server tick while Playing
//! menu Quit to Menu / Back -> shutdown() stops the worker and the reactor
//! ```
//!
//! One bridge at a time. The render thread owns the [`NetBridge`]; dropping
//! it (or an explicit `shutdown`) disconnects cleanly and joins the worker.
//!
//! The client itself already stamps what only the connection knows: the
//! session echo (`send_input` overwrites the field) and the wire sequence.
//! This module stays at the level of "what does the loop do with an event".

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rfs_core::packet::{
    ConnectRejectReason, DisconnectReason, GameEvent, InputActions, InputPacket,
};
use rfs_net::client::{ClientConfig, ClientEvent, NetClient};
use rfs_net::Snapshot;
use tokio::sync::mpsc as tokio_mpsc;

use crate::render::camera::controller::CameraMovement;

/// Where the menu's Play connects when `RFS_SERVER` is unset: the dedicated
/// server's default port on the same machine.
pub const DEFAULT_SERVER_ADDR: &str = "127.0.0.1:7777";

/// How long the worker has to report the live client back. Binding a UDP
/// socket is instant; the connection *attempt* itself has the net client's
/// own 5 s timeout, and this deadline is only about the thread answering.
const STARTUP_REPORT_TIMEOUT: Duration = Duration::from_secs(3);

/// The bridge's own settings, gathered from the environment by the caller.
#[derive(Debug, Clone)]
pub struct NetBridgeConfig {
    pub server_addr: SocketAddr,
    pub player_name: String,
    /// `None` or blank is the unsigned dialect, which matches the shipped
    /// `server_config.json` (`allow_unsigned: true`, no token).
    pub auth_token: Option<String>,
}

/// Resolve a `host:port` / `ip:port` string the way the loop can use it.
///
/// `ToSocketAddrs` does the work, so both literal addresses and host names
/// are accepted; the first resolution wins, which is what a game client
/// wants — one endpoint, not a fan-out.
pub fn resolve_server_addr(spec: &str) -> Result<SocketAddr, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("the server address is empty".to_string());
    }
    match spec.to_socket_addrs() {
        Ok(mut addrs) => addrs
            .next()
            .ok_or_else(|| format!("'{spec}' resolved to no address")),
        Err(e) => Err(format!("'{spec}' is not a host:port address ({e})")),
    }
}

/// Build the input packet the loop sends while playing.
///
/// The movement flags come straight from the camera controller, so what the
/// player sees moving is what the server simulates. `tick` is the observed
/// server tick (the server keys its ack by this value) and `session` is left
/// zero: `send_input` overwrites it with the negotiated session, and a
/// caller that never negotiated cannot poison the field.
pub fn input_packet(tick: u32, delta_time: f32, movement: &CameraMovement) -> InputPacket {
    let signed = |pressed: bool, positive: f32| if pressed { positive } else { 0.0 };
    InputPacket {
        tick,
        delta_time: delta_time.clamp(0.0, 0.25),
        move_forward: signed(movement.forward, 1.0) + signed(movement.backward, -1.0),
        move_right: signed(movement.right, 1.0) + signed(movement.left, -1.0),
        move_up: signed(movement.up, 1.0) + signed(movement.down, -1.0),
        // The ship's rudder is a later slice: rotation stays camera-side for
        // now, so the packet carries no rotation intent rather than a wrong
        // one.
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
        actions: InputActions::NONE,
        station_interaction: None,
        session: 0,
    }
}

/// One event the loop acts on. The mapping from `ClientEvent` lives in
/// [`NetBridge::poll`]; this type is what the loop matches on.
#[derive(Debug, Clone, PartialEq)]
pub enum NetUpdate {
    /// The server answered ConnectAccept. The world clock starts here.
    Connected { server_tick: u64 },
    /// The attempt is over and it did not connect (refused, timed out, or
    /// the socket could not be used). The text is for the connecting panel.
    Failed(String),
    /// There was a connection and it is gone (remote disconnect or kick).
    Lost(String),
    /// The operator's `say` announcement, world news for every player.
    Announcement(String),
}

/// One command the render thread can send the worker.
enum NetCommand {
    /// Stop the reactor: disconnect (if still up) and let the runtime drop,
    /// which aborts the net client's internal tasks.
    Shutdown,
}

/// The live link between the render loop and the server.
pub struct NetBridge {
    client: Arc<NetClient>,
    events: tokio_mpsc::Receiver<ClientEvent>,
    commands: Option<tokio_mpsc::UnboundedSender<NetCommand>>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// Counters for the stats line: how much the bridge consumed silently.
    input_acks: u64,
    /// How many times the loop pulled a fresh world state (see
    /// [`NetBridge::take_latest_state`]).
    snapshots_seen: u64,
}

impl NetBridge {
    /// Start the net worker and put a connect attempt on the wire.
    ///
    /// The worker thread owns the runtime and the client for the tasks'
    /// lifetime; the render thread receives `Arc<NetClient>` handles for its
    /// synchronous calls (`send_input`, `is_connected`, `server_tick`). The
    /// `connect()` call happens inside the reactor context on purpose: the
    /// net client spawns its tasks with `tokio::spawn`, which panics outside
    /// a runtime.
    pub fn start(config: NetBridgeConfig) -> Result<Self, String> {
        let (report_tx, report_rx) =
            std::sync::mpsc::channel::<Result<(Arc<NetClient>, tokio_mpsc::Receiver<ClientEvent>), String>>();
        let (commands_tx, mut commands_rx) = tokio_mpsc::unbounded_channel::<NetCommand>();

        let worker = std::thread::Builder::new()
            .name("rfs-net".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(e) => {
                        let _ = report_tx.send(Err(format!("tokio runtime: {e}")));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let net_config = ClientConfig {
                        server_addr: config.server_addr,
                        player_name: config.player_name.clone(),
                        // The server's dialect check pairs this with what it
                        // was started with; the shipped configs agree on
                        // "0.1.0" unsigned.
                        build_version: env!("CARGO_PKG_VERSION").to_string(),
                        auth_token: config.auth_token.clone(),
                        ..ClientConfig::default()
                    };
                    let client = match NetClient::new(net_config).await {
                        Ok(client) => Arc::new(client),
                        Err(e) => {
                            let _ = report_tx.send(Err(format!(
                                "cannot bind the client socket for {}: {e}",
                                config.server_addr
                            )));
                            return;
                        }
                    };
                    let Some(events) = client.get_event_receiver() else {
                        let _ = report_tx
                            .send(Err("the net client produced no event receiver".to_string()));
                        return;
                    };

                    // The attempt starts here and its retry/timeout machinery
                    // lives on this reactor; the menu shows "Connecting..."
                    // until the first event answers it.
                    client.connect();
                    let _ = report_tx.send(Ok((client.clone(), events)));

                    // Park in the command loop: this keeps the reactor driving
                    // the net client's tasks. `std::future::pending` never
                    // resolves, so only a command can break the loop.
                    loop {
                        tokio::select! {
                            _ = std::future::pending::<()>() => unreachable!("pending never resolves"),
                            command = commands_rx.recv() => {
                                match command {
                                    Some(NetCommand::Shutdown) | None => break,
                                }
                            }
                        }
                    }

                    // Local farewell. The server frees the slot either on this
                    // datagram or on its timeout sweep; the client itself is
                    // deliberately silent on a local disconnect (bug №245).
                    client.disconnect(DisconnectReason::ClientQuit);
                });
            })
            .map_err(|e| format!("cannot spawn the net thread: {e}"))?;

        match report_rx.recv_timeout(STARTUP_REPORT_TIMEOUT) {
            Ok(Ok((client, events))) => Ok(Self {
                client,
                events,
                commands: Some(commands_tx),
                worker: Some(worker),
                input_acks: 0,
                snapshots_seen: 0,
            }),
            Ok(Err(message)) => {
                // The worker already exited; join to be sure.
                let _ = worker.join();
                Err(message)
            }
            Err(_) => {
                // Nothing reported in time: tell the worker to stop and wait
                // for it, so no bridge can outlive its owner.
                let _ = commands_tx.send(NetCommand::Shutdown);
                let _ = worker.join();
                Err(format!(
                    "the net thread did not report within {}",
                    STARTUP_REPORT_TIMEOUT.as_secs_f32()
                ))
            }
        }
    }

    /// Drain the events the worker delivered since the last call.
    ///
    /// The world is deliberately NOT among these: the net client coalesces
    /// world state into one slot (bug №175) and hands it out through
    /// [`NetBridge::take_latest_state`], because a queue of stale worlds is
    /// useless — only the newest one is ever rendered. What rides the event
    /// channel is the lifecycle (connect, refuse, disconnect), the
    /// per-entity traffic, and the operator's announcements. Counters cover
    /// the rest, the honest proof the wire is alive.
    pub fn poll(&mut self) -> Vec<NetUpdate> {
        let mut updates = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                ClientEvent::Connected { server_tick, .. } => {
                    updates.push(NetUpdate::Connected { server_tick });
                }
                ClientEvent::ConnectionFailed { reason } => {
                    updates.push(NetUpdate::Failed(describe_reject(reason)));
                }
                ClientEvent::Disconnected { reason } => {
                    updates.push(NetUpdate::Lost(describe_disconnect(reason)));
                }
                // The connect timeout surfaces as a plain error (the retry
                // task has no reject reason to give), so it maps to Failed
                // too — otherwise a dead address would hang the panel forever.
                ClientEvent::Error { error } => {
                    updates.push(NetUpdate::Failed(error));
                }
                ClientEvent::GameEvent {
                    event: GameEvent::Announcement { text },
                } => {
                    updates.push(NetUpdate::Announcement(text));
                }
                ClientEvent::InputAck { .. } => self.input_acks += 1,
                _ => {}
            }
        }
        updates
    }

    /// The live client, for the loop's synchronous calls.
    pub fn client(&self) -> &NetClient {
        &self.client
    }

    /// Whether the server has accepted the connection.
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }

    /// Our own id on the wire, once the server answered ConnectAccept. The
    /// server echoes it in every player snapshot's `player_id`, which is
    /// what binds "our" crew member to us: the display name is not unique
    /// (the server names every connection `player`).
    pub fn connection_id(&self) -> Option<u32> {
        self.client.connection_id()
    }

    /// The newest world state the client holds, if any has arrived.
    ///
    /// Pulled, not pushed: the net client coalesces the whole-world state
    /// into one slot (bug №175), so the loop asks for the latest instead of
    /// draining a queue of worlds that are already stale. Every pull that
    /// returns a snapshot counts in [`NetBridge::snapshots_seen`].
    ///
    /// This slice only consumes it for the stats line — the loop draws its
    /// own local scene. Rendering the server's world is the next slice, and
    /// this is the exact handle it will read.
    pub fn take_latest_state(&mut self) -> Option<Arc<Snapshot>> {
        match self.client.take_latest_state() {
            Some(ClientEvent::StateUpdate { snapshot }) => {
                self.snapshots_seen += 1;
                Some(snapshot)
            }
            // The slot only ever holds a StateUpdate or `None` (the writers
            // are the snapshot/delta apply paths); the catch-all keeps the
            // contract honest if that ever changes.
            Some(_) => None,
            None => None,
        }
    }

    /// How many times a fresh world state was pulled since the bridge was
    /// created.
    pub fn snapshots_seen(&self) -> u64 {
        self.snapshots_seen
    }

    /// How many input acks arrived since the bridge was created.
    pub fn input_acks(&self) -> u64 {
        self.input_acks
    }

    /// Stop the worker and tear the link down. Idempotent: a second call is
    /// a no-op, and `Drop` runs it too.
    pub fn shutdown(&mut self) {
        if let Some(worker) = self.worker.take() {
            if let Some(commands) = &self.commands {
                let _ = commands.send(NetCommand::Shutdown);
            }
            self.commands = None;
            // The worker wakes, disconnects, and drops the runtime, which
            // aborts the net client's tasks.
            let _ = worker.join();
        }
    }
}

impl Drop for NetBridge {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A human line for a connect refusal, for the connecting panel.
fn describe_reject(reason: ConnectRejectReason) -> String {
    match reason {
        ConnectRejectReason::VersionMismatch => "version mismatch".to_string(),
        ConnectRejectReason::ServerFull => "the server is full".to_string(),
        ConnectRejectReason::Banned => "banned".to_string(),
        ConnectRejectReason::InvalidData => "the server refused the data".to_string(),
        ConnectRejectReason::MatchNotFound => "no match is running".to_string(),
        ConnectRejectReason::MatchFull => "the match is full".to_string(),
        ConnectRejectReason::InternalError => "server error".to_string(),
    }
}

/// A human line for a remote disconnect, for the connecting panel.
fn describe_disconnect(reason: DisconnectReason) -> String {
    match reason {
        DisconnectReason::ClientQuit => "the client quit".to_string(),
        DisconnectReason::Kicked => "kicked by the operator".to_string(),
        DisconnectReason::Timeout => "timed out".to_string(),
        DisconnectReason::ServerShutdown => "the server is shutting down".to_string(),
        DisconnectReason::Error => "the server closed the link".to_string(),
        DisconnectReason::Banned => "banned by the operator".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_literal_address_resolves_to_itself() {
        assert_eq!(
            resolve_server_addr("127.0.0.1:7777").unwrap(),
            "127.0.0.1:7777".parse().unwrap()
        );
        assert_eq!(
            resolve_server_addr(" 127.0.0.1:7777 ").unwrap(),
            "127.0.0.1:7777".parse().unwrap()
        );
    }

    #[test]
    fn a_host_name_resolves_through_the_system() {
        // `localhost` needs no network: the resolver answers from the hosts
        // file, so this holds on a machine with no link at all.
        let resolved = resolve_server_addr("localhost:7777").unwrap();
        assert_eq!(resolved.port(), 7777);
    }

    #[test]
    fn a_bad_spec_is_a_message_not_a_panic() {
        assert!(resolve_server_addr("").is_err());
        assert!(resolve_server_addr("no-port-here").is_err());
        assert!(resolve_server_addr("127.0.0.1:99999").is_err());
    }

    #[test]
    fn wasd_map_onto_the_throttle_axes() {
        let mut movement = CameraMovement::default();
        movement.forward = true;
        movement.right = true;
        let packet = input_packet(7, 0.016, &movement);
        assert_eq!(packet.tick, 7);
        assert!((packet.move_forward - 1.0).abs() < 1e-6, "W is full ahead");
        assert!((packet.move_right - 1.0).abs() < 1e-6, "D is full right");
        assert!((packet.move_up).abs() < 1e-6, "no vertical keys pressed");

        let mut movement = CameraMovement::default();
        movement.backward = true;
        movement.left = true;
        let packet = input_packet(8, 0.016, &movement);
        assert!((packet.move_forward + 1.0).abs() < 1e-6, "S is full astern");
        assert!((packet.move_right + 1.0).abs() < 1e-6, "A is full left");
    }

    #[test]
    fn no_keys_is_a_neutral_packet() {
        let movement = CameraMovement::default();
        let packet = input_packet(1, 1.0, &movement);
        assert!((packet.move_forward).abs() < 1e-6);
        assert!((packet.move_right).abs() < 1e-6);
        // A dragged window must not report a second-long step.
        assert!((packet.delta_time - 0.25).abs() < 1e-6);
        assert_eq!(packet.actions, InputActions::NONE);
        assert!(packet.station_interaction.is_none());
        // The session is stamped by send_input, never by this constructor.
        assert_eq!(packet.session, 0);
    }

    #[test]
    fn every_refusal_reads_as_a_line() {
        for reason in [
            ConnectRejectReason::VersionMismatch,
            ConnectRejectReason::ServerFull,
            ConnectRejectReason::Banned,
            ConnectRejectReason::InvalidData,
            ConnectRejectReason::MatchNotFound,
            ConnectRejectReason::MatchFull,
            ConnectRejectReason::InternalError,
        ] {
            let line = describe_reject(reason);
            assert!(!line.is_empty(), "{reason:?} must have a line");
        }
    }

    #[test]
    fn every_disconnect_reason_reads_as_a_line() {
        for reason in [
            DisconnectReason::ClientQuit,
            DisconnectReason::Kicked,
            DisconnectReason::Timeout,
            DisconnectReason::ServerShutdown,
            DisconnectReason::Error,
            DisconnectReason::Banned,
        ] {
            let line = describe_disconnect(reason);
            assert!(!line.is_empty(), "{reason:?} must have a line");
        }
    }

    #[test]
    fn a_fresh_bridge_holds_no_world_state() {
        // The address has no server behind it, but the bridge is created
        // the moment its thread reports — the report does not wait for the
        // handshake — which is the state the loop holds during the first
        // frames of any attempt.
        let mut bridge = NetBridge::start(NetBridgeConfig {
            server_addr: "127.0.0.1:1".parse().unwrap(),
            player_name: "unit-test".to_string(),
            auth_token: None,
        })
        .expect("bridge start");

        assert!(
            bridge.take_latest_state().is_none(),
            "no world state before the server has sent any"
        );
        assert_eq!(bridge.snapshots_seen(), 0);

        bridge.shutdown();
    }
}
