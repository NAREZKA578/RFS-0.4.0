use crate::bandwidth::{BandwidthLimiter, BandwidthTracker};
use crate::channel::{Channel, ChannelConfig};
use crate::fragment::{make_fragments, FragmentAssembler, FRAGMENT_HEADER_SIZE};
use crate::snapshot::LAYER_COUNT;
use parking_lot::Mutex;
use rfs_core::packet::*;
use rfs_core::time::TICK_RATE;
use std::collections::{BTreeSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::warn;
use uuid::Uuid;

/// Bug №239: this is a re-export of the protocol's single wire budget, not a
/// second number. The core constant is the one place the MTU is stated; a
/// restatement here would drift silently the next time someone alters either
/// copy.
pub const MAX_PACKET_SIZE: usize = rfs_core::packet::MAX_PACKET_SIZE;

/// Bug №239 (addendum): the deserialize cap for one message — the number the
/// DoS guard in `server.rs` and `client.rs` enforces before any payload is
/// believed (bug №732 reworded the guard from a bincode Option to an explicit
/// length check; the number and its authority did not move). Those two files
/// used to restate `1024 * 1024` independently, which is precisely the defect
/// an alias exists to stop: a wire number lives here, once, and the framings
/// alias it. (The fragment
/// reassembly ceiling, `MAX_REASSEMBLED_SIZE`, is a different limit by role
/// and stays its own statement — see the registry entry.)
pub const WIRE_LIMIT: u64 = 1024 * 1024;

/// Bug №700: what one datagram may carry as payload — the wire budget
/// (the number the builders fragment against, `max_packet_size`) minus the
/// packet header and minus the authentication block both sides now pay
/// before the header. One statement, here, so the seven packet builders
/// never grow private copies of the arithmetic.
pub fn max_payload_len(config: &ConnectionConfig) -> usize {
    config
        .max_packet_size
        .saturating_sub(HEADER_SIZE + rfs_core::packet::AUTH_WIRE_LEN)
}
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

/// Bug №215: retransmit cadence. 250 ms is a conservative RTO for the
/// loopback/LAN profiles this server actually runs; the retry budget bounds
/// a retransmit storm to ~2.5 s, after which the entry is declared lost and
/// the connection timeout finishes the session.
pub const RETRANSMIT_TIMEOUT_MS: u64 = 250;
pub const RETRANSMIT_MAX_RETRIES: u32 = 10;
/// Entries one pass may re-emit per connection; the next pass 33 ms later
/// naturally throttles a burst instead of spending a whole tick socket on it.
pub const RETRANSMIT_MAX_PER_PASS: usize = 16;
pub const MAX_RELIABLE_WINDOW: u32 = 32;

/// Bug №167: how many out-of-order reliable sequences the SACK tracker keeps
/// before the oldest gap is forgotten. A forgotten gap simply means the peer
/// keeps that entry in its queue a little longer — it is not a correctness
/// problem, only a memory/latency trade-off.
const SACK_TRACKED_GAPS: usize = 256;

/// Wrap-aware "is `a` newer than `b`?" for u32 transport sequences
/// (bug №11): plain `>` freezes at the MAX->0 wrap, this doesn't.
pub fn seq_is_newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < (1 << 31)
}

/// Bug №176: a sample can be nonsensical — a heartbeat ack that arrives long
/// after we gave up waiting, or a clock reading of zero. Left unclamped, one
/// such sample poisons the smoothed average for a long time and any adaptive
/// logic driven by it (interpolation delay, resync cadence) drifts with it.
pub const RTT_MIN: Duration = Duration::from_millis(1);
pub const RTT_MAX: Duration = Duration::from_secs(5);

/// EWMA with a 3/4 weight on history, clamped to [RTT_MIN, RTT_MAX].
pub fn smooth_rtt(current: Duration, sample: Duration) -> Duration {
    let sample = sample.clamp(RTT_MIN, RTT_MAX);
    let blended = (current.as_millis() as u64 * 3 + sample.as_millis() as u64) / 4;
    Duration::from_millis(blended.clamp(RTT_MIN.as_millis() as u64, RTT_MAX.as_millis() as u64))
}

/// Decode the channel byte from a wire header. Unknown values are treated as
/// unreliable, which keeps the SACK tracker from advancing on garbage.
pub fn channel_from_u8(value: u8) -> ChannelType {
    match value {
        x if x == ChannelType::Unreliable as u8 => ChannelType::Unreliable,
        x if x == ChannelType::ReliableOrdered as u8 => ChannelType::ReliableOrdered,
        x if x == ChannelType::ReliableUnordered as u8 => ChannelType::ReliableUnordered,
        _ => ChannelType::UnreliableSequenced,
    }
}

/// Does this channel promise delivery?
///
/// Used to decide whether an exhausted bandwidth budget may discard a packet
/// (bug №171): unreliable traffic is replaceable, reliable traffic is not
/// until a retransmit queue exists.
pub fn is_reliable_channel(channel: u8) -> bool {
    matches!(
        channel_from_u8(channel),
        ChannelType::ReliableOrdered | ChannelType::ReliableUnordered
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
}

/// Bug №215: what `track_received_reliable` decided about a reliable sequence.
/// `Duplicate` is receiver-side suppression: a retransmitted payload whose
/// first delivery the peer never acked (our ack was lost) must not apply
/// twice. `None` is "not a reliable channel at all".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReliableArrival {
    New,
    Duplicate,
}

/// What `handle_packet` hands back: the reply packets for the transport plus
/// the arrival verdict the caller's dispatch needs for reliable payloads.
#[derive(Debug)]
pub struct PacketOutcome {
    /// Packets to send back (server.rs `send_packet`, client handler).
    pub outgoing: Vec<OutgoingPacket>,
    /// Reliable-arrival verdict for THIS datagram (`None` on unreliable
    /// channels). Command processing gates on `Some(New)`: a retransmitted
    /// Command must not run twice just because our ack of the first one was
    /// lost in transit.
    pub reliable_arrival: Option<ReliableArrival>,
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub max_packet_size: usize,
    pub connection_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub reliable_window: u32,
    pub send_buffer_size: usize,
    pub receive_buffer_size: usize,
    pub channels: Vec<ChannelConfig>,
    /// Bug №176: the server used one process-wide limiter for every
    /// connection, so one noisy client spent everyone else's budget. Each
    /// connection now carries its own bucket.
    pub max_bandwidth_bps: f64,
    /// Bug №671: the connection's own rate tracker lives on this window.
    /// Default is one second; the server passes its configured value down.
    pub bandwidth_window: Duration,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_packet_size: MAX_PACKET_SIZE,
            connection_timeout: CONNECTION_TIMEOUT,
            heartbeat_interval: HEARTBEAT_INTERVAL,
            reliable_window: MAX_RELIABLE_WINDOW,
            send_buffer_size: 1024 * 1024,
            receive_buffer_size: 1024 * 1024,
            max_bandwidth_bps: 512.0 * 1024.0,
            bandwidth_window: Duration::from_secs(1),
            channels: vec![
                ChannelConfig::new(ChannelType::UnreliableSequenced, 100),
                // Bug №215: the reliable ledgers hold retransmittable payload
                // bytes. A burst of commands must not evict a queued ack
                // answer, hence the wider buffer than the unreliable group.
                ChannelConfig::new(ChannelType::ReliableOrdered, 1000),
                ChannelConfig::new(ChannelType::ReliableUnordered, 100),
            ],
        }
    }
}

pub struct Connection {
    pub id: u32,
    pub addr: SocketAddr,
    pub state: Mutex<ConnectionState>,
    pub config: ConnectionConfig,
    pub channels: Vec<Channel>,
    pub local_sequence: Mutex<u32>,
    pub remote_sequence: Mutex<u32>,
    pub remote_ack: Mutex<u32>,
    pub remote_ack_bitfield: Mutex<u32>,
    pub pending_acks: Mutex<VecDeque<u32>>,
    /// Bug №272: how many entries `pending_acks` has dropped to its window cap.
    ///
    /// The drop itself cannot be avoided today — nothing acks a reliable
    /// packet, because there is no retransmit layer (see `track_reliable`) —
    /// but LOGGING each drop was costing more than the work it described. At
    /// 200 bots one 30 s run produced 20127 of these lines, all from the tick
    /// loop, and the `sim` phase grew 5.5 -> 14.9 ms with a constant 8 ships
    /// and 200 players. Same reasoning already applied to layer stalls in
    /// client.rs: a per-occurrence log turns the recovery path into the load
    /// generator. The count stays exact and is read by the caller that reports
    /// it; the log is gone.
    pub pending_ack_drops: AtomicU64,
    /// First dropped sequence, kept so a periodic summary can say which window
    /// overflowed rather than only how often.
    pub first_pending_ack_drop: Mutex<Option<u32>>,
    pub last_received: Mutex<Instant>,
    pub last_sent: Mutex<Instant>,
    pub last_heartbeat: Mutex<Instant>,
    pub rtt: Mutex<Duration>,
    pub bandwidth: BandwidthTracker,
    /// Bug №219: when this connection came into being. An in-flight handshake
    /// cannot be distinguished from a stalled one by `last_received` (a live
    /// client keeps the heartbeat moving), so the "player never registered"
    /// reaper needs its own clock anchored at connection creation.
    connected_since: Instant,
    pub client_id: Mutex<Option<u32>>,
    pub player_entity: Mutex<Option<EntityId>>,
    pub ship_entity: Mutex<Option<EntityId>>,
    pub interest_entities: Mutex<Vec<EntityId>>,
    pub next_fragment_id: Mutex<u32>,
    pub fragment_assembler: Mutex<FragmentAssembler>,
    /// Bug №167: highest CONTIGUOUS reliable sequence received from the peer.
    /// Everything at or below it is acknowledged cumulatively.
    reliable_hcr: Mutex<u32>,
    /// Bug №167: reliable sequences received ABOVE `reliable_hcr` (out of order).
    /// These become the SACK bits, so a gap no longer pins the peer's queue.
    reliable_gaps: Mutex<BTreeSet<u32>>,
    /// Bug №176: this connection's own send budget. A single shared server-wide
    /// limiter let one chatty client starve every other connection.
    bandwidth_limiter: BandwidthLimiter,
    /// Bug №267 (A): layers this client asked to resync immediately. Consumed
    /// by the snapshot pass on the next tick, so a stalled layer heals in one
    /// round trip instead of waiting out the scheduled cadence.
    resync_requests: Mutex<[bool; LAYER_COUNT]>,
    /// Bug №30 (B): the applied ticks this client REPORTED per layer
    /// (LayerSync). Monotonic by merge: a duplicated or reordered report can
    /// only be stale, never a rewind — the confirmed base of a layer moves
    /// forward onto these numbers, never across them. This is the server's
    /// half of №30's fix: it is what replaces the old "the base is whatever
    /// we last queued" bookkeeping.
    reported_applied: Mutex<[u64; LAYER_COUNT]>,
    /// Bug №742: has ANY LayerSync report arrived yet. Applied tick 0 is a
    /// real, reportable value (the client applied a full state whose tick
    /// happened to be 0), so "no measurement yet" cannot be read off
    /// `reported_applied` itself — the applied-lag gauge needs this one bit
    /// to know it holds a datapoint and not the pre-first-report void.
    report_seen: AtomicBool,
    /// Bug №700: this connection's outbound auth sequence. Starts at 1 — a
    /// zero sequence is the unsigned dialect — and skips 0 on wrap.
    pub(crate) wire_seq: Mutex<u64>,
    /// Bug №216: this connection's session number. Minted once by whoever
    /// creates the connection and communicated to the client inside
    /// `ConnectAcceptPacket`; every inbound Heartbeat/Input/Command must echo
    /// it and every outbound packet of those types carries it. A value of 0
    /// means "not negotiated yet" — the client carries that only before its
    /// first accept.
    session: Mutex<u128>,
}

/// Bug №216: the session value meaning "no session negotiated". The accept
/// always carries a minted (non-zero) value; zero on the wire means the
/// sender never received an accept and its packets are refused.
pub const SESSION_UNSET: u128 = 0;

impl Connection {
    pub fn new(id: u32, addr: SocketAddr, mut config: ConnectionConfig) -> Self {
        let channels = config
            .channels
            .iter()
            .map(|c| Channel::new(c.clone()))
            .collect();
        // Bug №10: a max_packet_size below the header size would underflow
        // every `max_packet_size - HEADER_SIZE`. Clamp once, loudly.
        // Bug №700: the auth block rides on EVERY datagram, so with a key on
        // the wire the payload room shrinks by AUTH_WIRE_LEN; a maximum that
        // doesn't acknowledge that would push signed traffic over MTU while
        // passing an unsigned check.
        if config.max_packet_size <= HEADER_SIZE {
            warn!(
                "max_packet_size {} <= HEADER_SIZE {}, clamping",
                config.max_packet_size, HEADER_SIZE
            );
            config.max_packet_size = HEADER_SIZE + 1;
        }
        if config.max_packet_size <= HEADER_SIZE + AUTH_WIRE_LEN {
            warn!(
                "max_packet_size {} <= HEADER_SIZE {} + AUTH_WIRE_LEN {}, clamping",
                config.max_packet_size, HEADER_SIZE, AUTH_WIRE_LEN
            );
            config.max_packet_size = HEADER_SIZE + AUTH_WIRE_LEN + 1;
        }

        let limiter = BandwidthLimiter::new(config.max_bandwidth_bps);
        let bandwidth_window = config.bandwidth_window;

        Self {
            id,
            addr,
            state: Mutex::new(ConnectionState::Connecting),
            config,
            channels,
            local_sequence: Mutex::new(0),
            remote_sequence: Mutex::new(0),
            remote_ack: Mutex::new(0),
            remote_ack_bitfield: Mutex::new(0),
            pending_acks: Mutex::new(VecDeque::with_capacity(64)), // №755 prealloc
            pending_ack_drops: AtomicU64::new(0),
            first_pending_ack_drop: Mutex::new(None),
            last_received: Mutex::new(Instant::now()),
            last_sent: Mutex::new(Instant::now()),
            last_heartbeat: Mutex::new(Instant::now()),
            rtt: Mutex::new(Duration::from_millis(100)),
            bandwidth: BandwidthTracker::with_window(bandwidth_window),
            connected_since: Instant::now(),
            client_id: Mutex::new(None),
            player_entity: Mutex::new(None),
            ship_entity: Mutex::new(None),
            interest_entities: Mutex::new(Vec::new()),
            next_fragment_id: Mutex::new(0),
            fragment_assembler: Mutex::new(FragmentAssembler::new()),
            reliable_hcr: Mutex::new(0),
            reliable_gaps: Mutex::new(BTreeSet::new()),
            bandwidth_limiter: limiter,
            resync_requests: Mutex::new([false; LAYER_COUNT]),
            reported_applied: Mutex::new([0; LAYER_COUNT]),
            report_seen: AtomicBool::new(false),
            wire_seq: Mutex::new(0),
            // Bug №216: every connection mints its own session — never zero,
            // so an unset session cannot be forged by a value.
            session: Mutex::new(Uuid::new_v4().as_u128()),
        }
    }

    /// Bug №216: the session this connection negotiates. Inbound packets of
    /// the gated types must echo it; read before client-side sends too, so
    /// both sides stamp their datagrams with the same number.
    pub fn session(&self) -> u128 {
        *self.session.lock()
    }

    /// Bug №216: the client installs the session it learned from the accept.
    /// The server never needs this — it minted the value itself.
    pub fn set_session(&self, session: u128) {
        *self.session.lock() = session;
    }

    pub fn state(&self) -> ConnectionState {
        *self.state.lock()
    }

    pub fn set_state(&self, state: ConnectionState) {
        *self.state.lock() = state;
    }

    pub fn is_connected(&self) -> bool {
        *self.state.lock() == ConnectionState::Connected
    }

    /// Bug №267 (A): client asks for an immediate resync of one layer.
    /// Out-of-range layers are ignored rather than trusted from the wire.
    pub fn request_resync(&self, layer: u8) {
        if (layer as usize) < LAYER_COUNT {
            self.resync_requests.lock()[layer as usize] = true;
        }
    }

    /// Consumes a pending resync request: `true` only once, so one request
    /// drives one resync pass and cannot keep re-arming itself.
    pub fn take_resync_request(&self, layer: usize) -> bool {
        if layer >= LAYER_COUNT {
            return false;
        }
        std::mem::replace(&mut self.resync_requests.lock()[layer], false)
    }

    /// Bug №30 (B): record a LayerSync report — the ticks the client says it
    /// has applied per layer. The merge is `max` per layer: the report is an
    /// absolute tick, so a duplicate or reordered datagram can only be
    /// stale, never a rewind. (A layer whose report reads 0 has simply never
    /// applied anything; an OLDER value than the stored one is ignored, not
    /// zeroed.)
    pub fn report_applied(&self, applied: [u32; LAYER_COUNT]) {
        let mut reported = self.reported_applied.lock();
        for (slot, value) in reported.iter_mut().zip(applied) {
            // u32 on the wire, u64 in bookkeeping: every server tick ever
            // serialized is u32, so the cast is lossless by dataflow.
            let value = value as u64;
            if value > *slot {
                *slot = value;
            }
        }
        // Bug №742: the gauge is live from the first report, even an
        // all-zero one — "the client applied tick 0 of every layer" is a
        // measurement, not silence.
        self.report_seen.store(true, Ordering::Relaxed);
    }

    /// Bug №30 (B): the last applied tick the client reported for `layer`.
    /// Zero means the client has never reported this layer (or never
    /// applied it) — the snapshot pass must not treat that as a
    /// confirmation of anything.
    pub fn reported_applied(&self, layer: usize) -> u64 {
        if layer >= LAYER_COUNT {
            return 0;
        }
        self.reported_applied.lock()[layer]
    }

    /// Bug №742: the applied-lag gauge — how far behind the server's current
    /// tick the client's last REPORTED applied ticks sit, per layer. This is
    /// the end-to-end freshness the heartbeat RTT never carried: it totals
    /// the wire both ways plus the client's own apply pipeline, sampled where
    /// the actually-applied state intersects the server's clock — in TICKS,
    /// with no timestamps of ours to drift against. None until the first
    /// report lands (see `report_seen`); per-layer 0 means "at the server's
    /// tick exactly", and a value greater than the tick itself is impossible
    /// by `saturating_sub` (an ahead-of-clock report reads 0, not a negative
    /// lag — it cannot happen through the dataflow anyway: a client applies
    /// ticks this server sent).
    pub fn applied_lag(&self, server_tick: u64) -> Option<[u64; LAYER_COUNT]> {
        if !self.report_seen.load(Ordering::Relaxed) {
            return None;
        }
        let reported = self.reported_applied.lock();
        Some(std::array::from_fn(|i| {
            server_tick.saturating_sub(reported[i])
        }))
    }

    pub fn client_id(&self) -> Option<u32> {
        *self.client_id.lock()
    }

    pub fn set_client_id(&self, client_id: Option<u32>) {
        *self.client_id.lock() = client_id;
    }

    pub fn player_entity(&self) -> Option<EntityId> {
        *self.player_entity.lock()
    }

    pub fn set_player_entity(&self, player_entity: Option<EntityId>) {
        *self.player_entity.lock() = player_entity;
    }

    pub fn ship_entity(&self) -> Option<EntityId> {
        *self.ship_entity.lock()
    }

    pub fn set_ship_entity(&self, ship_entity: Option<EntityId>) {
        *self.ship_entity.lock() = ship_entity;
    }

    pub fn interest_entities(&self) -> Vec<EntityId> {
        self.interest_entities.lock().clone()
    }

    pub fn set_interest_entities(&self, entities: Vec<EntityId>) {
        *self.interest_entities.lock() = entities;
    }

    pub fn next_sequence(&self) -> u32 {
        let mut seq = self.local_sequence.lock();
        *seq = seq.wrapping_add(1);
        *seq
    }

    /// Bug №77: feed one inbound header into the peer tracking so outbound
    /// packets can carry a real `ack`/`ack_bitfield` and the far side can GC
    /// its reliable queue. The client does not route every packet through
    /// `handle_packet`, so this is the light-weight equivalent of its header
    /// bookkeeping block.
    pub fn record_remote(&self, sequence: u32, ack: u32, ack_bitfield: u32) {
        {
            let mut remote_sequence = self.remote_sequence.lock();
            if seq_is_newer(sequence, *remote_sequence) {
                *remote_sequence = sequence;
            }
        }
        // Only accept ack state that moves forward (reorder-safe); stale
        // ack+bitfield pairs would corrupt pending_acks GC.
        {
            let mut remote_ack = self.remote_ack.lock();
            if seq_is_newer(ack, *remote_ack) {
                *remote_ack = ack;
                *self.remote_ack_bitfield.lock() = ack_bitfield;
            }
        }
        // Bug №697: the ack state must also reach OUR send ledgers — the
        // client does not route datagrams through `handle_packet`, so this
        // light-weight path is the only place it could happen. Before the
        // feed, every reliable packet the client ever sent was retransmitted
        // until the retry budget retired it (11 copies of each heartbeat),
        // and each duplicate's ack then fed the RTT EWMA an ever-growing
        // sample — the fabricated ~300 ms "round trip".
        self.process_acks();
        self.note_send_ack();
    }

    pub fn handle_packet(
        &self,
        header: PacketHeader,
        payload: &[u8],
    ) -> Result<PacketOutcome, ConnectionError> {
        *self.last_received.lock() = Instant::now();
        self.bandwidth.record_received(payload.len());

        // Bug №167: feed the reliable-sequence tracker before building any ack
        // we may send, so the mark reflects this very packet.
        // Bug №215: the verdict doubles as the caller's duplicate gate.
        let reliable_arrival =
            self.track_received_reliable(header.sequence, channel_from_u8(header.channel));

        {
            let mut remote_sequence = self.remote_sequence.lock();
            if seq_is_newer(header.sequence, *remote_sequence) {
                *remote_sequence = header.sequence;
            }
        }

        {
            let mut remote_ack = self.remote_ack.lock();
            if seq_is_newer(header.ack, *remote_ack) {
                *remote_ack = header.ack;
                *self.remote_ack_bitfield.lock() = header.ack_bitfield;
            }
        }
        self.process_acks();

        // Bug №215: the peer's ack state also clears OUR send ledgers — the
        // consumer №167 promised and never had.
        self.note_send_ack();

        let outgoing = match PacketType::from_u8(header.packet_type) {
            Some(PacketType::Heartbeat) => self.send_heartbeat_ack(header.sequence)?,
            Some(PacketType::HeartbeatAck) => {
                // Bug №12: RTT is measured against the last heartbeat WE
                // sent (stamped in send_heartbeat), not against "now".
                let rtt = self.last_heartbeat.lock().elapsed();
                let mut current = self.rtt.lock();
                *current = smooth_rtt(*current, rtt);
                Vec::new()
            }
            Some(PacketType::Input) => Vec::new(),
            Some(PacketType::Fragment) => {
                // Reassembled by the caller via `reassemble_fragment`.
                Vec::new()
            }
            Some(PacketType::Command) => Vec::new(),
            Some(PacketType::EventAck) => Vec::new(),
            Some(PacketType::Rpc) => Vec::new(),
            Some(PacketType::LayerSync) => {
                // Bug №753: the applied-layer report is normal, server-handled
                // traffic — the №216 session gate has already checked its echo
                // and `NetServer` applies the report in its own dispatch
                // (№30 (B)). Like Input/EventAck above, the session level owes
                // it no reply and no noise. The old catch-all shouted "Unhandled
                // packet type" per report: ~10 reports a second per connected
                // client (№30 (B) reports every ~100 ms), so a single second of
                // the shutdown flood carried ~1000 warn lines — a storm that
                // teaches the log reader to ignore WARN entirely (the same
                // lesson №272 already fixed for Disconnect on the normal exit
                // path).
                Vec::new()
            }
            Some(PacketType::ResyncRequest) => {
                // Bug №754: the same lesson the same surface taught twice.
                // A resync request is honest client traffic applied by
                // NetServer's own dispatch (№267 (A): apply_resync_request →
                // request_resync arms the next snapshot pass) — like the
                // LayerSync report above, the session level owes it no reply
                // and no per-request warn. The old catch-all produced a WARN
                // for every legitimate request: 5 per 3-wave stress run with
                // two sunk ships, more whenever layers actually drift.
                Vec::new()
            }
            Some(PacketType::Disconnect) => {
                // Bug №272: handled one level up, in
                // `NetServer::handle_received_packet`, which removes the
                // connection. It used to fall into `Some(_)` and log
                // "Unhandled packet type" for every client that left cleanly —
                // a warning that fires on the normal shutdown path trains the
                // reader to ignore the log.
                Vec::new()
            }
            Some(PacketType::Connect) => {
                // Same: `NetServer` performs the handshake and the version
                // check. Also on the normal path, once per client.
                Vec::new()
            }
            Some(other) => {
                warn!("Unhandled packet type in handle_packet: {other:?}");
                Vec::new()
            }
            None => {
                warn!("Invalid packet type: {:?}", header.packet_type);
                Vec::new()
            }
        };

        Ok(PacketOutcome {
            outgoing,
            reliable_arrival,
        })
    }

    /// Bug №215: feed the peer's (cumulative, SACK) ack state into the send
    /// ledgers. One truth rule: this is exactly the `(ack, bitfield)` pair
    /// `process_acks` uses for its own queue, and the bit layout is pinned
    /// by `bit_layout_agrees_with_the_receiver_across_the_whole_window`.
    fn note_send_ack(&self) {
        let ack = *self.remote_ack.lock();
        if ack == 0 {
            return; // "nothing ever acked" clears nothing; keep it cheap.
        }
        let ack_bitfield = *self.remote_ack_bitfield.lock();

        for channel in &self.channels {
            if matches!(
                channel.config().channel_type,
                ChannelType::ReliableOrdered | ChannelType::ReliableUnordered
            ) {
                channel.handle_ack(ack, ack_bitfield);
            }
        }
    }

    fn process_acks(&self) {
        let remote_ack = *self.remote_ack.lock();
        let remote_ack_bitfield = *self.remote_ack_bitfield.lock();

        // Bug №13: the old loop stopped at the first unacked entry, so one
        // lost packet pinned the whole queue (head-of-line blocking). SACK
        // bitfield entries clear individually now; the queue is also capped
        // (bug №14), so this scan stays cheap.
        self.pending_acks.lock().retain(|seq| {
            if *seq == remote_ack || !seq_is_newer(*seq, remote_ack) {
                return false; // cumulatively acked
            }
            let offset = seq.wrapping_sub(remote_ack) & 31;
            (remote_ack_bitfield & (1 << offset)) == 0
        });
    }

    /// Record a reliable sequence number, keeping the queue within
    /// `reliable_window` (bug §14). Dropped entries are unrecoverable anyway
    /// (no retransmit layer exists yet) — this is pure GC.
    ///
    /// Bug §272: this used to `warn!` on every dropped entry. At 200 bots the
    /// tick loop emitted 20127 of those lines in a 30 s run, and the log
    /// writing was itself a measurable share of the tick budget. The count is
    /// now exact and cheap; `log_pending_ack_summary` reports it in bulk.
    fn track_reliable(&self, seq: u32) {
        let mut pending = self.pending_acks.lock();
        pending.push_back(seq);
        let cap = self.config.reliable_window.max(1) as usize;
        while pending.len() > cap {
            if let Some(dropped) = pending.pop_front() {
                self.pending_ack_drops.fetch_add(1, Ordering::Relaxed);
                let mut first = self.first_pending_ack_drop.lock();
                if first.is_none() {
                    *first = Some(dropped);
                }
            }
        }
    }

    /// Bug №272: drops recorded since `since`, for callers that keep their own
    /// high-water mark and do the reporting in bulk. Does not log — the
    /// per-occurrence log this replaced was itself costing more than the work
    /// it described (see `track_reliable`).
    pub fn pending_ack_drops_since(&self, since: u64) -> u64 {
        self.pending_ack_drops
            .load(Ordering::Relaxed)
            .saturating_sub(since)
    }

    /// Bug №272: the first sequence that aged out, for a summary that says
    /// which window overflowed rather than only how often.
    pub fn first_pending_ack_drop(&self) -> Option<u32> {
        *self.first_pending_ack_drop.lock()
    }

    /// Bug №167: build an outbound header that advertises OUR real receive
    /// state — the contiguous reliable mark plus the SACK bits — instead of
    /// echoing back whatever sequence the peer happened to send last.
    fn ack_header(
        &self,
        packet_type: PacketType,
        channel: ChannelType,
        sequence: u32,
        payload_size: u16,
    ) -> PacketHeader {
        let (ack, ack_bitfield) = self.ack_info();
        PacketHeader::new(
            packet_type,
            channel,
            sequence,
            ack,
            ack_bitfield,
            payload_size,
        )
    }

    pub fn send_heartbeat(&self) -> Option<OutgoingPacket> {
        let seq = self.next_sequence();
        *self.last_heartbeat.lock() = Instant::now();
        let packet = HeartbeatPacket {
            client_time: 0.0,
            server_time: 0.0,
            session: self.session(),
        };
        let header = self.ack_header(PacketType::Heartbeat, ChannelType::ReliableOrdered, seq, 0);
        // OutgoingPacket::new refuses on serialize failure, so nothing but a
        // real datagram enters either bookkeeping.
        let outgoing = OutgoingPacket::new(header, packet)?;
        self.track_reliable(seq);
        // Bug №215: a dropped heartbeat never came back before; now the
        // ledger re-sends it until the ack (or the budget) decides.
        self.register_reliable_send(&outgoing.header, &outgoing.payload);
        Some(outgoing)
    }

    pub fn send_heartbeat_ack(&self, ack_seq: u32) -> Result<Vec<OutgoingPacket>, ConnectionError> {
        let seq = self.next_sequence();
        let packet = HeartbeatPacket {
            client_time: 0.0,
            server_time: 0.0,
            session: self.session(),
        };
        // The `ack` field of a HeartbeatAck echoes the heartbeat it answers
        // (RTT correlation), so it stays the echoed sequence; the SACK half
        // comes from the real receive state.
        let (_, ack_bitfield) = self.ack_info();
        let header = PacketHeader::new(
            PacketType::HeartbeatAck,
            ChannelType::ReliableOrdered,
            seq,
            ack_seq,
            ack_bitfield,
            0,
        );
        Ok(OutgoingPacket::new(header, packet).into_iter().collect())
    }

    pub fn send_connect_accept(
        &self,
        server_tick: u64,
        server_time: f64,
        match_id: Uuid,
    ) -> Option<OutgoingPacket> {
        let seq = self.next_sequence();
        let packet = ConnectAcceptPacket {
            server_tick,
            server_time,
            assigned_client_id: self.id,
            match_id,
            tick_rate: TICK_RATE,
            // Bug №216: this is where the client first learns the session it
            // must echo from now on.
            session: self.session(),
        };
        let header = self.ack_header(
            PacketType::ConnectAccept,
            ChannelType::ReliableOrdered,
            seq,
            0,
        );
        let outgoing = OutgoingPacket::new(header, packet)?;
        self.track_reliable(seq);
        // Bug №215: a lost ConnectAccept used to hang the client until it
        // reconnected from scratch; the ledger restores the accept itself.
        self.register_reliable_send(&outgoing.header, &outgoing.payload);
        Some(outgoing)
    }

    pub fn send_state(&self, state: StatePacket) -> Vec<OutgoingPacket> {
        let mut packets = Vec::new();
        // Bug №8: a masked serialize error used to go out as an empty
        // State that the peer drops in deserialize. Skip loudly instead.
        // Bug №732: postcard; the error stays a bincode::Error (Custom).
        let serialized = match rfs_core::packet::encode(&state) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("send_state serialize failed, skipping: {e}");
                return packets;
            }
        };

        if serialized.len() <= max_payload_len(&self.config) {
            let seq = self.next_sequence();
            let header =
                self.ack_header(PacketType::State, ChannelType::UnreliableSequenced, seq, 0);
            // Reuse already-serialized bytes (no second serialize + clone).
            packets.extend(OutgoingPacket::raw(header, serialized));
        } else {
            packets.extend(self.fragment_large_packet(PacketType::State, serialized));
        }
        packets
    }

    pub fn send_state_delta(&self, delta: StateDeltaPacket) -> Vec<OutgoingPacket> {
        match self.serialize_state_delta(delta) {
            Some(bytes) => self.send_state_delta_bytes(bytes),
            None => Vec::new(),
        }
    }

    pub fn serialize_state_delta(&self, delta: StateDeltaPacket) -> Option<Vec<u8>> {
        match rfs_core::packet::encode(&delta) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                warn!("send_state_delta serialize failed, skipping: {e}");
                None
            }
        }
    }

    /// Like `send_state_delta` but takes already-serialized payload bytes. The
    /// payload is the expensive part; at 100-200 clients the same layer delta
    /// is byte-identical for every client that shares a base tick, so the
    /// server serializes it once per round and reuses the bytes (the per-client
    /// sequence header is still fresh here, which is why the cache lives at the
    /// payload level and never below).
    pub fn send_state_delta_bytes(&self, serialized: Vec<u8>) -> Vec<OutgoingPacket> {
        let mut packets = Vec::new();

        if serialized.len() <= max_payload_len(&self.config) {
            let seq = self.next_sequence();
            let header = self.ack_header(
                PacketType::StateDelta,
                ChannelType::UnreliableSequenced,
                seq,
                0,
            );
            packets.extend(OutgoingPacket::raw(header, serialized));
        } else {
            packets.extend(self.fragment_large_packet(PacketType::StateDelta, serialized));
        }
        packets
    }

    pub fn send_event(&self, event: EventPacket) -> Vec<OutgoingPacket> {
        // Bug №28: large events are fragmentable just like State/Delta —
        // previously they overflowed the MTU guard and were dropped by the
        // `let _` at the call site.
        let serialized = match rfs_core::packet::encode(&event) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("send_event serialize failed, skipping: {e}");
                return Vec::new();
            }
        };

        if serialized.len() <= max_payload_len(&self.config) {
            let seq = self.next_sequence();
            // Bug №272: events go out UNRELIABLE, and enter no ledger.
            //
            // Making them reliable was possible once №215 wired the
            // retransmit layer — it would have bought nothing: events are
            // notifications, not state. The client puts them on an event
            // queue and keeps its world state in the layer deltas
            // (`entity_states`), where every event's content also arrives;
            // re-deliveries would just fire the notification twice. At 200
            // bots `broadcast_event` fans every event out to every client, so
            // a ledger entry per event would have churned continuously.
            //
            // The one field that was NOT covered by any layer — the hit
            // normal and compartment — is now carried in layer 0, so nothing
            // gameplay-relevant is lost.
            //
            // Per plan §3.4, guaranteed delivery is required for COMMANDS
            // ("seal the bulkhead"), not for events. `CommandAck` and
            // `Connect` keep `ReliableOrdered` and keep tracking.
            let header = self.ack_header(
                PacketType::Event,
                ChannelType::UnreliableSequenced,
                seq,
                serialized.len() as u16,
            );
            OutgoingPacket::raw(header, serialized)
                .into_iter()
                .collect()
        } else {
            self.fragment_large_packet(PacketType::Event, serialized)
        }
    }

    /// Bug №698: the world's static description, sent once when the client
    /// connects.
    ///
    /// It goes out UNRELIABLE, like the events: the world does not change, and
    /// a client that missed it can be told again (the server re-sends on the
    /// next Connect). Making it reliable would put the whole map into the
    /// retransmit ledger of every client, for a message that is sent once.
    pub fn send_map_description(
        &self,
        description: &rfs_core::world::MapDescription,
    ) -> Vec<OutgoingPacket> {
        let packet = MapDescriptionPacket {
            name: description.name.clone(),
            bounds_min: description.bounds_min,
            bounds_max: description.bounds_max,
            obstacles: description.obstacles.clone(),
        };
        let serialized = match rfs_core::packet::encode(&packet) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("send_map_description serialize failed, skipping: {e}");
                return Vec::new();
            }
        };
        if serialized.len() <= max_payload_len(&self.config) {
            let seq = self.next_sequence();
            let header = self.ack_header(
                PacketType::MapDescription,
                ChannelType::UnreliableSequenced,
                seq,
                serialized.len() as u16,
            );
            OutgoingPacket::raw(header, serialized)
                .into_iter()
                .collect()
        } else {
            // A map with thousands of structures does not fit a datagram; the
            // fragmentation path carries it, exactly as it carries a full
            // state.
            self.fragment_large_packet(PacketType::MapDescription, serialized)
        }
    }

    pub fn send_command_ack(
        &self,
        command_id: u64,
        success: bool,
        error: Option<String>,
    ) -> Vec<OutgoingPacket> {
        let packet = CommandAckPacket {
            command_id,
            success,
            error,
        };
        let serialized = match rfs_core::packet::encode(&packet) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("send_command_ack serialize failed, skipping: {e}");
                return Vec::new();
            }
        };

        if serialized.len() <= max_payload_len(&self.config) {
            let seq = self.next_sequence();
            let header = self.ack_header(
                PacketType::CommandAck,
                ChannelType::ReliableOrdered,
                seq,
                serialized.len() as u16,
            );
            let packet = match OutgoingPacket::raw(header, serialized) {
                Some(packet) => packet,
                None => return Vec::new(),
            };
            self.track_reliable(seq);
            // Bug №215: the command result is the "seal the bulkhead"
            // guarantee from plan §3.4 — the client may re-fire the command if
            // it never learns the outcome, which is exactly why a lost ack
            // goes back through the ledger instead of vanishing.
            self.register_reliable_send(&packet.header, &packet.payload);
            vec![packet]
        } else {
            self.fragment_large_packet(PacketType::CommandAck, serialized)
        }
    }

    pub fn send_input_ack(&self, ack: InputAckPacket) -> Vec<OutgoingPacket> {
        let serialized = match rfs_core::packet::encode(&ack) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("send_input_ack serialize failed, skipping: {e}");
                return Vec::new();
            }
        };

        if serialized.len() <= max_payload_len(&self.config) {
            let seq = self.next_sequence();
            // Bug №272: unreliable, and NOT tracked — it was the one send path
            // that declared `ReliableOrdered` without calling
            // `track_reliable`, so the channel promised a guarantee the
            // bookkeeping never recorded. That is the worst of both: no
            // delivery guarantee AND a sequence that can never be reclaimed.
            //
            // Losing one ack is harmless by construction, which is what keeps
            // it unreliable even now that retransmit exists (№215): the
            // client's `pending_inputs` is capped (bug №171) and evicts the
            // oldest, so a lost ack delays one slot's release for up to the
            // cap rather than leaking. `last_acknowledged_tick` is only ever
            // moved forward, so a lost ack also delays nothing — the next one
            // carries the same information. Mirroring it into the ledger
            // would instead resend ~6000 packets/s of pure staleness at
            // 30 acks/s and 200 clients.
            let header = self.ack_header(
                PacketType::InputAck,
                ChannelType::UnreliableSequenced,
                seq,
                serialized.len() as u16,
            );
            OutgoingPacket::raw(header, serialized)
                .into_iter()
                .collect()
        } else {
            self.fragment_large_packet(PacketType::InputAck, serialized)
        }
    }

    fn fragment_large_packet(&self, packet_type: PacketType, data: Vec<u8>) -> Vec<OutgoingPacket> {
        let mut packets = Vec::new();
        let max_payload = max_payload_len(&self.config).saturating_sub(FRAGMENT_HEADER_SIZE);
        if max_payload == 0 {
            warn!("max_packet_size leaves no room for fragments, dropping");
            return packets;
        }
        let msg_id = {
            let mut id = self.next_fragment_id.lock();
            *id = id.wrapping_add(1);
            if *id == 0 {
                *id = 1;
            }
            *id
        };

        for payload in make_fragments(
            packet_type,
            &data,
            msg_id,
            max_payload + FRAGMENT_HEADER_SIZE,
        ) {
            let seq = self.next_sequence();
            // Bug №163: fragments used to always ride UnreliableSequenced, so a
            // reliable message above the MTU (a CommandAck) lost its channel
            // semantics the moment it needed fragmenting. Keep the reliable
            // packet's channel, and register the reliable sequences so the
            // peer's queue can be garbage-collected.
            //
            // Bug №215: `Event` and `InputAck` left this list — №272 made
            // them unreliable on the small-packet path, and this mapping
            // quietly resurrected the oversized ones into the reliable
            // bucket. With the retransmit ledger live that is no longer a
            // bookkeeping nicety: every dropped chunk of every event would
            // be re-sent, exactly the churn №272 removed.
            let channel = match packet_type {
                PacketType::Command
                | PacketType::CommandAck
                | PacketType::Connect
                | PacketType::ConnectAccept => ChannelType::ReliableOrdered,
                _ => ChannelType::UnreliableSequenced,
            };
            let header = self.ack_header(PacketType::Fragment, channel, seq, payload.len() as u16);
            let outgoing = match OutgoingPacket::raw(header, payload) {
                Some(packet) => packet,
                None => continue,
            };
            if matches!(
                channel,
                ChannelType::ReliableOrdered | ChannelType::ReliableUnordered
            ) {
                self.track_reliable(seq);
                // Bug №215: №167(3) said fragment reordering "relies on the
                // resend" — now there is one. The ledger holds the fragment
                // payload; the assembler's msg_id dedup (fragment.rs) swallows
                // the repeats.
                self.register_reliable_send(&outgoing.header, &outgoing.payload);
            }
            packets.push(outgoing);
        }
        packets
    }

    /// Feed one `Fragment` payload into this connection's reassembler.
    /// Returns the original packet type plus the full message bytes once complete.
    pub fn reassemble_fragment(&self, payload: &[u8]) -> Option<(PacketType, Vec<u8>)> {
        self.fragment_assembler.lock().push(payload)
    }

    /// Bug №167: the whole channel layer was never wired in, so `ack_bitfield`
    /// stayed 0 forever and reliability degraded to a single cumulative ack
    /// built from the LAST sequence seen. One lost packet in the middle of the
    /// window therefore marked everything after it as delivered, and a
    /// reordered 5,6 looked acknowledged when 5 never arrived.
    ///
    /// This tracks received RELIABLE sequences properly: `reliable_hcr` is the
    /// highest contiguous one (so "≤ hcr" really means "delivered"), and
    /// out-of-order arrivals above it are remembered and reported as SACK bits.
    /// Unreliable traffic is deliberately ignored — it must not raise the
    /// cumulative mark, that was the original bug.
    ///
    /// Bug №215: the return value doubles as the duplicate-suppression gate.
    /// Once retransmits exist, a sequence the peer saw twice (our ack of the
    /// first copy was lost) would apply twice without this: the contiguous
    /// mark catches covered sequences, gap membership catches ones the mark
    /// has not advanced over yet.
    pub fn track_received_reliable(
        &self,
        sequence: u32,
        channel: ChannelType,
    ) -> Option<ReliableArrival> {
        if !matches!(
            channel,
            ChannelType::ReliableOrdered | ChannelType::ReliableUnordered
        ) {
            return None;
        }

        let mut hcr = self.reliable_hcr.lock();
        if !seq_is_newer(sequence, *hcr) {
            // Already covered cumulatively, or a duplicate: nothing to add,
            // and the caller must not apply the payload again.
            return Some(ReliableArrival::Duplicate);
        }

        let mut gaps = self.reliable_gaps.lock();
        if !gaps.insert(sequence) {
            // Already queued in a gap: a retransmit of something the peer is
            // still waiting to see acknowledged contiguous. Same rule applies.
            return Some(ReliableArrival::Duplicate);
        }

        // Advance the contiguous mark while the next sequence is present.
        while gaps.remove(&hcr.wrapping_add(1)) {
            *hcr = hcr.wrapping_add(1);
        }

        // Bound the memory. Forget the gap furthest from the contiguous mark —
        // in ring terms that is the OLDEST one still outstanding.
        while gaps.len() > SACK_TRACKED_GAPS {
            let hcr_snapshot = *hcr;
            let oldest = gaps
                .iter()
                .copied()
                .max_by_key(|s| s.wrapping_sub(hcr_snapshot));
            match oldest {
                Some(seq) => {
                    gaps.remove(&seq);
                }
                None => break,
            }
        }

        Some(ReliableArrival::New)
    }

    /// The `(ack, ack_bitfield)` pair to advertise in outbound headers.
    ///
    /// Bit layout mirrors the receiver exactly (`process_acks` and
    /// `Channel::handle_ack` both test `1 << (seq.wrapping_sub(ack) & 31)`):
    /// bit `n` describes the sequence `ack + n`. `rel == 0` is already covered
    /// by the cumulative mark, so reporting starts at 1; `rel == 32` aliases
    /// onto bit 0, which is safe because the receiver only tests bit 0 for
    /// that very sequence.
    pub fn ack_info(&self) -> (u32, u32) {
        let hcr = *self.reliable_hcr.lock();
        let gaps = self.reliable_gaps.lock();

        let mut bitfield = 0u32;
        for &seq in gaps.iter() {
            let rel = seq.wrapping_sub(hcr);
            if (1..=32).contains(&rel) {
                bitfield |= 1 << (rel & 31);
            }
        }
        (hcr, bitfield)
    }

    /// Reset the SACK state — a fresh session must not inherit the old mark.
    /// Bug №215: the send ledgers belong to this session just as much, so a
    /// reused connection must not keep re-sending the previous session's
    /// queue against new sequence space.
    pub fn reset_reliable_tracking(&self) {
        *self.reliable_hcr.lock() = 0;
        self.reliable_gaps.lock().clear();
        for channel in &self.channels {
            channel.clear_send_queue();
        }
    }

    /// Bug №215: mirror a reliable send into the Channel ledger. The payload
    /// bytes mirror what the wire helper serializes; retransmit passes of the
    /// same sequence just refresh the stamp. Unreliable channels are not
    /// mirrored — their loss is by design, and mirroring them would burn
    /// ledger space for nothing.
    pub fn register_reliable_send(&self, header: &PacketHeader, payload: &[u8]) {
        let channel_type = channel_from_u8(header.channel);
        if !matches!(
            channel_type,
            ChannelType::ReliableOrdered | ChannelType::ReliableUnordered
        ) {
            return;
        }
        let Some(channel) = self.get_channel(channel_type) else {
            return; // no ledger configured for this channel type
        };
        channel.register_sent(header.sequence, header.packet_type, payload);
    }

    /// Bug №215: retransmit pass over the reliable ledgers.
    ///
    /// The ORIGINAL sequence is preserved — the receiver's SACK accounting
    /// (№167) is keyed on it, so a retransmit slots into the same hole it
    /// fell out of — while the header carries OUR CURRENT receive state, so
    /// the retransmit doubles as fresh ack traffic for the peer's own queue
    /// GC. Timeout/retries are constants: the RTT is measured already (№12)
    /// but an adaptive timeout is a tuning knob, not a correctness gap.
    pub fn collect_retransmits(&self) -> Vec<OutgoingPacket> {
        self.collect_retransmits_with(
            Duration::from_millis(RETRANSMIT_TIMEOUT_MS),
            RETRANSMIT_MAX_RETRIES,
            RETRANSMIT_MAX_PER_PASS,
        )
    }

    /// Bug №215: the same pass with injectable timing. Exposed because the
    /// fixed cadence above is honest production behaviour but useless for
    /// tests — sleeping a 250 ms RTO in a unit test would be the test
    /// writing the poller, not the poller being tested.
    pub fn collect_retransmits_with(
        &self,
        timeout: Duration,
        max_retries: u32,
        max_count: usize,
    ) -> Vec<OutgoingPacket> {
        const MAX_PER_PASS: usize = RETRANSMIT_MAX_PER_PASS;

        let mut result = Vec::new();
        for channel in &self.channels {
            if result.len() >= MAX_PER_PASS {
                break;
            }
            if !matches!(
                channel.config().channel_type,
                ChannelType::ReliableOrdered | ChannelType::ReliableUnordered
            ) {
                continue;
            }
            for (sequence, packet_type, data) in channel.collect_retransmits(
                timeout,
                max_retries,
                max_count.saturating_sub(result.len()).max(1),
            ) {
                let Some(ptype) = PacketType::from_u8(packet_type) else {
                    continue;
                };
                let header = self.ack_header(
                    ptype,
                    channel.config().channel_type,
                    sequence,
                    data.len() as u16,
                );
                if let Some(packet) = OutgoingPacket::raw(header, data) {
                    result.push(packet);
                }
            }
        }
        result
    }

    #[cfg(test)]
    fn tracked_gap_count(&self) -> usize {
        self.reliable_gaps.lock().len()
    }

    pub fn is_timed_out(&self) -> bool {
        self.last_received.lock().elapsed() > self.config.connection_timeout
    }

    /// Bug №219: how long this connection has existed. Anchored at `new()`,
    /// not at the first datagram — the gate that waits for the player to be
    /// registered needs a clock the game layer cannot refresh.
    pub fn age_connected(&self) -> Duration {
        self.connected_since.elapsed()
    }

    #[cfg(test)]
    pub(crate) fn new_backdated(
        id: u32,
        addr: SocketAddr,
        config: ConnectionConfig,
        age: Duration,
    ) -> Self {
        let mut conn = Self::new(id, addr, config);
        conn.connected_since = Instant::now()
            .checked_sub(age)
            .unwrap_or(conn.connected_since);
        conn
    }

    pub fn should_send_heartbeat(&self) -> bool {
        self.last_heartbeat.lock().elapsed() >= self.config.heartbeat_interval
    }

    pub fn update_heartbeat(&self) {
        *self.last_heartbeat.lock() = Instant::now();
    }

    pub fn get_channel(&self, channel_type: ChannelType) -> Option<&Channel> {
        self.channels
            .iter()
            .find(|c| c.config().channel_type == channel_type)
    }

    /// Bug №176: this connection's own send budget, replacing the single
    /// server-wide limiter.
    pub fn bandwidth_limiter(&self) -> &BandwidthLimiter {
        &self.bandwidth_limiter
    }
}

#[derive(Debug, Clone)]
pub struct OutgoingPacket {
    pub header: PacketHeader,
    pub payload: Vec<u8>,
}

impl OutgoingPacket {
    /// Bug №8/№9: serialization failure or an over-u16 payload used to be
    /// masked (`unwrap_or_default` + `as u16`) into a corrupt packet the
    /// peer drops in deserialize. Now it refuses loudly instead.
    pub fn new(header: PacketHeader, packet: impl Serializable) -> Option<Self> {
        let payload = match rfs_core::packet::encode(&packet) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("OutgoingPacket serialize failed, dropping: {e}");
                return None;
            }
        };
        Self::raw(header, payload)
    }

    pub fn raw(mut header: PacketHeader, payload: Vec<u8>) -> Option<Self> {
        let len = match u16::try_from(payload.len()) {
            Ok(len) => len,
            Err(_) => {
                warn!(
                    "OutgoingPacket payload {} exceeds u16, dropping",
                    payload.len()
                );
                return None;
            }
        };
        header.payload_size = len;
        Some(Self { header, payload })
    }

    pub fn total_size(&self) -> usize {
        HEADER_SIZE + self.payload.len()
    }
}

#[cfg(test)]
mod wire_budget_tests {
    /// Bug №239: the net side of the MTU must be the core constant itself —
    /// an alias, not a restated number. A change of the core budget that the
    /// net side failed to follow would silently split the truth about the
    /// wire again, which is what this guards against by construction.
    ///
    /// №239 addendum: the deserialize cap is under the same rule —
    /// the two framing files alias the one statement in this file, and the
    /// value stays what the builders fragment against. (Bug №732 turned the
    /// guard into an explicit length check; the number is the one here still.)
    #[test]
    fn the_wire_budget_is_not_a_second_number() {
        assert_eq!(
            super::MAX_PACKET_SIZE,
            rfs_core::packet::MAX_PACKET_SIZE,
            "connection::MAX_PACKET_SIZE must alias the core constant"
        );
        assert_eq!(
            super::WIRE_LIMIT,
            crate::server::WIRE_LIMIT,
            "server::WIRE_LIMIT must alias connection::WIRE_LIMIT"
        );
        assert_eq!(
            super::WIRE_LIMIT,
            crate::client::WIRE_LIMIT,
            "client::WIRE_LIMIT must alias connection::WIRE_LIMIT"
        );
        assert_eq!(super::WIRE_LIMIT, 1024 * 1024, "the wire budget stays the number the framings fragment against");
    }
}

#[cfg(test)]
mod resync_request_tests {
    //! Bug №267 (A): the per-connection resync request bits.
    //!
    //! A request must survive until the snapshot pass consumes it, must be
    //! consumed exactly once (a request that re-arms itself would resync the
    //! layer forever), and must not be settable for a layer that does not
    //! exist — the index arrives from the wire.

    use super::*;

    fn test_connection() -> Connection {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        Connection::new(1, addr, ConnectionConfig::default())
    }

    #[test]
    fn a_request_is_taken_once_and_only_for_its_layer() {
        let conn = test_connection();
        conn.request_resync(1);
        assert!(conn.take_resync_request(1), "the request must be visible");
        assert!(
            !conn.take_resync_request(1),
            "one request must drive exactly one resync pass"
        );
        assert!(
            !conn.take_resync_request(0),
            "another layer must not inherit the request"
        );
    }

    #[test]
    fn a_request_for_a_layer_outside_the_range_is_ignored() {
        let conn = test_connection();
        conn.request_resync(LAYER_COUNT as u8);
        conn.request_resync(200);
        for layer in 0..LAYER_COUNT {
            assert!(
                !conn.take_resync_request(layer),
                "a request from the wire must not reach a real layer"
            );
        }
    }

    #[test]
    fn a_request_out_of_range_in_the_take_path_is_refused() {
        let conn = test_connection();
        assert!(!conn.take_resync_request(LAYER_COUNT));
        assert!(!conn.take_resync_request(usize::MAX));
    }
}

#[cfg(test)]
mod sack_tests {
    use super::*;
    use crate::bandwidth::Admission;

    fn test_connection() -> Connection {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        Connection::new(1, addr, ConnectionConfig::default())
    }

    /// Exact copy of the receiver's predicate, so the test checks the real
    /// contract rather than a re-statement of it.
    fn is_seq_acked(seq: u32, ack: u32, bitfield: u32) -> bool {
        if seq == ack || !seq_is_newer(seq, ack) {
            return true;
        }
        (bitfield & (1 << (seq.wrapping_sub(ack) & 31))) != 0
    }

    #[test]
    fn in_order_reliable_advances_the_contiguous_mark() {
        let conn = test_connection();
        for seq in 1..=5 {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        let (ack, bits) = conn.ack_info();
        assert_eq!(ack, 5, "contiguous run must advance the cumulative mark");
        assert_eq!(bits, 0, "nothing outstanding above the mark");
    }

    #[test]
    fn unreliable_traffic_never_advances_the_mark() {
        // Bug №167: this is the original defect — an unreliable packet bumped
        // the value peers treated as "everything up to here arrived".
        let conn = test_connection();
        conn.track_received_reliable(9, ChannelType::UnreliableSequenced);
        let (ack, _) = conn.ack_info();
        assert_eq!(ack, 0, "unreliable packets must not be acknowledged");

        // A reliable packet that arrives first cannot advance the mark either:
        // 1..=3 were never seen, so claiming them would be a lie. It is
        // reported as a gap instead.
        conn.track_received_reliable(4, ChannelType::ReliableOrdered);
        let (ack, bits) = conn.ack_info();
        assert_eq!(ack, 0);
        assert!(
            is_seq_acked(4, ack, bits),
            "the one reliable packet is still reported"
        );
        assert!(
            !is_seq_acked(1, ack, bits),
            "a packet never seen is not acknowledged"
        );
    }

    #[test]
    fn gap_is_reported_as_sack_bit_and_does_not_pin_the_queue() {
        let conn = test_connection();
        // 1,2 received, 3 lost, 4..7 received.
        for seq in [1u32, 2, 4, 5, 6, 7] {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        let (ack, bits) = conn.ack_info();
        assert_eq!(ack, 2, "mark stops at the gap");
        assert_ne!(bits, 0, "the received-above-gap sequences must be reported");

        // The whole point: everything after the lost packet is acked even
        // though the cumulative mark is stuck behind it.
        let mut still_pending: Vec<u32> = (1u32..=7)
            .filter(|seq| !is_seq_acked(*seq, ack, bits))
            .collect();
        still_pending.sort_unstable();
        assert_eq!(
            still_pending,
            vec![3],
            "only the genuinely lost sequence stays"
        );
    }

    #[test]
    fn gap_filling_advances_the_mark_and_clears_the_bit() {
        let conn = test_connection();
        for seq in [1u32, 2, 4] {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        assert_eq!(conn.ack_info().0, 2);

        // The straggler shows up: the mark must jump and the bit disappear.
        conn.track_received_reliable(3, ChannelType::ReliableOrdered);
        let (ack, bits) = conn.ack_info();
        assert_eq!(ack, 4);
        assert_eq!(bits, 0);
    }

    #[test]
    fn bit_layout_agrees_with_the_receiver_across_the_whole_window() {
        // Guards the off-by-one a hand-written bitfield invites: the sender's
        // mapping must equal the receiver's for every offset in the window.
        //
        // Offset 1 is deliberately absent: with a contiguous mark, a packet at
        // hcr+1 is absorbed into the mark the moment it arrives, so bit 1 can
        // never legitimately be set. `gap_filling_advances_the_mark_and_clears_
        // the_bit` covers that case.
        const BASE: u32 = 100;

        for offset in 2u32..=32 {
            let conn = test_connection();
            // Contiguous run up to BASE, then the missing BASE+1, then one
            // packet at BASE+offset.
            for seq in 1..=BASE {
                conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
            }
            conn.track_received_reliable(BASE + offset, ChannelType::ReliableOrdered);

            let (ack, bits) = conn.ack_info();
            assert_eq!(ack, BASE, "offset {offset}: mark must stall at the gap");
            assert!(
                is_seq_acked(BASE + offset, ack, bits),
                "offset {offset}: sender bit layout disagrees with the receiver"
            );
            assert!(
                !is_seq_acked(BASE + 1, ack, bits),
                "offset {offset}: the missing sequence must not be acknowledged"
            );
        }
    }

    #[test]
    fn the_next_sequence_is_absorbed_into_the_mark() {
        // The counterpart of the note in `bit_layout_agrees_...`: offset 1 can
        // never appear as a SACK bit, because receiving hcr+1 advances hcr.
        let conn = test_connection();
        for seq in 1..=10 {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        conn.track_received_reliable(12, ChannelType::ReliableOrdered);
        assert_eq!(conn.ack_info(), (10, 1 << 2));

        // Filling 11 collapses both the mark and the 12 bit.
        conn.track_received_reliable(11, ChannelType::ReliableOrdered);
        assert_eq!(conn.ack_info(), (12, 0));
    }

    #[test]
    fn duplicate_and_stale_packets_do_not_corrupt_the_mark() {
        let conn = test_connection();
        for seq in [1u32, 2, 3] {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        // Reordered retransmits of already-delivered sequences.
        for seq in [1u32, 2, 3, 2, 1] {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        let (ack, bits) = conn.ack_info();
        assert_eq!((ack, bits), (3, 0));
    }

    #[test]
    fn gaps_beyond_the_window_do_not_grow_without_bound() {
        let conn = test_connection();
        for seq in 2u32..5000 {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        let (ack, bits) = conn.ack_info();
        assert_eq!(ack, 0, "mark stays behind the never-delivered 1");
        assert!(
            conn.tracked_gap_count() <= SACK_TRACKED_GAPS,
            "an attacker sending a flood of sequences must not grow the set without bound"
        );
        // The most recent arrivals still land inside the window, so a real
        // peer keeps making progress.
        assert!(
            is_seq_acked(4999, ack, bits),
            "recent arrivals must still be acknowledged"
        );
    }

    #[test]
    fn reset_clears_the_mark() {
        let conn = test_connection();
        for seq in 1..=10 {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        conn.reset_reliable_tracking();
        assert_eq!(conn.ack_info(), (0, 0));
        assert_eq!(conn.tracked_gap_count(), 0);
    }

    #[test]
    fn reliable_tracking_survives_the_sequence_wrap() {
        let conn = test_connection();
        let start = u32::MAX - 2;
        for seq in start..=u32::MAX {
            conn.track_received_reliable(seq, ChannelType::ReliableOrdered);
        }
        // Wraps past MAX to 0 and 1 — still newer, so the mark must follow.
        conn.track_received_reliable(0, ChannelType::ReliableOrdered);
        conn.track_received_reliable(1, ChannelType::ReliableOrdered);
        let (ack, bits) = conn.ack_info();
        assert_eq!(
            ack, 1,
            "the contiguous mark must wrap with the sequence space"
        );
        assert_eq!(bits, 0);
    }

    #[test]
    fn the_lag_gauge_says_nothing_until_the_first_report() {
        // Bug №742: before any LayerSync report there is no measurement — a
        // fabricated lag of the whole server tick count would read as if the
        // client were infinitely behind from the moment it connected.
        let conn = test_connection();
        assert!(conn.applied_lag(41).is_none(), "no report yet, no gauge");
        // After the first report the gauge is live, measured against the
        // caller's current tick.
        conn.report_applied([10, 9, 8, 7]);
        assert_eq!(conn.applied_lag(41), Some([31, 32, 33, 34]));
        // A later tick reads a fresher lag — the gauge samples now, it is
        // not a stamp frozen at report time.
        assert_eq!(conn.applied_lag(45), Some([35, 36, 37, 38]));
    }

    #[test]
    fn an_all_zero_report_is_a_measurement_not_silence() {
        // Bug №742: applied tick 0 is real (a full state whose tick was 0);
        // a zero must not read as "never reported". NC for this ход removed
        // the flag and returned the array outright — this and the None test
        // above went red together.
        let conn = test_connection();
        conn.report_applied([0, 0, 0, 0]);
        assert_eq!(conn.applied_lag(40), Some([40; LAYER_COUNT]));
    }

    #[test]
    fn an_ahead_of_clock_report_reads_zero_lag_not_underflow() {
        // Bug №742: impossible through the dataflow, but a wrap-adjacent or
        // otherwise ahead report must saturate, not flip into u64 arithmetic
        // garbage.
        let conn = test_connection();
        conn.report_applied([100, 100, 100, 100]);
        assert_eq!(conn.applied_lag(90), Some([0; LAYER_COUNT]));
    }

    #[test]
    fn a_wildly_late_rtt_sample_is_clamped() {
        // Bug №176: unclamped, one late ack permanently inflated the average
        // used for interpolation and resync pacing.
        let start = Duration::from_millis(50);
        let late = smooth_rtt(start, Duration::from_secs(30));
        assert!(
            late <= RTT_MAX,
            "a 30 s sample must not survive smoothing, got {late:?}"
        );
        assert!(late > start, "but the average should still move upward");
    }

    #[test]
    fn a_zero_rtt_sample_is_floored() {
        let zero = smooth_rtt(Duration::from_millis(50), Duration::ZERO);
        assert!(
            zero >= RTT_MIN,
            "a zero sample must not drag the average to zero, got {zero:?}"
        );
    }

    #[test]
    fn rtt_smoothing_converges_on_a_steady_sample() {
        let mut current = Duration::from_millis(100);
        let sample = Duration::from_millis(40);
        for _ in 0..200 {
            current = smooth_rtt(current, sample);
        }
        let drift = current.abs_diff(sample);
        assert!(
            drift < Duration::from_millis(2),
            "EWMA must settle on a steady sample, got {current:?}"
        );
    }

    #[test]
    fn a_heartbeat_stamp_actually_moves() {
        // Bug №176, server side: `update_heartbeat` was never called there, so
        // the stamp sat at construction time and elapsed() grew forever.
        let conn = test_connection();
        let first = conn.last_heartbeat.lock();
        std::thread::sleep(Duration::from_millis(5));
        drop(first);
        conn.update_heartbeat();
        let elapsed = conn.last_heartbeat.lock().elapsed();
        assert!(
            elapsed < Duration::from_millis(5),
            "the send stamp must be refreshed, but it read {elapsed:?}"
        );
    }

    #[test]
    fn connections_do_not_share_a_send_budget() {
        // Bug №176: one server-wide limiter meant a single noisy client spent
        // everyone else's budget. Each connection must get its own bucket.
        let a = test_connection();
        let b = test_connection();

        // Drain A's budget completely.
        let a_limit = a.config.max_bandwidth_bps as usize;
        assert!(a.bandwidth_limiter().try_consume(a_limit));

        assert_eq!(
            a.bandwidth_limiter().admit(1_000, false),
            Admission::Drop,
            "A is out of budget"
        );
        assert_eq!(
            b.bandwidth_limiter().admit(1_000, false),
            Admission::Send,
            "B must be unaffected by A's traffic"
        );
    }

    // ---------- Bug №215: retransmit ledger ----------

    #[test]
    fn a_lost_command_retransmits_with_the_original_sequence() {
        let conn = test_connection();
        let sent = conn.send_command_ack(7, true, None);
        assert_eq!(sent.len(), 1, "a CommandAck must be built");
        let seq = sent[0].header.sequence;

        let ledger = conn.get_channel(ChannelType::ReliableOrdered).unwrap();
        assert!(ledger.contains(seq), "the send must reach the ledger");

        // Zero timeout = "the timeout already elapsed". The retransmit keeps
        // the ORIGINAL sequence (receiver SACK accounting is keyed on it) and
        // re-wraps the SAME payload bytes.
        let retransmits = conn.collect_retransmits_with(Duration::ZERO, 10, 16);
        assert_eq!(retransmits.len(), 1);
        assert_eq!(retransmits[0].header.sequence, seq);
        assert_eq!(
            retransmits[0].header.packet_type,
            sent[0].header.packet_type
        );
        assert!(retransmits[0].header.ack <= seq, "fresh ack state");
        assert_eq!(retransmits[0].payload, sent[0].payload);

        // The ledger still holds it: the peer has not confirmed anything yet.
        assert!(ledger.contains(seq));
    }

    #[test]
    fn the_ledger_is_cleared_by_the_peers_ack_header() {
        let conn = test_connection();
        let sent = conn.send_command_ack(7, true, None);
        let seq = sent[0].header.sequence;

        // The peer's next datagram acknowledges our sequence. Its own channel
        // is unreliable, so the arrival itself must not touch our mark.
        let header = PacketHeader::new(
            PacketType::Heartbeat,
            ChannelType::UnreliableSequenced,
            991,
            seq,
            0,
            0,
        );
        conn.handle_packet(header, &[0u8; 4]).unwrap();

        let ledger = conn.get_channel(ChannelType::ReliableOrdered).unwrap();
        assert!(
            !ledger.contains(seq),
            "an ack header must retire the ledger entry"
        );
        assert!(conn
            .collect_retransmits_with(Duration::ZERO, 10, 16)
            .is_empty());
    }

    #[test]
    fn the_lightweight_record_remote_also_feeds_the_send_ledgers() {
        // Bug №697: `record_remote` is the client's stand-in for
        // `handle_packet`'s header bookkeeping, but it retired only the ack
        // mark and never fed it to the send ledgers. The client then kept
        // retransmitting every reliable packet until the retry budget gave
        // up (11 copies of each heartbeat), and every duplicate's ack went
        // into the RTT EWMA as an ever-growing sample — the fabricated
        // ~300 ms "round trip" of the stress runs.
        let conn = test_connection();
        let sent = conn.send_command_ack(7, true, None);
        let seq = sent[0].header.sequence;

        // The client's inbound path in miniature: header bookkeeping WITHOUT
        // dispatching the payload (how the receive loop treats snapshots and
        // heartbeats it answers elsewhere). The peer's header acks `seq`.
        conn.record_remote(991, seq, 0);

        let ledger = conn.get_channel(ChannelType::ReliableOrdered).unwrap();
        assert!(
            !ledger.contains(seq),
            "the light-weight bookkeeping must retire ledger entries too"
        );
        assert!(conn
            .collect_retransmits_with(Duration::ZERO, 10, 16)
            .is_empty());
    }

    #[test]
    fn a_duplicate_reliable_arrival_is_suppressed() {
        let conn = test_connection();
        let header = PacketHeader::new(
            PacketType::Command,
            ChannelType::ReliableOrdered,
            5,
            0,
            0,
            0,
        );

        let first = conn.handle_packet(header, &[0u8; 8]).unwrap();
        assert_eq!(first.reliable_arrival, Some(ReliableArrival::New));

        let again = conn.handle_packet(header, &[0u8; 8]).unwrap();
        assert_eq!(
            again.reliable_arrival,
            Some(ReliableArrival::Duplicate),
            "a retransmitted Command must not apply twice"
        );

        // The gap-filling neighbour is new; from now on 5 is behind the mark,
        // so even a THIRD copy of it stays suppressed.
        let mut filler = header;
        filler.sequence = 6;
        let third = conn.handle_packet(filler, &[0u8; 8]).unwrap();
        assert_eq!(third.reliable_arrival, Some(ReliableArrival::New));
        let old_again = conn.handle_packet(header, &[0u8; 8]).unwrap();
        assert_eq!(old_again.reliable_arrival, Some(ReliableArrival::Duplicate));
    }

    #[test]
    fn unreliable_channels_return_no_arrival_verdict() {
        let conn = test_connection();
        let header = PacketHeader::new(
            PacketType::Input,
            ChannelType::UnreliableSequenced,
            3,
            0,
            0,
            0,
        );
        let outcome = conn.handle_packet(header, &[0u8; 4]).unwrap();
        assert_eq!(
            outcome.reliable_arrival, None,
            "unreliable traffic has no verdict — and duplicates by design"
        );
    }

    #[test]
    fn unreliable_sends_never_reach_the_ledger() {
        let conn = test_connection();

        // Events (small and fragmenting — lent to CompartmentSealed, the
        // smallest packet variant) go out UNRELIABLE since №272; the retransmit
        // ledger must never resurrect them.
        //
        // NLLINE-CHECK-TEMP: with the Event/InputAck exclusion removed from
        // the fragment mapping this test goes red (fragments re-enter the
        // ReliableUnordered ledger).
        let small = conn.send_event(EventPacket {
            events: vec![GameEvent::CompartmentSealed {
                compartment: EntityId(1),
            }],
        });
        assert!(!small.is_empty(), "a small event must serialize");
        for packet in small {
            assert_eq!(
                channel_from_u8(packet.header.channel),
                ChannelType::UnreliableSequenced,
                "small events ride unreliable"
            );
        }
        assert!(
            conn.get_channel(ChannelType::ReliableUnordered)
                .unwrap()
                .pending_count()
                == 0
        );

        let big = conn.send_event(EventPacket {
            events: vec![
                GameEvent::CompartmentSealed {
                    compartment: EntityId(1),
                };
                8_000
            ],
        });
        assert!(big.len() > 1, "an 8k-event payload must fragment");
        for packet in big {
            assert_eq!(
                channel_from_u8(packet.header.channel),
                ChannelType::UnreliableSequenced,
                "fragmented events must not resurrect into the reliable bucket"
            );
        }

        assert!(
            conn.get_channel(ChannelType::ReliableUnordered)
                .unwrap()
                .pending_count()
                == 0
                && conn
                    .get_channel(ChannelType::ReliableOrdered)
                    .unwrap()
                    .pending_count()
                    == 0,
            "events (small or fragmented) never enter the ledger"
        );
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Invalid packet type: {0}")]
    InvalidPacketType(u8),
    #[error("Packet too large: {0} > {1}")]
    PacketTooLarge(usize, usize),
    #[error("Packet too small: {0} < {1}")]
    PacketTooSmall(usize, usize),
    #[error("Connection timed out")]
    Timeout,
    #[error("Connection closed")]
    Closed,
}

#[cfg(test)]
mod quiet_client_traffic_tests {
    //! Bugs №753 + №754: server-handled client traffic must not warn at the
    //! session level. LayerSync (№30 (B)) arrives in `handle_packet` on every
    //! client's layer-report beat while the session lives and the OLD catch-all
    //! warned "Unhandled packet type" per report — ~10 warns a second per
    //! client (№30 (B) reports every ~100 ms), ~1000 a second in the
    //! bot-shutdown flood window. ResyncRequest
    //! (№267 (A)) warned likewise for every legitimate request. Both are
    //! normal traffic NetServer applies in its own dispatch, so the session
    //! surface must stay silent on both. Machine-checkable through a capture
    //! subscriber: a WARN on an ordinary report/request is the defect.

    use super::*;
    use std::sync::Arc;

    /// A subscriber that only remembers events' composed messages.
    struct WarnCapture(Arc<parking_lot::Mutex<Vec<String>>>);

    impl WarnCapture {
        fn new() -> (Self, Arc<parking_lot::Mutex<Vec<String>>>) {
            let store: Arc<parking_lot::Mutex<Vec<String>>> = Arc::default();
            (Self(store.clone()), store)
        }
    }

    struct MessageOf<'a>(&'a mut String);

    impl tracing::field::Visit for MessageOf<'_> {
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            if field.name() == "message" {
                self.0.push_str(value);
            }
        }

        fn record_debug(
            &mut self,
            field: &tracing::field::Field,
            value: &dyn std::fmt::Debug,
        ) {
            if field.name() == "message" {
                self.0.push_str(&format!("{value:?}"));
            }
        }
    }

    impl tracing::Subscriber for WarnCapture {
        fn enabled(&self, _meta: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::Id {
            tracing::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _span: &tracing::Id, _follows: &tracing::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut message = event.metadata().level().to_string();
            event.record(&mut MessageOf(&mut message));
            self.0.lock().push(message);
        }

        fn enter(&self, _span: &tracing::Id) {}
        fn exit(&self, _span: &tracing::Id) {}
    }

    /// The wire body of an ordinary applied-layer report, as №30 (B) frames
    /// it: four layer clocks and the session echo.
    fn sync_report_payload() -> Vec<u8> {
        rfs_core::packet::encode(&LayerSyncPacket {
            applied: [12, 34, 56, 78],
            session: 42,
        })
        .unwrap()
    }

    #[test]
    fn an_ordinary_layer_sync_report_warns_nothing_and_replies_nothing() {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let conn = Connection::new(7, addr, ConnectionConfig::default());
        // The echo rides every gated packet (№216); handle_packet itself does
        // not verify it — that is the gate's job one level up.
        let header = PacketHeader::new(
            PacketType::LayerSync,
            ChannelType::UnreliableSequenced,
            1,
            0,
            0,
            42,
        );
        let (capture, seen) = WarnCapture::new();
        let outgoing = tracing::subscriber::with_default(capture, || {
            conn.handle_packet(header, &sync_report_payload())
                .expect("an ordinary report is not an error")
        });

        assert!(
            outgoing.outgoing.is_empty(),
            "a server-handled report owes the session level no reply packets"
        );
        let warned = seen.lock();
        assert!(
            warned.is_empty(),
            "no WARN may attach to an ordinary LayerSync report: {warned:?}"
        );
    }

    /// The wire body of an ordinary resync request (№267 (A)): the layer the
    /// client wants healed in full. Ungated (№216 lists its set explicitly),
    /// applied by NetServer's own dispatch.
    fn resync_request_payload() -> Vec<u8> {
        rfs_core::packet::encode(&ResyncRequestPacket { layer: 2 }).unwrap()
    }

    #[test]
    fn an_ordinary_resync_request_warns_nothing_and_replies_nothing() {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let conn = Connection::new(7, addr, ConnectionConfig::default());
        // As №753 frames it for the report type: the session echo is the
        // gate's business one level up; handle_packet itself does not verify it.
        let header = PacketHeader::new(
            PacketType::ResyncRequest,
            ChannelType::UnreliableSequenced,
            1,
            0,
            0,
            0,
        );
        let (capture, seen) = WarnCapture::new();
        let outgoing = tracing::subscriber::with_default(capture, || {
            conn.handle_packet(header, &resync_request_payload())
                .expect("an ordinary request is not an error")
        });

        assert!(
            outgoing.outgoing.is_empty(),
            "a server-handled request owes the session level no reply packets"
        );
        let warned = seen.lock();
        assert!(
            warned.is_empty(),
            "no WARN may attach to an ordinary ResyncRequest: {warned:?}"
        );
    }
}
