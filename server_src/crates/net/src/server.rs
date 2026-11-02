use crate::bandwidth::{Admission, BandwidthStats, BandwidthTracker};
use crate::connection::{
    is_reliable_channel, Connection, ConnectionConfig, ConnectionState, OutgoingPacket,
    ReliableArrival,
};
use crate::interest::InterestManager;
use crate::snapshot::{
    entity_layer, DeltaCompressor, EntitySnapshot, LayerBase, ProjectileSnapshot, Snapshot,
    SnapshotBuffer, LAYER_COUNT, LAYER_INTERVAL_TICKS, LAYER_RESYNC_TICKS,
};
use parking_lot::{Mutex, RwLock};
use rayon::prelude::*;
use rfs_core::entity::{PlayerEntity, ShipEntity};
use rfs_core::packet::*;
use rfs_core::spatial::InterestConfig;
use rfs_core::time::{Tick, TICK_DURATION, TICK_RATE};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Wire cap for a single postcard message (bug №732 rewords it; DoS: a
/// corrupted message framing could still OOM, so the cap keeps guarding the
/// length of the bytes we hand to a deserializer).
/// №239 addendum: an alias of the one statement in `connection` — the local
/// restatement is gone; making the alias unequal would be a compile-visible
/// act of renumbering, which is what the alias-for-number swap is for.
pub(crate) const WIRE_LIMIT: u64 = crate::connection::WIRE_LIMIT;

/// Bug №732: postcard has no Options dial, so the wire cap (№239 family)
/// keeps its job with a new shape — an explicit length check on the bytes we
/// are about to hand to a deserializer, then one decode. Trailing bytes are
/// refused by postcard; the pad → exact payload slice → this helper, so the
/// bytes checked here are exactly the bytes decoded.
fn deserialize_limited<T: serde::de::DeserializeOwned>(
    payload: &[u8],
) -> Result<T, bincode::Error> {
    if payload.len() as u64 > WIRE_LIMIT {
        return Err(bincode::Error::new(bincode::ErrorKind::Custom(
            format!(
                "payload of {} bytes exceeds the {WIRE_LIMIT}-byte parse cap",
                payload.len()
            ),
        )));
    }
    postcard::from_bytes(payload).map_err(encoding_error)
}

pub struct NetServer {
    socket: Arc<UdpSocket>,
    config: ServerConfig,
    connections: Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
    connection_by_addr: Arc<RwLock<HashMap<SocketAddr, u32>>>,
    next_connection_id: Arc<Mutex<u32>>,
    snapshot_buffer: Arc<SnapshotBuffer>,
    /// Bug №266: how many snapshot rounds the state loop has run, and how many
    /// packets those rounds put on the wire.
    ///
    /// The simulation runs at `tick_rate` and the loop costs far less than its
    /// budget, yet clients only learn about ticks at a lower rate than the
    /// server numbers them. The only place that gap can come from is this
    /// publication step, and until it was counted the server had no way to say
    /// which half of it was a missed round and which half was a layer that is
    /// simply not sent on this tick.
    snapshot_rounds: Arc<AtomicU64>,
    snapshot_packets: Arc<AtomicU64>,
    /// Bug №710: first-full builds admitted out of the join queue so far. The
    /// storm-side counterpart of the client's resync counter: ~4 per round
    /// during a flood draining to ~0 is the queue working; a value stuck at
    /// zero while clients sit Connected and unregistered is the queue
    /// starving (or the discovery path broken) — either way it is loud.
    first_fulls_built: Arc<AtomicU64>,
    /// Bug №710: the join admission queue itself, so disconnects between
    /// rounds are pruned by the admission stage instead of being admitted
    /// into a dead base.
    first_full_queue: Arc<Mutex<FirstFullQueue>>,
    /// Bug №274: deltas actually sent, per layer. The send-side half of the
    /// delivery measurement — the client's `applied_delta_count_by_layer` is
    /// the receive half, and a per-layer shortfall means different things on
    /// each side.
    sent_deltas_by_layer: Arc<[AtomicU64; LAYER_COUNT]>,
    delta_compressor: Arc<DeltaCompressor>,
    client_layers: Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
    /// Bug №30 (B): per-client queued sends awaiting the applied report, plus
    /// the self-shield's cooldown clock. The send pass queues here what went
    /// on the wire; the LayerSync dispatch installs from here the base the
    /// client actually holds — `client_layers` moves only on those installs.
    pending_confirms: Arc<Mutex<HashMap<u32, ClientLayerTrack>>>,
    interest_manager: Arc<InterestManager>,
    bandwidth_tracker: Arc<BandwidthTracker>,
    tick_rate: u32,
    /// The authoritative tick of the world, published by the simulation loop.
    ///
    /// Bug №268: this used to be a second, independent counter advanced by the
    /// send loop itself. Nothing tied it to the simulation, so every snapshot
    /// was stamped with a tick the world had not reached — and when the main
    /// loop overran its budget, `MissedTickBehavior::Skip` dropped simulation
    /// ticks silently while this counter kept climbing at a steady 30 Hz. The
    /// send loop now only reads it; `publish_tick` is the single writer.
    current_tick: Arc<Mutex<Tick>>,
    server_time: Arc<Mutex<f64>>,
    running: Arc<Mutex<bool>>,
    send_handle: Mutex<Option<JoinHandle<()>>>,
    recv_handle: Mutex<Option<JoinHandle<()>>>,
    /// Bug №613: both channels were `unbounded_channel`. An unbounded queue is
    /// an OOM waiting for a producer that outruns the consumer: every inbound
    /// datagram is turned into at least one event, and a client that floods can
    /// outpace the tick loop indefinitely. Both are bounded now, and the dropped
    /// counters say so out loud instead of the memory simply growing.
    event_sender: mpsc::Sender<ServerEvent>,
    event_receiver: Mutex<Option<mpsc::Receiver<ServerEvent>>>,
    match_id: Uuid,
    /// Per-address Connect token buckets (bug №27).
    connect_limits: Arc<Mutex<HashMap<SocketAddr, ConnectLimit>>>,
    /// Bug №172: single ordered outbox. Ack/event/disconnect packets used to
    /// spawn one task each, so the executor queue grew without bound and the
    /// UDP order between concurrently spawned tasks was never guaranteed.
    /// Everything now queues here and a single writer task drains it in order.
    outbox: mpsc::Sender<OutboxItem>,
    outbox_handle: Mutex<Option<JoinHandle<()>>>,
    /// Held until the server starts, then moved into the writer task.
    outbox_rx: Mutex<Option<mpsc::Receiver<OutboxItem>>>,
    /// Events dropped because the consumer fell behind (Bug №613).
    dropped_events: Arc<AtomicU64>,
    /// Outbox entries dropped for the same reason (Bug №613).
    dropped_outbox: Arc<AtomicU64>,
    /// Bug №698: the world's static description, sent to every client when it
    /// connects. The layers carry ships, players, stations, compartments and
    /// projectiles — and no structure, so a client that never heard of the
    /// world could not draw it and could not explain a shell vanishing at an
    /// invisible rock.
    world_description: Arc<RwLock<Option<rfs_core::world::MapDescription>>>,
    /// Bug №700: the shared auth key plus the per-peer outbound sequence
    /// table, carried together so every send path needs one handle instead
    /// of two.
    wire_auth: Arc<WireAuth>,
    /// Bug №216: un-confirmed connect challenges, by (claimed) address.
    /// The gate between "this datagram says Connect" and "this address owns
    /// a connection": without it the address registration trusts the sender
    /// field of a UDP datagram, which anyone with a raw socket can write.
    pending_challenges: Arc<RwLock<HashMap<SocketAddr, PendingChallenge>>>,
    /// Bug №216: inbound packets of the gated types with a session echo that
    /// does not match the connection the address is bound to — the spoof
    /// signature. Dropped before any state is touched.
    session_rejects: Arc<AtomicU64>,
}

/// Bug №700: the shared key and its outbound sequence table live in ONE
/// struct, so no send path re-implements "empty means unsigned" on its own.
struct WireAuth {
    key: Arc<RwLock<Option<Vec<u8>>>>,
    out_seq: Arc<Mutex<HashMap<SocketAddr, (Instant, u64)>>>,
}

impl WireAuth {
    fn new() -> Self {
        Self {
            key: Arc::new(RwLock::new(None)),
            out_seq: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Bug №700: the secret every datagram is keyed with. `None` or a
    /// blank token means the unsigned dialect — the caller says so in the
    /// log; here only the wire is concerned.
    fn set_token(&self, token: Option<String>) {
        *self.key.write() = token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .map(String::into_bytes);
    }

    fn key(&self) -> Option<Vec<u8>> {
        self.key.read().clone()
    }
}

/// Bug №700: the outbound sequence an unsigned peer's next datagram gets.
/// Wrap skips 0 — a zero sequence means "this server is unkeyed", and a
/// keyed server must never emit it.
fn next_out_seq(table: &Mutex<HashMap<SocketAddr, (Instant, u64)>>, addr: SocketAddr) -> u64 {
    let mut table = table.lock();
    let entry = table.entry(addr).or_insert((Instant::now(), 0));
    entry.0 = Instant::now();
    entry.1 = entry.1.wrapping_add(1);
    if entry.1 == 0 {
        entry.1 = 1;
    }
    entry.1
}

/// Bug №700: a rejection datagram carries the same auth block as everything
/// else — a forgeable rejection would be its own attack (spoofed ServerFull
/// rate-limits nobody but lies to the victim with no receipt left behind).
async fn send_reject(
    socket: &Arc<UdpSocket>,
    addr: SocketAddr,
    packet_data: &[u8],
    auth: &Arc<WireAuth>,
    bandwidth: &BandwidthTracker,
) -> anyhow::Result<()> {
    let seq = next_out_seq(&auth.out_seq, addr);
    let key = auth.key();
    let mut wire = Vec::with_capacity(crate::auth::AUTH_WIRE_LEN + packet_data.len());
    crate::auth::seal(
        key.as_deref(),
        crate::auth::Direction::ServerToClient,
        seq,
        packet_data,
        &mut wire,
    );
    socket.send_to(&wire, addr).await?;
    bandwidth.record_sent(wire.len());
    Ok(())
}

/// One queued datagram awaiting the writer task.
struct OutboxItem {
    conn: Arc<Connection>,
    packet: OutgoingPacket,
}

/// Bug №613: the event queue is bounded, so "the channel is full" is a real
/// outcome and not a hypothetical. Every send therefore goes through here: a
/// full queue drops the event and says so, instead of `let _ = …send()` which
/// made a dropped event look exactly like a delivered one. A closed channel is
/// not counted — nothing is waiting to read it any more, so the number would
/// only ever grow for a server that is already going down.
fn emit_event(tx: &mpsc::Sender<ServerEvent>, dropped: &AtomicU64, event: ServerEvent) {
    match tx.try_send(event) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            dropped.fetch_add(1, Ordering::Relaxed);
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {}
    }
}

/// Bug №267 (A): does this layer's pass have to be a full resync?
///
/// Two independent reasons produce one: the scheduled cadence (bug №173's
/// healing path — the client holds a base, so a full layer has to be sent
/// when the content is gone) and an explicit client request, which bypasses
/// the cadence because the client's base mismatch is a live stall.
///
/// Extracted from the snapshot pass so the decision is testable on its own:
/// the pass is one of the largest functions in the server, and a rule buried
/// in it is a rule nobody can check.
fn layer_needs_resync(
    layer: usize,
    since: u64,
    content_empty: bool,
    base_empty: bool,
    requested: bool,
) -> bool {
    if requested {
        return true;
    }
    since >= LAYER_RESYNC_TICKS[layer] && (!content_empty || !base_empty)
}

/// Bug №267 (A): apply an incoming resync request to the connection.
///
/// The layer index arrives from the wire, so it is bounds-checked here rather
/// than trusted. A malformed request must not be able to reach the layer
/// arrays.
fn apply_resync_request(conn: &Connection, payload: &[u8]) -> bool {
    match deserialize_limited::<ResyncRequestPacket>(payload) {
        Ok(request) => {
            if (request.layer as usize) < LAYER_COUNT {
                conn.request_resync(request.layer);
                true
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Bug №30 (B): the queued sends awaiting the client's applied report, per
/// client. Each layer's ring holds one entry per send that went on the wire:
/// entry `i` says "if the client applies this send, it holds exactly this
/// content, and its applied clock reads `base_tick`". A LayerSync report
/// names exactly such a tick; the matching entry is then PROOF of what the
/// client holds and is installed into `client_layers` as that layer's
/// confirmed base — which displaces the old blind "advance the base on every
/// queue" bookkeeping, because a queued send is not a delivered send.
#[derive(Debug, Default)]
struct ClientLayerTrack {
    /// Per layer, the queued-but-unconfirmed sends, oldest first.
    pending: [VecDeque<LayerBase>; LAYER_COUNT],
    /// Per layer, the tick of the last self-shield firing (its cooldown stamp).
    last_self_resync: [u64; LAYER_COUNT],
}

/// Bug №30 (B): how many pending-confirmation entries one layer's ring keeps.
/// Reports travel every ~100 ms and layers queue at their own intervals, so a
/// healthy ring is a few entries deep; the cap exists for the worst case (a
/// client that never reports), where the ring would otherwise keep whatever
/// the pass queued for as long as the connection lived.
pub(crate) const MAX_PENDING_CONFIRMS: usize = 4;

/// Bug №30 (B): the self-shield spacing. When the oldest queued send has gone
/// this many ticks without the client confirming anything, arm a resync:
/// either the report path is dead and the client's base has drifted away from
/// what the send chain assumes, or the client stalled outright — a full layer
/// heals both. The same span is the cooldown, so an unreporting client
/// settles into one full resync per interval instead of a resync storm.
const LAYER_CONFIRM_RESYNC_TICKS: [u64; LAYER_COUNT] = [30; LAYER_COUNT];

/// Bug №30 (B): pure rule for the self-shield. `oldest_pending_age` is the
/// age of the oldest still-unconfirmed send (0 = nothing pending — never
/// due); `since_self_resync` is the ticks since the shield last fired for
/// this layer. Extracted from the snapshot pass so the cadence is testable
/// the way `layer_needs_resync` is.
fn layer_confirm_self_resync_due(
    layer: usize,
    oldest_pending_age: u64,
    since_self_resync: u64,
) -> bool {
    oldest_pending_age >= LAYER_CONFIRM_RESYNC_TICKS[layer]
        && since_self_resync >= LAYER_CONFIRM_RESYNC_TICKS[layer]
}

/// Bug №30 (B): match an applied report against a layer's pending ring.
///
/// The report names the tick the client applied. An exact match with a ring
/// entry is proof of the client's state: that entry is returned for install.
/// Entries BELOW the applied tick are consumed either way — a send that was
/// superseded (the client has applied something newer) or dropped outright
/// has left the client's state past them, and no future report can name them
/// again, because applied clocks only move forward. No exact match means no
/// install: the confirmed base never moves on a guess.
fn take_confirmed_base(pending: &mut VecDeque<LayerBase>, applied: u64) -> Option<LayerBase> {
    let confirmed = pending
        .iter()
        .position(|entry| entry.base_tick == applied)
        .and_then(|pos| pending.remove(pos));
    while pending
        .front()
        .is_some_and(|entry| entry.base_tick < applied)
    {
        pending.pop_front();
    }
    confirmed
}

/// Bug №742: the applied-lag rows behind the stats surface. One current-tick
/// stamp and one read lock: every row's lag is measured against the SAME
/// server tick, so the rows stay comparable to each other — per-connection
/// stamps would sample different NOWs and turn the table into noise.
fn applied_lag_rows(
    server_tick: u64,
    connections: &RwLock<HashMap<u32, Arc<Connection>>>,
) -> Vec<(u32, Option<[u64; LAYER_COUNT]>)> {
    let connections = connections.read();
    let mut rows: Vec<(u32, Option<[u64; LAYER_COUNT]>)> = connections
        .iter()
        .map(|(&id, conn)| (id, conn.applied_lag(server_tick)))
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    rows
}

/// Bug №30 (B): merge an incoming report into the connection and install
/// what it confirms.
///
/// The session echo is checked before dispatch (the №216 gate); what remains
/// is the merge — `max` per layer, so duplicates and reordering cannot rewind
/// a confirmed base — and the install: for every layer whose merged value
/// matches a pending ring entry, that entry becomes the layer's confirmed
/// base in `client_layers`. No row yet (a report racing the first full)
/// simply skips the install: the admit path writes the base the full state
/// carries, and the next report re-confirms it.
fn apply_layer_sync_report(
    conn: &Connection,
    pending_confirms: &Arc<Mutex<HashMap<u32, ClientLayerTrack>>>,
    client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
    client_id: u32,
    payload: &[u8],
) -> bool {
    match deserialize_limited::<LayerSyncPacket>(payload) {
        Ok(report) => {
            conn.report_applied(report.applied);
            for layer in 0..LAYER_COUNT {
                // Read back the MERGED value, not the packet's field: a stale
                // report arriving out of order must not take an entry for a
                // tick the connection has already confirmed.
                let applied = conn.reported_applied(layer);
                let confirmed = {
                    let mut confirms = pending_confirms.lock();
                    confirms
                        .get_mut(&client_id)
                        .and_then(|track| take_confirmed_base(&mut track.pending[layer], applied))
                    // Released before `client_layers` is touched — the two
                    // maps are never held in one hand (bug №79 discipline).
                };
                if let Some(entry) = confirmed {
                    if let Some(row) = client_layers.write().get_mut(&client_id) {
                        row[layer] = entry;
                    }
                }
            }
            true
        }
        Err(_) => false,
    }
}

/// Token bucket for inbound Connects from one source address (bug №27).
/// Burst 4, refill 4/sec: a legitimate client (1 Connect + 250 ms retries
/// while the Accept is in flight) never notices; a sprayer is capped.
const CONNECT_BURST: f64 = 4.0;const CONNECT_REFILL_PER_SEC: f64 = 4.0;
/// Upper bound for the limiter table itself; oldest entries are evicted past it.
const CONNECT_TABLE_CAP: usize = 4096;
/// Bug №700: the same shape for the per-peer auth tables — a window entry
/// or a sequence counter nobody spoke to for a minute is dead and may go.
const PEER_TABLE_TTL: Duration = Duration::from_secs(60);

/// Bug №171: how long a reliable packet may wait for bandwidth tokens before
/// the send path gives up and returns an error instead of silently discarding
/// a message that has no retransmit behind it. Bounded so one slow client
/// cannot stall the whole send loop.
const RELIABLE_ADMIT_BUDGET: Duration = Duration::from_millis(250);

/// Bug №710: how many clients get their FIRST full state built per publish
/// round. The first full is per client expensive work — `compute_interest`,
/// a filtered world pass, a full `to_state_packet` serialization and the
/// layer split — and the old hot path did all of it inside ONE tick round
/// for every join that had just become ready: a connect storm of 200 bots
/// turned a single publish round into 200 first-full builds (measured during
/// the №697 flood: sim+sync 126-144 ms with only 13-73 players registered).
///
/// A budget of 4 drains a 200-join storm in ~1.7 s at 30 rounds/s while a
/// LONE late join is admitted one round later than before — the cost of the
/// guarantee is one publish round for the single client, and the benefit is
/// that the round itself stays inside the tick budget.
const FIRST_FULL_BUDGET_PER_ROUND: usize = 4;

/// Bug №216: how many un-confirmed connect challenges may be pending at once.
/// A challenge is per (claimed) address and lives until the nonce comes back
/// or the TTL expires, so the table is the one state an address sprayer can
/// grow — it must be finite.
const PENDING_CHALLENGE_CAP: usize = 1024;
/// Bug №216: how long a pending challenge is honored. An honest handshake
/// completes in three datagrams (Challenge, Confirm, Accept) — well under a
/// second on anything the game targets.
const CHALLENGE_TTL: Duration = Duration::from_secs(8);
/// Bug №216: a Connect signed with a session it does not know — the spoof
/// shape. Counted, not chatty: one line per 1024 rejections keeps the flood
/// visible without turning the log into the attack's amplifier.
const SESSION_REJECT_LOG_EVERY: u64 = 1024;

/// Bug №216: a Connect whose source address has not answered a challenge
/// yet. The recorded ConnectPacket is the one that carries the claimed
/// identity — when the nonce comes back, the admission proceeds with THESE
/// packets, not with the confirm's (a Confirm carries no identity at all).
struct PendingChallenge {
    nonce: u128,
    connect: ConnectPacket,
    since: Instant,
}

/// Bug №710: the join admission queue. Discovery ("this client is ready for
/// its first full") and admission ("build that first full now") are split:
/// the per-client pass in `send_snapshots` only DISCOVERS, and the admission
/// stage pops at most `FIRST_FULL_BUDGET_PER_ROUND` ready clients per round.
///
/// The queue is bounded twice over: by the dedupe set (a client discovered
/// in round N-1 and still not admitted is NOT re-added in round N+1, so the
/// storm cannot grow the queue with copies of itself) and by `CAP` (a queue
/// longer than this is a queue nobody can drain in any sane number of
/// rounds; exceptions are refused and re-discovered the next round).
#[derive(Default)]
struct FirstFullQueue {
    queue: VecDeque<u32>,
    waiting: HashSet<u32>,
}

impl FirstFullQueue {
    /// Bound on clients waiting for admission at once. 256 is far above any
    /// legitimate simultaneous join (the storm measured at 200) yet keeps the
    /// queue and its drain time finite even under a sprayer that never stops.
    const CAP: usize = 256;

    /// Discovery. Returns `false` when the client was already queued or the
    /// queue is full — both are non-events: the client either waits for a
    /// later round or is re-discovered next round.
    fn push(&mut self, id: u32) -> bool {
        if self.waiting.contains(&id) {
            return false;
        }
        if self.waiting.len() >= Self::CAP {
            return false;
        }
        self.waiting.insert(id);
        self.queue.push_back(id);
        true
    }

    /// Admission. Pops up to `budget` clients who are `ready`; clients that
    /// are not ready are dropped WITHOUT consuming budget slots — a queued
    /// client that disconnected (or whose player was despawned) while
    /// waiting must not stall the line's head, and a stale id must not be
    /// admitted at all (that would rebuild the №164 empty-base trap against
    /// a viewer that no longer exists). FIFO order is preserved.
    fn admit(&mut self, budget: usize, ready: impl Fn(u32) -> bool) -> Vec<u32> {
        let mut out = Vec::new();
        while out.len() < budget && !self.queue.is_empty() {
            let id = self.queue.pop_front().expect("queue checked non-empty");
            self.waiting.remove(&id);
            if ready(id) {
                out.push(id);
            }
        }
        out
    }

    /// How many clients are currently waiting for admission.
    /// Test-visible only: production reads the queue's drain state through
    /// the gateway counters, not by peeking at the line.
    #[cfg(test)]
    fn waiting_len(&self) -> usize {
        self.queue.len()
    }
}

/// Bug №710: the admission stage of a publish round.
///
/// Pops at most `FIRST_FULL_BUDGET_PER_ROUND` ids the queue marks ready —
/// Connected connection AND a registered player. Both conditions were
/// already enforced at discovery, but both can change between rounds, so
/// they are re-checked HERE: a client that disconnected while queued, or
/// lost its player, is dropped rather than admitted into an empty base
/// (the №164 shape, recreated through the queue if omitted).
///
/// For every admitted client the first full is built exactly as the hot
/// path used to do it in-line: refresh the interest sets, filter the world,
/// serialize the full state packet and split it into per-layer bases
/// registered in `client_layers`. Returns the packets to flush; called from
/// `send_snapshots` before the per-client pass, and directly from tests.
fn admit_first_fulls(
    connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
    client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
    interest_manager: &Arc<InterestManager>,
    snapshot: &Snapshot,
    first_full: &Arc<Mutex<FirstFullQueue>>,
    first_fulls_built: &AtomicU64,
) -> Vec<(Arc<Connection>, OutgoingPacket)> {
    // The ready-check holds a short connection-map read only; the expensive
    // per-client work never runs under any lock (bug №79 lock discipline).
    let ready = |id: u32| -> bool {
        let connected = {
            let conns = connections.read();
            conns
                .get(&id)
                .map(|c| c.state() == ConnectionState::Connected)
                .unwrap_or(false)
        };
        connected && interest_manager.has_player(id)
    };
    let admitted = first_full.lock().admit(FIRST_FULL_BUDGET_PER_ROUND, ready);

    let mut out: Vec<(Arc<Connection>, OutgoingPacket)> = Vec::new();
    for id in admitted {
        // Released before the work below; a connection that vanished between
        // the ready-check and here is skipped and will be re-discovered next
        // round by the ordinary discovery path.
        let conn = {
            let conns = connections.read();
            conns.get(&id).cloned()
        };
        let Some(conn) = conn else {
            continue;
        };

        // Exactly the work the old in-line first-full path performed.
        interest_manager.compute_interest(id);
        let filtered = NetServer::filter_snapshot(snapshot, interest_manager, id);
        let tick = filtered.tick.value();
        let already = client_layers.read().contains_key(&id);
        let mut layers = client_layers.write();
        layers.entry(id).or_insert_with(|| {
            // First contact: full state now, layer bases start from it.
            // Under bug №30 (B) the write itself stays trusted: the full
            // carries `tick`, the client applies it and its first report
            // re-confirms the very same base_tick. The ring is empty here,
            // so nothing double-books this state.
            for packet in conn.send_state(filtered.to_state_packet()) {
                out.push((conn.clone(), packet));
            }
            NetServer::split_layers(&filtered, tick)
        });
        if !already {
            first_fulls_built.fetch_add(1, Ordering::Relaxed);
        }
    }
    out
}

/// Bug №219: how long a Connected connection may go without its game-layer
/// player being registered. The snapshot gate (send_snapshots, №164) holds
/// every byte back until `interest_manager.has_player` is true, so an
/// unregistered player is a client that receives nothing — and, without this
/// reaper, waits forever on a silent socket. Registration in the happy path
/// happens in milliseconds (a spawn on ClientConnected), so ten seconds is
/// far beyond any legitimate stall and short enough to notice a wiring hole
/// "forgot to add the player" during one run, not a session later.
const PLAYER_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct ConnectLimit {
    tokens: f64,
    last: Instant,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub max_connections: usize,
    pub connection_config: ConnectionConfig,
    pub interest_config: InterestConfig,
    pub snapshot_history: usize,
    pub tick_rate: u32,
    pub max_bandwidth_bps: f64,
    pub enable_bandwidth_limit: bool,
    /// Server-side rate tracker window. Passed down to every connection's
    /// own tracker (bug №671) — before that, `with_window` had no caller at
    /// all and the trackers carried a hardcoded second.
    pub bandwidth_window: Duration,
    /// Bug №216: the shared secret the wire is keyed with, straight from the
    /// match config. `new` applies it while the server is being built —
    /// before the live receiver exists — so no datagram is ever processed in
    /// the unsigned dialect while `main` was still reading the disk. The
    /// gap is closed by construction; `set_auth_token` remains the runtime
    /// knob for callers that already hold a running server.
    pub auth_token: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:7777".parse().unwrap(),
            max_connections: 256,
            connection_config: ConnectionConfig::default(),
            interest_config: InterestConfig::default(),
            snapshot_history: 128,
            tick_rate: TICK_RATE,
            max_bandwidth_bps: 1024.0 * 1024.0,
            enable_bandwidth_limit: false,
            bandwidth_window: Duration::from_secs(1),
            auth_token: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected {
        connection_id: u32,
        addr: SocketAddr,
    },
    ClientDisconnected {
        connection_id: u32,
        reason: DisconnectReason,
    },
    ClientTimeout {
        connection_id: u32,
    },
    ClientInput {
        connection_id: u32,
        input: InputPacket,
    },
    ClientCommand {
        connection_id: u32,
        command: CommandPacket,
    },
    PacketReceived {
        connection_id: u32,
        packet_type: PacketType,
    },
    PacketSent {
        connection_id: u32,
        packet_type: PacketType,
        bytes: usize,
    },
    BandwidthStats {
        connection_id: u32,
        stats: BandwidthStats,
    },
    Error {
        connection_id: Option<u32>,
        error: String,
    },
}

impl NetServer {
    pub async fn new(config: ServerConfig) -> Result<Self, std::io::Error> {
        let socket = Arc::new(UdpSocket::bind(config.bind_addr).await?);
        // Bug №613: bounded, with a measured cap. 4096 events is far more than a
        // healthy tick loop ever has in flight, and small enough that a flood is
        // dropped rather than buffered until the process dies.
        const EVENT_QUEUE_LIMIT: usize = 4096;
        const OUTBOX_QUEUE_LIMIT: usize = 8192;
        let (event_sender, event_receiver) = mpsc::channel(EVENT_QUEUE_LIMIT);
        // Bug №172: the outbox and the channel holding its receiving end. The
        // receiver is moved into the writer task when the server starts.
        let (outbox, outbox_rx) = mpsc::channel(OUTBOX_QUEUE_LIMIT);
        let match_id = Uuid::new_v4();

        let server = Self {
            socket,
            config: config.clone(),
            connections: Arc::new(RwLock::new(HashMap::new())),
            connection_by_addr: Arc::new(RwLock::new(HashMap::new())),
            next_connection_id: Arc::new(Mutex::new(1)),
            snapshot_buffer: Arc::new(SnapshotBuffer::new(config.snapshot_history)),
            snapshot_rounds: Arc::new(AtomicU64::new(0)),
            snapshot_packets: Arc::new(AtomicU64::new(0)),
            first_fulls_built: Arc::new(AtomicU64::new(0)),
            first_full_queue: Arc::new(Mutex::new(FirstFullQueue::default())),
            // Bug №274: per-layer send counter, see the field.
            sent_deltas_by_layer: Arc::new(std::array::from_fn(|_| AtomicU64::new(0))),
            delta_compressor: Arc::new(DeltaCompressor::new()),
            client_layers: Arc::new(RwLock::new(HashMap::new())),
            pending_confirms: Arc::new(Mutex::new(HashMap::new())),
            interest_manager: Arc::new(InterestManager::new(config.interest_config)),
            bandwidth_tracker: Arc::new(BandwidthTracker::with_window(config.bandwidth_window)),
            // Bug №31: tick_rate 0 panics on 1000/0 and >1000 busy-loops the
            // send task. Clamp to the sane interval of 1..=1000 ticks/sec.
            tick_rate: config.tick_rate.clamp(1, 1000),
            current_tick: Arc::new(Mutex::new(Tick(0))),
            server_time: Arc::new(Mutex::new(0.0)),
            running: Arc::new(Mutex::new(false)),
            send_handle: Mutex::new(None),
            recv_handle: Mutex::new(None),
            event_sender,
            event_receiver: Mutex::new(Some(event_receiver)),
            match_id,
            connect_limits: Arc::new(Mutex::new(HashMap::new())),
            outbox,
            outbox_handle: Mutex::new(None),
            outbox_rx: Mutex::new(Some(outbox_rx)),
            dropped_events: Arc::new(AtomicU64::new(0)),
            dropped_outbox: Arc::new(AtomicU64::new(0)),
            world_description: Arc::new(RwLock::new(None)),
            // Bug №216: the token is part of construction, not an afterthought
            // — the receiver that exists the moment this struct does already
            // holds the key, so the unsigned window between "bind" and "apply
            // config" is closed by ordering alone.
            wire_auth: Arc::new({
                let auth = WireAuth::new();
                auth.set_token(config.auth_token.clone());
                auth
            }),
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
            session_rejects: Arc::new(AtomicU64::new(0)),
        };

        Ok(server)
    }

    pub fn match_id(&self) -> Uuid {
        self.match_id
    }

    /// Bug №698: the world this server runs, as the client is told about it.
    ///
    /// The server loads the map in the match binary; the network layer keeps
    /// the DESCRIPTION of it so it can hand it to every client that connects.
    pub fn set_world_description(&self, description: rfs_core::world::MapDescription) {
        *self.world_description.write() = Some(description);
    }

    /// The world description, if one was set.
    pub fn world_description(&self) -> Option<rfs_core::world::MapDescription> {
        self.world_description.read().clone()
    }

    /// Bug №700: the shared secret the server's datagrams are keyed with.
    /// `None` or a blank string is the unsigned dialect — the caller warns
    /// the human about it; here only the wire is concerned.
    pub fn set_auth_token(&self, token: Option<String>) {
        self.wire_auth.set_token(token);
    }

    pub fn current_tick(&self) -> Tick {
        *self.current_tick.lock()
    }

    /// Publish the tick the simulation has just finished (bug №268).
    ///
    /// Called once per simulation iteration, after that tick's state has been
    /// written into the interest manager, so a snapshot stamped with this tick
    /// is built from state that is at least this tick. The send loop is the
    /// only reader and stamps every packet with what it finds here.
    ///
    /// Monotonic on purpose: a caller that reports an older tick is ignored
    /// rather than allowed to rewind the world for every connected client.
    pub fn publish_tick(&self, tick: Tick) {
        let mut current = self.current_tick.lock();
        if tick > *current {
            *current = tick;
            *self.server_time.lock() = tick.value() as f64 * TICK_DURATION.as_secs_f64();
        }
    }

    pub fn server_time(&self) -> f64 {
        *self.server_time.lock()
    }

    pub fn connection_count(&self) -> usize {
        self.connections.read().len()
    }

    /// Bug №710: first-full builds admitted out of the join queue so far.
    pub fn first_fulls_built(&self) -> u64 {
        self.first_fulls_built.load(Ordering::Relaxed)
    }

    /// Bug №216: inbound packets carrying a session echo that did not match
    /// the connection their source address is bound to. In signed mode the
    /// HMAC already proves the datagram's origin; the session gate catches
    /// the spoof shape in the unsigned dialect where the datagram itself
    /// proves nothing.
    pub fn session_rejects(&self) -> u64 {
        self.session_rejects.load(Ordering::Relaxed)
    }

    /// Bug №216: how many connects are awaiting their challenge confirmation.
    pub fn pending_challenges(&self) -> usize {
        self.pending_challenges.read().len()
    }

    pub fn get_connection(&self, connection_id: u32) -> Option<Arc<Connection>> {
        self.connections.read().get(&connection_id).cloned()
    }

    pub fn get_connection_by_addr(&self, addr: SocketAddr) -> Option<Arc<Connection>> {
        // Bug №79: never hold the addr map and the connection map at once.
        // remove_connection takes them in the opposite order — nested locks
        // here are an ABBA deadlock (parking_lot is not reentrant).
        let id = self.connection_by_addr.read().get(&addr).copied()?;
        self.connections.read().get(&id).cloned()
    }

    pub fn start(&self) {
        *self.running.lock() = true;
        self.start_outbox_writer();
        self.start_receive_loop();
        self.start_send_loop();
        info!("NetServer started on {}", self.config.bind_addr);
    }

    pub fn stop(&self) {
        // Bug №610: the farewells have to be queued BEFORE the task that sends
        // them is aborted. The outbox has exactly one writer, and it was
        // aborted first, so every `Disconnect(ServerShutdown)` that
        // `disconnect_all` queued afterwards went into a channel with nobody
        // left reading it — the shutdown notice was dropped on the floor and
        // clients learned about the match ending from a silent socket.
        self.disconnect_all(DisconnectReason::ServerShutdown);
        *self.running.lock() = false;
        self.abort_tasks();
        info!("NetServer stopped");
    }

    /// `stop()`, but the outbox gets a bounded window to deliver the farewells.
    ///
    /// Bug №610: ordering alone is not enough — the farewell is queued, then the
    /// only task that can send it is aborted in the same breath. On a real
    /// socket that is a lost packet, which is exactly the case this path exists
    /// for. The pause is the drain window; it is short and it is paid once, at
    /// the end of a match.
    pub async fn stop_gracefully(&self) {
        const FLUSH_WINDOW: std::time::Duration = std::time::Duration::from_millis(100);
        self.disconnect_all(DisconnectReason::ServerShutdown);
        *self.running.lock() = false;
        tokio::time::sleep(FLUSH_WINDOW).await;
        self.abort_tasks();
        info!("NetServer stopped");
    }

    fn abort_tasks(&self) {
        if let Some(handle) = self.send_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.recv_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.outbox_handle.lock().take() {
            handle.abort();
        }
    }

    /// Bug №172: the single writer for every queued datagram.
    ///
    /// A FIFO channel plus one draining task replaces one spawned task per
    /// ack/event/disconnect. The executor queue no longer grows with traffic,
    /// and datagrams leave in the order they were queued — which the old
    /// racing tasks did not guarantee.
    fn start_outbox_writer(&self) {
        let Some(mut rx) = self.outbox_rx.lock().take() else {
            return;
        };
        let socket = self.socket.clone();
        let bandwidth = self.bandwidth_tracker.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let auth = self.wire_auth.clone();

        let handle = tokio::spawn(async move {
            while let Some(item) = rx.recv().await {
                let OutboxItem { conn, packet } = item;
                if let Err(e) =
                    Self::send_packet(&socket, &conn, &packet, &bandwidth, limit_enabled, &auth)
                        .await
                {
                    // An oversize or unadmittable packet is a per-packet
                    // problem; the connection keeps running.
                    warn!("outbox send failed for connection {}: {e}", conn.id);
                }
            }
        });
        *self.outbox_handle.lock() = Some(handle);
    }

    /// Bug №172: queue instead of spawning. Returns false only if the server
    /// has been dropped, which is the one case where a send is truly lost.
    ///
    /// Bug №613: the channel is bounded now, so `send` can also fail because it
    /// is FULL — a consumer that fell behind. That is a dropped packet, and it
    /// is counted: a silently dropped farewell or ack is the kind of failure
    /// that looks like a network problem for an hour.
    fn enqueue(&self, conn: Arc<Connection>, packets: Vec<OutgoingPacket>) {
        for packet in packets {
            match self.outbox.try_send(OutboxItem {
                conn: conn.clone(),
                packet,
            }) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    self.dropped_outbox.fetch_add(1, Ordering::Relaxed);
                    warn!("outbox full; dropping a packet for connection {}", conn.id);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    warn!("outbox closed; dropping packet for connection {}", conn.id);
                    return;
                }
            }
        }
    }

    fn start_receive_loop(&self) {
        let socket = self.socket.clone();
        let connections = self.connections.clone();
        let connection_by_addr = self.connection_by_addr.clone();
        let next_id = self.next_connection_id.clone();
        let config = self.config.clone();
        let bandwidth_tracker = self.bandwidth_tracker.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let connect_limits = self.connect_limits.clone();
        let running = self.running.clone();
        let event_sender = self.event_sender.clone();
        // Bug №613: the drop counter travels with the sender into the task, or a
        // bounded queue would drop events with nothing left to report it.
        let dropped_events = self.dropped_events.clone();
        let current_tick = self.current_tick.clone();
        let server_time = self.server_time.clone();
        let match_id = self.match_id;
        // Bug №30 (B): the receive path installs confirmed bases on reports.
        let pending_confirms = self.pending_confirms.clone();
        let client_layers = self.client_layers.clone();
        // Bug №700: the auth key goes with the loop, and every datagram
        // opens its block before any packet parsing. A datagram that does
        // not open is indistinguishable from network noise: dropped with a
        // count, no state touched.
        let auth = self.wire_auth.clone();
        // Per-peer replay windows, keyed the same way the handshake rate
        // table is, bounded with the same TTL sweep shape.
        let peer_replay: Arc<Mutex<HashMap<SocketAddr, (Instant, crate::auth::ReplayWindow)>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let world_description = self.world_description.clone();
        // Bug №216: the challenge table and the spoof counter travel with the
        // loop — the handshake happens inside the receive path itself.
        let pending_challenges = self.pending_challenges.clone();
        let session_rejects = self.session_rejects.clone();

        let handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];

            while *running.lock() {
                match socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        let datagram = &buf[..len];
                        bandwidth_tracker.record_received(len);

                        // Bug №700: the block is stripped and verified before
                        // anything else. WITHOUT a key the datagram must still
                        // carry the 24-byte block — the framing of the wire
                        // has one shape now, instead of "raw when unsigned".
                        let key = auth.key();
                        let (seq, data) = match crate::auth::unseal(
                            key.as_deref(),
                            crate::auth::Direction::ClientToServer,
                            datagram,
                        ) {
                            Some((seq, packet)) => (seq, packet),
                            None => {
                                debug!(
                                    "unauthenticated datagram ({} bytes) from {addr}",
                                    datagram.len()
                                );
                                continue;
                            }
                        };

                        // Replay freshness only means anything when a key is
                        // in play: an unsigned peer emits no sequence.
                        if key.is_some() {
                            let mut windows = peer_replay.lock();
                            let entry = windows.entry(addr).or_insert_with(|| {
                                (Instant::now(), crate::auth::ReplayWindow::default())
                            });
                            if !entry.1.accept(seq) {
                                warn!(
                                    "replayed datagram from {addr}: sequence {seq} is stale for its peer window"
                                );
                                continue;
                            }
                            entry.0 = Instant::now();
                            if windows.len() > CONNECT_TABLE_CAP {
                                windows.retain(|_, (last, _)| last.elapsed() < PEER_TABLE_TTL);
                            }
                        }

                        if let Err(e) = Self::handle_received_packet(
                            &socket,
                            &connections,
                            &connection_by_addr,
                            &next_id,
                            &config,
                            &bandwidth_tracker,
                            limit_enabled,
                            &connect_limits,
                            &event_sender,
                            &dropped_events,
                            &current_tick,
                            &server_time,
                            match_id,
                            &pending_confirms,
                            &client_layers,
                            &auth,
                            &world_description,
                            &pending_challenges,
                            &session_rejects,
                            addr,
                            data,
                        )
                        .await
                        {
                            warn!("Error handling packet from {}: {}", addr, e);
                            emit_event(
                                &event_sender,
                                &dropped_events,
                                ServerEvent::Error {
                                    connection_id: None,
                                    error: e.to_string(),
                                },
                            );
                        }
                    }
                    Err(e) => {
                        if *running.lock() {
                            error!("Receive error: {}", e);
                        }
                    }
                }
            }
        });

        *self.recv_handle.lock() = Some(handle);
    }

    /// Token-bucket admission for inbound Connects (bug №27).
    /// Returns true if this Connect may proceed, false if the sender is over rate.
    fn check_connect_rate(
        limits: &Arc<Mutex<HashMap<SocketAddr, ConnectLimit>>>,
        addr: SocketAddr,
    ) -> bool {
        let mut limits = limits.lock();
        let now = Instant::now();
        // Bound the table itself: evict entries idle > 60 s when over cap.
        if limits.len() > CONNECT_TABLE_CAP {
            limits.retain(|_, e| now.duration_since(e.last) < Duration::from_secs(60));
        }
        let entry = limits.entry(addr).or_insert(ConnectLimit {
            tokens: CONNECT_BURST,
            last: now,
        });
        entry.tokens = (entry.tokens
            + now.duration_since(entry.last).as_secs_f64() * CONNECT_REFILL_PER_SEC)
            .min(CONNECT_BURST);
        entry.last = now;
        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    // The per-connection shared state is passed explicitly rather than
    // bundled into a context struct, to keep the borrow scopes of the
    // individual locks narrow and obvious at each use site.
    #[allow(clippy::too_many_arguments)]
    async fn handle_received_packet(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        next_id: &Arc<Mutex<u32>>,
        config: &ServerConfig,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
        connect_limits: &Arc<Mutex<HashMap<SocketAddr, ConnectLimit>>>,
        event_sender: &mpsc::Sender<ServerEvent>,
        // Bug №613: the bounded event queue drops what it cannot hold, and
        // this is where that drop is counted.
        dropped_events: &AtomicU64,
        current_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        match_id: Uuid,
        // Bug №30 (B): the receive path installs confirmed bases on reports.
        pending_confirms: &Arc<Mutex<HashMap<u32, ClientLayerTrack>>>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        // Bug №700: unsealing happens in the receive loop; what reaches here
        // is an already-verified payload. The auth handle serves the OUTGOING
        // side (rejects) and the world reference serves the №698 connect path.
        auth: &Arc<WireAuth>,
        world_description: &Arc<RwLock<Option<rfs_core::world::MapDescription>>>,
        // Bug №216: the challenge table and the gate's counter, both part of
        // the handshake the receive path now owns.
        pending_challenges: &Arc<RwLock<HashMap<SocketAddr, PendingChallenge>>>,
        session_rejects: &Arc<AtomicU64>,
        addr: SocketAddr,
        data: &[u8],
    ) -> anyhow::Result<()> {
        if data.len() < HEADER_SIZE {
            return Err(anyhow::anyhow!(
                "Packet too small: {} < {}",
                data.len(),
                HEADER_SIZE
            ));
        }

        let (header, payload) = deserialize_packet(data)?;

        // Bug №176: the header carries a version, and a Connect carries
        // `protocol_version`, but neither was ever checked. A mismatched
        // client is admitted, and because the packet enums are positional the
        // two sides then disagree about what every subsequent field means.
        // Reject before any state is allocated.
        if header.version != PROTOCOL_VERSION {
            warn!(
                "rejecting packet from {addr}: header version {} != {PROTOCOL_VERSION}",
                header.version
            );
            return Ok(());
        }

        let connection_id = {
            let existing = connection_by_addr.read().get(&addr).copied();
            if let Some(id) = existing {
                id
            } else {
                // Bug №27: only a Connect opens a connection. Any other packet
                // from an unknown address is stray traffic (late duplicate,
                // scanner, spoof) — it must not allocate state or spawn a player.
                // Bug №216: except the handshake's own proof-of-life — the
                // Confirm answers a challenge we sent, and is the ONLY packet
                // that may complete the registration of an unknown address.
                if !matches!(
                    PacketType::from_u8(header.packet_type),
                    Some(PacketType::Connect) | Some(PacketType::ConnectConfirm)
                ) {
                    return Ok(());
                }
                // ...and Connects themselves are rate-limited per address, so
                // one sender cannot spray registrations at full line rate.
                // The Confirm shares the bucket: its cost is comparable, and
                // the honest path (Connect + Confirm + retries) stays far
                // below the burst.
                if !Self::check_connect_rate(connect_limits, addr) {
                    return Ok(());
                }

                // Bug №216: a Confirm without a pending challenge, with the
                // wrong nonce, or long after the challenge left, is refused —
                // the address proves nothing on its own.
                if PacketType::from_u8(header.packet_type) == Some(PacketType::ConnectConfirm) {
                    match deserialize_limited::<ConnectConfirmPacket>(payload) {
                        Ok(confirm) => {
                            let pending = pending_challenges.write().remove(&addr);
                            match pending {
                                Some(pending)
                                    if pending.nonce == confirm.nonce
                                        && pending.since.elapsed() <= CHALLENGE_TTL =>
                                {
                                    Self::admit_connection(
                                        socket,
                                        connections,
                                        connection_by_addr,
                                        next_id,
                                        config,
                                        bandwidth,
                                        limit_enabled,
                                        auth,
                                        event_sender,
                                        dropped_events,
                                        current_tick,
                                        server_time,
                                        match_id,
                                        world_description,
                                        pending.connect,
                                        addr,
                                    )
                                    .await?;
                                    return Ok(());
                                }
                                _ => {
                                    // The confirm is forged, stale or ours got
                                    // replaced by a newer claim. Nothing was
                                    // ever registered, so leak nothing: drop
                                    // without a reply.
                                    return Ok(());
                                }
                            }
                        }
                        Err(e) => {
                            warn!("malformed ConnectConfirm from {addr}: {e}");
                            return Ok(());
                        }
                    }
                }

                // Bug №176: check the build before allocating a slot. A
                // mismatched client must not consume a connection id, and must
                // not get as far as ClientConnected.
                let connect = match deserialize_limited::<ConnectPacket>(payload) {
                    Ok(connect) if connect.protocol_version != PROTOCOL_VERSION => {
                        warn!(
                            "rejecting Connect from {addr}: protocol {} != {PROTOCOL_VERSION}",
                            connect.protocol_version
                        );
                        let reject = ConnectRejectPacket {
                            reason: ConnectRejectReason::VersionMismatch,
                        };
                        let header = PacketHeader::new(
                            PacketType::ConnectReject,
                            ChannelType::ReliableOrdered,
                            0,
                            0,
                            0,
                            0,
                        );
                        let packet_data = serialize_packet(&reject, header)?;
                        send_reject(socket, addr, &packet_data, auth, bandwidth).await?;
                        return Ok(());
                    }
                    Ok(connect) => connect,
                    Err(e) => {
                        // An undecodable Connect is malformed, not a version
                        // problem — drop it silently rather than allocating.
                        warn!("rejecting malformed Connect from {addr}: {e}");
                        return Ok(());
                    }
                };

                // Bug №216: the address claim is NOT trusted yet. Registration
                // does not happen here — the source field of a UDP datagram is
                // raw and can be written by anyone, and a bare Connect would
                // bind the victim's address to the attacker. Instead: mint a
                // nonce, remember this Connect, and challenge the claimed
                // socket. Only the real listener of that address ever sees the
                // challenge, and only its Confirm carries the nonce.
                // Bug №216: ...challenge... (the serialized challenge leaves
                // the lock scope BEFORE the socket write: a parking_lot guard
                // is not Send, and this whole arm must stay a Send future.)
                let packet_data = {
                    let mut pending = pending_challenges.write();
                    if pending.len() >= PENDING_CHALLENGE_CAP {
                        warn!(
                            "challenge table full ({}), dropping Connect from {addr}",
                            pending.len()
                        );
                        return Ok(());
                    }
                    let nonce = Uuid::new_v4().as_u128();
                    let entry = PendingChallenge {
                        nonce,
                        connect: connect.clone(),
                        since: Instant::now(),
                    };
                    // A second Connect from the same address replaces the
                    // pending claim: the latest Connect speaks for whoever now
                    // claims it (the honest case is a client whose challenge
                    // reply was lost and who retries).
                    pending.insert(addr, entry);

                    let challenge = ConnectChallengePacket { nonce };
                    let header = PacketHeader::new(
                        PacketType::ConnectChallenge,
                        ChannelType::ReliableOrdered,
                        0,
                        0,
                        0,
                        0,
                    );
                    serialize_packet(&challenge, header)?
                };
                send_reject(socket, addr, &packet_data, auth, bandwidth).await?;
                return Ok(());
            }
        };

        let conn = connections.read().get(&connection_id).cloned();

        if let Some(conn) = conn {
            // Bug №216: the session gate. Heartbeat, Input and Command ARE the
            // packets a spoofed source would ride — they act as someone. The
            // session echo was carried over the accept, which only the real
            // listener of the claimed address ever received, so a mismatch
            // proves the datagram did not come from that listener. Refused
            // before any state (ack bookkeeping, liveness, ledgers) is touched.
            if !Self::session_gate(&conn, header.packet_type, payload) {
                let rejects = session_rejects.fetch_add(1, Ordering::Relaxed);
                if rejects.is_multiple_of(SESSION_REJECT_LOG_EVERY) {
                    warn!(
                        "session echo rejected for connection {connection_id} from {addr} \
                         (rejection no. {})",
                        rejects + 1
                    );
                }
                return Ok(());
            }

            // Bug №215: the outcome carries both the reply packets and the
            // reliable-arrival verdict the dispatch below needs.
            let outcome = conn.handle_packet(header, payload)?;

            for packet in outcome.outgoing {
                Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled, auth)
                    .await
                    .map(|_| ())?;
            }

            match PacketType::from_u8(header.packet_type) {
                Some(PacketType::Input) => {
                    if let Ok(input) = deserialize_limited::<InputPacket>(payload) {
                        emit_event(
                            event_sender,
                            dropped_events,
                            ServerEvent::ClientInput {
                                connection_id,
                                input,
                            },
                        );
                    }
                }
                Some(PacketType::Command) => {
                    // Bug №215: only a genuinely NEW reliable datagram applies.
                    // A retransmitted Command arrives because our answer ack
                    // was lost — executing it twice would be manifestly worse
                    // than the answer's delay, which the ledger (№215) now
                    // bounds instead of forever.
                    if outcome.reliable_arrival != Some(ReliableArrival::New) {
                        return Ok(());
                    }
                    if let Ok(command) = deserialize_limited::<CommandPacket>(payload) {
                        emit_event(
                            event_sender,
                            dropped_events,
                            ServerEvent::ClientCommand {
                                connection_id,
                                command,
                            },
                        );
                    }
                }
                Some(PacketType::Connect) => {
                    // Duplicate Connect: our Accept never made it back.
                    // Registration is keyed by addr, so re-sending is idempotent.
                    // Bug №176: a repeat Connect must still be version-checked —
                    // it is a fresh ConnectPacket, and accepting a mismatched
                    // build here would admit it after the first pass.
                    match deserialize_limited::<ConnectPacket>(payload) {
                        Ok(connect) if connect.protocol_version != PROTOCOL_VERSION => {
                            let reject = ConnectRejectPacket {
                                reason: ConnectRejectReason::VersionMismatch,
                            };
                            let header = PacketHeader::new(
                                PacketType::ConnectReject,
                                ChannelType::ReliableOrdered,
                                0,
                                0,
                                0,
                                0,
                            );
                            let packet_data = serialize_packet(&reject, header)?;
                            send_reject(socket, addr, &packet_data, auth, bandwidth).await?;
                            return Ok(());
                        }
                        _ => {}
                    }
                    let accept = {
                        let tick = current_tick.lock().value();
                        let time = *server_time.lock();
                        conn.send_connect_accept(tick, time, match_id)
                    };
                    if let Some(accept) = accept {
                        Self::send_packet(socket, &conn, &accept, bandwidth, limit_enabled, auth)
                            .await
                            .map(|_| ())?;
                    }
                    // Bug №698: the world, right after the accept and before
                    // any snapshot. The client cannot draw a world it was
                    // never told about, and a snapshot that arrives first
                    // would put entities in a void the client has no
                    // description of.
                    let description = world_description.read().clone();
                    if let Some(description) = description {
                        let packets = conn.send_map_description(&description);
                        for packet in packets {
                            Self::send_packet(
                                socket,
                                &conn,
                                &packet,
                                bandwidth,
                                limit_enabled,
                                auth,
                            )
                            .await
                            .map(|_| ())?;
                        }
                    }
                }
                Some(PacketType::Fragment) => {
                    if let Some((orig_type, bytes)) = conn.reassemble_fragment(payload) {
                        // Bug №216: the top-level gate validated the FRAGMENT
                        // datagram itself; identity packets come back here in
                        // reassembled form, with the session echo inside the
                        // reassembled bytes. Same gate, actual payload.
                        if matches!(orig_type, PacketType::Input | PacketType::Command)
                            && !Self::session_gate(&conn, orig_type as u8, &bytes)
                        {
                            session_rejects.fetch_add(1, Ordering::Relaxed);
                            return Ok(());
                        }
                        match orig_type {
                            PacketType::Input => {
                                if let Ok(input) = deserialize_limited::<InputPacket>(&bytes) {
                                    emit_event(
                                        event_sender,
                                        dropped_events,
                                        ServerEvent::ClientInput {
                                            connection_id,
                                            input,
                                        },
                                    );
                                }
                            }
                            PacketType::Command => {
                                if let Ok(command) = deserialize_limited::<CommandPacket>(&bytes) {
                                    emit_event(
                                        event_sender,
                                        dropped_events,
                                        ServerEvent::ClientCommand {
                                            connection_id,
                                            command,
                                        },
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some(PacketType::ResyncRequest) => {
                    // Bug №267 (A): a stalled layer asks for an immediate
                    // resync. One bit per connection is enough; the snapshot
                    // pass consumes it on the next tick.
                    apply_resync_request(&conn, payload);
                }
                Some(PacketType::LayerSync) => {
                    // Bug №30 (B): the client's applied-layer report — the same
                    // numbers its own guard checked deltas against. The session
                    // echo has passed the gate; merge the report and install
                    // whatever pending send it confirms as the layer's new
                    // diff base.
                    apply_layer_sync_report(
                        &conn,
                        pending_confirms,
                        client_layers,
                        connection_id,
                        payload,
                    );
                }
                Some(PacketType::Disconnect) => {
                    // Bug №68: a client Disconnect used to fall into `_ => {}` —
                    // only `last_received` was refreshed (extending the timeout!)
                    // and the player slot stayed occupied until the 10 s sweep.
                    // Remove the connection right here, idempotently.
                    let reason = deserialize_limited::<DisconnectPacket>(payload)
                        .map(|d| d.reason)
                        .unwrap_or(DisconnectReason::ClientQuit);
                    if Self::remove_connection(
                        connection_by_addr,
                        connections,
                        pending_confirms,
                        client_layers,
                        connection_id,
                        addr,
                    ) {
                        emit_event(
                            event_sender,
                            dropped_events,
                            ServerEvent::ClientDisconnected {
                                connection_id,
                                reason,
                            },
                        );
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Bug №216: whether an inbound packet's session echo matches the
    /// connection its source address is bound to. Types a spoofed source
    /// could act through (Heartbeat, HeartbeatAck, Input, Command, and — bug
    /// №30 (B) — LayerSync, whose numbers become the server's confirmed diff
    /// bases) must echo the session the server minted and delivered over the
    /// accept; every other type is ungated. A payload of a gated type that
    /// fails to deserialize is refused too — an unverifiable claim of
    /// identity is not an identity.
    fn session_gate(conn: &Connection, packet_type: u8, payload: &[u8]) -> bool {
        let expected = conn.session();
        match PacketType::from_u8(packet_type) {
            Some(PacketType::Heartbeat | PacketType::HeartbeatAck) => {
                deserialize_limited::<HeartbeatPacket>(payload)
                    .is_ok_and(|p| p.session == expected)
            }
            Some(PacketType::Input) => deserialize_limited::<InputPacket>(payload)
                .is_ok_and(|p| p.session == expected),
            Some(PacketType::Command) => deserialize_limited::<CommandPacket>(payload)
                .is_ok_and(|p| p.session == expected),
            // Bug №30 (B): the applied report turns into the confirmed base
            // of every layer — it gates exactly like the rest of the set.
            Some(PacketType::LayerSync) => deserialize_limited::<LayerSyncPacket>(payload)
                .is_ok_and(|p| p.session == expected),
            _ => true,
        }
    }

    /// Bug №216: the admission proper, run only after ConnectConfirm proved
    /// the claimed address actually listens. Extracted from the old
    /// unknown-addr Connect path so the challenge handshake can call it; the
    /// body is the registration, the ClientConnected event, the accept
    /// (which carries the session the client will echo from now on) and the
    /// static world description.
    #[allow(clippy::too_many_arguments)]
    async fn admit_connection(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        next_id: &Arc<Mutex<u32>>,
        config: &ServerConfig,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
        auth: &Arc<WireAuth>,
        event_sender: &mpsc::Sender<ServerEvent>,
        dropped_events: &AtomicU64,
        current_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        match_id: Uuid,
        world_description: &Arc<RwLock<Option<rfs_core::world::MapDescription>>>,
        // The claimed identity, already version-checked at the challenge
        // stage. Kept on the signature so the admit path stays reviewable
        // as "this connect packet became this connection" — the fields the
        // game layer consumes (player name) reach it through ClientConnected
        // handlers, not through here.
        _connect: ConnectPacket,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        // Bug №27: the cap check and the registration run under one
        // write lock, so a concurrent Connect cannot slip between the
        // len check and the insert (TOCTOU in the old read-then-write).
        let admitted: Option<(u32, Arc<Connection>)> = {
            let mut conns = connections.write();
            if conns.len() >= config.max_connections {
                None
            } else {
                let id = {
                    let mut id_gen = next_id.lock();
                    let id = *id_gen;
                    *id_gen = id_gen.wrapping_add(1);
                    if *id_gen == 0 {
                        *id_gen = 1;
                    }
                    id
                };

                // Bug №176: hand each connection its own send budget
                // instead of sharing one server-wide bucket.
                let conn_config = ConnectionConfig {
                    max_bandwidth_bps: config.max_bandwidth_bps,
                    bandwidth_window: config.bandwidth_window,
                    ..config.connection_config.clone()
                };
                let conn = Connection::new(id, addr, conn_config);
                conn.set_state(ConnectionState::Connected);
                let conn_arc = Arc::new(conn);
                conns.insert(id, conn_arc.clone());
                connection_by_addr.write().insert(addr, id);
                Some((id, conn_arc))
            }
        };

        let Some((id, conn_arc)) = admitted else {
            let reject = ConnectRejectPacket {
                reason: ConnectRejectReason::ServerFull,
            };
            let header = PacketHeader::new(
                PacketType::ConnectReject,
                ChannelType::ReliableOrdered,
                0,
                0,
                0,
                0,
            );
            let packet_data = serialize_packet(&reject, header)?;
            send_reject(socket, addr, &packet_data, auth, bandwidth).await?;
            return Ok(());
        };

        let accept = {
            let tick = current_tick.lock().value();
            let time = *server_time.lock();
            conn_arc.send_connect_accept(tick, time, match_id)
        };

        emit_event(
            event_sender,
            dropped_events,
            ServerEvent::ClientConnected {
                connection_id: id,
                addr,
            },
        );

        if let Some(accept) = accept {
            Self::send_packet(socket, &conn_arc, &accept, bandwidth, limit_enabled, auth)
                .await
                .map(|_| ())?;
        }

        // Bug №698: the world, right after the accept and before any
        // snapshot — the №698 order, moved here from the packet-type arm,
        // because a confirmed connect no longer travels as a Connect packet
        // and would never pass that arm again.
        let description = world_description.read().clone();
        if let Some(description) = description {
            let packets = conn_arc.send_map_description(&description);
            for packet in packets {
                Self::send_packet(socket, &conn_arc, &packet, bandwidth, limit_enabled, auth)
                    .await
                    .map(|_| ())?;
            }
        }

        Ok(())
    }

    async fn send_packet(
        socket: &Arc<UdpSocket>,
        conn: &Connection,
        packet: &OutgoingPacket,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
        auth: &Arc<WireAuth>,
    ) -> anyhow::Result<bool> {
        // Bug №700: the wire now also carries the auth block on EVERY
        // datagram, so the MTU guard and the limiter both count it.
        let total_size = crate::auth::AUTH_WIRE_LEN + HEADER_SIZE + packet.payload.len();
        // Bug №30: the MTU comes from the connection's own config — the same
        // value the packet builders fragment against — not a stray constant.
        let max = conn.config.max_packet_size;
        if total_size > max {
            return Err(anyhow::anyhow!(
                "Packet too large: {} > {}",
                total_size,
                max
            ));
        }
        // Bug №39: the limiter gates the wire.
        // Bug №171: dropping is only acceptable for unreliable traffic, whose
        // next update supersedes it. A reliable packet with no retransmit
        // queue behind it (the Channel layer is still unwired) would be lost
        // permanently, so it waits for tokens up to a bounded budget and
        // reports an error if even that is not enough.
        if limit_enabled {
            let reliable = is_reliable_channel(packet.header.channel);
            let mut budget = RELIABLE_ADMIT_BUDGET;
            loop {
                match conn.bandwidth_limiter().admit(total_size, reliable) {
                    Admission::Send => break,
                    Admission::Drop => return Ok(false),
                    Admission::Retry(wait) => {
                        if wait >= budget {
                            return Err(anyhow::anyhow!(
                                "limiter: reliable {} packet ({total_size} B) not admitted \
                                 within {RELIABLE_ADMIT_BUDGET:?}",
                                packet.header.channel
                            ));
                        }
                        tokio::time::sleep(wait).await;
                        budget -= wait;
                    }
                }
            }
        }

        let mut buffer = Vec::with_capacity(total_size);
        // Bug №700: the packet bytes are built first, then the auth block
        // rides in front — this is the one production seal for connected
        // sends. A reconnect makes a fresh connection with a fresh counter,
        // and the CLIENT's window remembers the old numbers, so a stale
        // datagram cannot pass for new at the far end.
        // Bug №732: the header is the hand-rolled fixed 24-byte codec
        // (`PacketHeader::to_wire`); nothing here can fail, no `?` needed.
        let mut packet_bytes = packet.header.to_wire();
        packet_bytes.extend_from_slice(&packet.payload);
        let seq = {
            let mut seq = conn.wire_seq.lock();
            *seq = seq.wrapping_add(1);
            if *seq == 0 {
                *seq = 1;
            }
            *seq
        };
        let key = auth.key();
        crate::auth::seal(
            key.as_deref(),
            crate::auth::Direction::ServerToClient,
            seq,
            &packet_bytes,
            &mut buffer,
        );

        socket.send_to(&buffer, conn.addr).await?;
        bandwidth.record_sent(total_size);
        Ok(true)
    }

    fn start_send_loop(&self) {
        let socket = self.socket.clone();
        let connections = self.connections.clone();
        let connection_by_addr = self.connection_by_addr.clone();
        let client_layers = self.client_layers.clone();
        // Bug №30 (B): the pass queues pending confirms and reads the live
        // diff anchor (the newest queued send) from here.
        let pending_confirms = self.pending_confirms.clone();
        let snapshot_buffer = self.snapshot_buffer.clone();
        // Bug №266: counted so the publication rate is measurable, not assumed.
        let snapshot_rounds = self.snapshot_rounds.clone();
        let snapshot_packets = self.snapshot_packets.clone();
        // Bug №274: shared per-layer send counter.
        let sent_deltas_by_layer = self.sent_deltas_by_layer.clone();
        let delta_compressor = self.delta_compressor.clone();
        let interest_manager = self.interest_manager.clone();
        let bandwidth_tracker = self.bandwidth_tracker.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let auth = self.wire_auth.clone();
        let event_sender = self.event_sender.clone();
        let running = self.running.clone();
        // Bug №710: the join admission queue travels with the send loop; the
        // queue is shared with nothing else, so round N+1 continues round N's
        // line instead of restarting it.
        let first_full_queue = self.first_full_queue.clone();
        let first_fulls_built = self.first_fulls_built.clone();
        // Bug №216: challenge cells expire through the same sweep pass the
        // timeouts run.
        let pending_challenges = self.pending_challenges.clone();
        // Bug №613: as in the receive loop, the drop counter goes with the
        // sender into the task.
        let dropped_events = self.dropped_events.clone();
        let current_tick = self.current_tick.clone();
        let server_time = self.server_time.clone();
        let tick_rate = self.tick_rate;

        let handle = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_millis(1000 / tick_rate as u64));
            // Bug №268: this loop no longer owns a tick counter. It reads the
            // tick the simulation published and stamps every packet with it, so
            // a client is never told the world advanced past the state it was
            // actually built from.
            //
            // Snapshot work is skipped when the simulation has not reached a
            // new tick. That is the honest behaviour and it is also the cheap
            // one: the world did not move, so there is nothing new to say, and
            // re-sending the same tick would burn the tick budget saying so.
            // The wall-clock cadence is unchanged, so per-layer rates measured
            // in ticks (`LAYER_INTERVAL_TICKS`) still hold: when the simulation
            // keeps up, consecutive rounds differ by one tick and layer 0 still
            // goes out every second tick; when it falls behind, every round is
            // a fresh tick and the layers publish at whatever the simulation
            // actually achieved. Heartbeats and timeout reaping are NOT part of
            // that skip — see the notes at their call sites.
            //
            // Bug #267 follow-up (REVERTED): setting MissedTickBehavior::Delay
            // here to clamp runaway publish rounds regressed the healthy 50-bot
            // case from 18.5 to 0.82 Hz applied — the multi-round behaviour
            // (rounds_per_tick 4-5.5 under flood) is a symptom of the connect
            // storm, not its cause; throttling it starves clients even when the
            // loop can keep up. Left at the default Burst. Skipping unchanged
            // ticks is not that throttle: it removes rounds that carried no new
            // state in the first place. The real cost during the flood is
            // sim+sync 126-144ms with only 13-73 players registered: connect
            // handling / interest / event drain, not network serialization.
            // Queue: make Connect admission and the first-full per client cheap
            // and staggered so 200 joins do not dominate the tick budget.
            let mut last_published: Option<Tick> = None;
            // Timeout reaping is wall-clock work, not simulation work: it must
            // keep running at ~1 Hz even when the simulation is far behind.
            // Bug №268: this used to be derived from the tick counter, so a slow
            // simulation silently stopped reaping dead connections.
            let mut rounds: u64 = 0;

            while *running.lock() {
                interval.tick().await;
                rounds += 1;

                // Bug №28: reap silent connections once a second so dead
                // clients stop consuming snapshots, interest and ids.
                //
                // Bug №268: this runs BEFORE the unchanged-tick skip on
                // purpose. Reaping is wall-clock work — a connection that has
                // stopped answering is dead whether or not the world moved —
                // so it must not be tied to the simulation keeping up. Sitting
                // after the skip, a lagging simulation would also stop
                // reaping, and dead clients would keep consuming interest and
                // snapshot work forever, which is the exact leak the sweep
                // exists to prevent.
                if rounds.is_multiple_of(tick_rate as u64) {
                    Self::sweep_timeouts(
                        &connections,
                        &connection_by_addr,
                        &pending_confirms,
                        &client_layers,
                        &event_sender,
                        &dropped_events,
                        &pending_challenges,
                    );
                }

                let tick = *current_tick.lock();
                if last_published != Some(tick) {
                    last_published = Some(tick);
                    let time = *server_time.lock();

                    let snapshot = Self::build_snapshot(tick, time, &interest_manager);
                    snapshot_buffer.write_snapshot(snapshot.clone());
                    snapshot_rounds.fetch_add(1, Ordering::Relaxed);

                    Self::send_snapshots(
                        &socket,
                        &connections,
                        &client_layers,
                        &pending_confirms,
                        &snapshot_buffer,
                        &delta_compressor,
                        &interest_manager,
                        &bandwidth_tracker,
                        limit_enabled,
                        tick,
                        &snapshot_packets,
                        &sent_deltas_by_layer,
                        &auth,
                        &first_full_queue,
                        &first_fulls_built,
                    )
                    .await;
                }

                // Heartbeats are liveness, not state. Bug №268: this used to
                // sit behind the snapshot work, so skipping a round also
                // skipped the heartbeat — and since the timeout sweep runs
                // unconditionally, a lagging simulation would have made every
                // connected client look dead and reaped it. The two must be
                // independent: the world can stand still while the server
                // still has to prove it is there.
                Self::send_heartbeats(
                    &socket,
                    &connections,
                    &bandwidth_tracker,
                    limit_enabled,
                    &auth,
                )
                .await;

                // Bug №215: retransmit pass immediately after the heartbeat
                // pass. Wall-clock work next to wall-clock work; neither is
                // part of the unchanged-tick skip above.
                Self::send_retransmits(
                    &socket,
                    &connections,
                    &bandwidth_tracker,
                    limit_enabled,
                    &auth,
                )
                .await;
            }
        });

        *self.send_handle.lock() = Some(handle);
    }

    /// Remove every connection that stopped answering (bug №28). Removal and
    /// event emission go through one helper so each timeout is reported
    /// exactly once even if a manual disconnect races it (bug №32).
    fn sweep_timeouts(
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        // Bug №30 (B): the sweep deletes the rings of the connections it reaps,
        // where the delta compressor's remove_client used to sit.
        pending_confirms: &Arc<Mutex<HashMap<u32, ClientLayerTrack>>>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        event_sender: &mpsc::Sender<ServerEvent>,
        // Bug №613: the bounded event queue drops what it cannot hold, and
        // this is where that drop is counted.
        dropped_events: &AtomicU64,
        // Bug №216: the challenge pass rides the same once-a-second sweep.
        pending_challenges: &Arc<RwLock<HashMap<SocketAddr, PendingChallenge>>>,
    ) {
        // Bug №216: challenges whose address never came back — expired, or
        // never honest — go with the once-a-second pass. The table only
        // grows through Connect-spam otherwise, so the sweep is what makes
        // the challenge memory bounded in reality, not just by its CAP.
        pending_challenges.write().retain(|addr, pending| {
            let keep = pending.since.elapsed() <= CHALLENGE_TTL;
            if !keep {
                debug!("challenge for {addr} expired unanswered");
            }
            keep
        });
        let timed_out: Vec<(u32, SocketAddr)> = {
            let conns = connections.read();
            conns
                .iter()
                .filter(|(_, conn)| conn.is_timed_out())
                .map(|(id, conn)| (*id, conn.addr))
                .collect()
        };
        // Bug №219: Connected, but the game layer never registered the
        // player (failed spawn, entity cap, a forgotten add_player
        // registration). The snapshot gate keeps every state byte back in
        // exactly this state, so the client sits on a silent socket for the
        // whole session unless this pass ends it.
        let unregistered: Vec<(u32, SocketAddr)> = {
            let conns = connections.read();
            let layers = client_layers.read();
            conns
                .iter()
                .filter(|(_, conn)| {
                    conn.state() == ConnectionState::Connected
                        && !layers.contains_key(&conn.id)
                        && conn.age_connected() >= PLAYER_WAIT_TIMEOUT
                })
                .map(|(id, conn)| (*id, conn.addr))
                .collect()
        };
        // Bug №272: this sweep runs once a second, so it is also where the
        // per-connection reliable-window overflow gets reported — in bulk,
        // once per connection, instead of once per dropped entry. See
        // `Connection::log_pending_ack_summary` for why that moved.
        {
            let conns = connections.read();
            let mut dropped_total = 0u64;
            let mut worst: Option<(u32, u64)> = None;
            for (id, conn) in conns.iter() {
                let drops = conn.pending_ack_drops_since(0);
                if drops > 0 {
                    dropped_total += drops;
                    if worst.is_none_or(|(_, d)| drops > d) {
                        worst = Some((*id, drops));
                    }
                }
            }
            if let Some((id, _)) = worst {
                warn!(
                    "reliable windows: {dropped_total} unacked entries aged out across {} \
                     connections (worst: conn {id}); no retransmit layer exists, so these \
                     were never recoverable — see bug №272",
                    conns.len(),
                );
            }
        }
        for (id, addr) in timed_out {
            if Self::remove_connection(
                connection_by_addr,
                connections,
                pending_confirms,
                client_layers,
                id,
                addr,
            ) {
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientDisconnected {
                        connection_id: id,
                        reason: DisconnectReason::Timeout,
                    },
                );
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientTimeout { connection_id: id },
                );
            }
        }
        for (id, addr) in unregistered {
            warn!(
                "connection {id}: player never registered within {PLAYER_WAIT_TIMEOUT:?}; \
                 disconnecting (the layer gate holds all state until has_player turns \
                 true — bug №219)"
            );
            // Same one-shot helper as the timeout sweep (bug №32): the removal
            // either happens here exactly once, or never because a manual
            // disconnect raced it.
            if Self::remove_connection(
                connection_by_addr,
                connections,
                pending_confirms,
                client_layers,
                id,
                addr,
            ) {
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::Error {
                        connection_id: Some(id),
                        error: format!("player never registered within {PLAYER_WAIT_TIMEOUT:?}"),
                    },
                );
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientDisconnected {
                        connection_id: id,
                        reason: DisconnectReason::Timeout,
                    },
                );
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientTimeout { connection_id: id },
                );
            }
        }
    }

    /// Erase a connection from every table. Returns true if it was present —
    /// callers emit disconnect events only then, so a timeout racing a manual
    /// disconnect cannot double-report (bug №32).
    fn remove_connection(
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        // Bug №30 (B): the pending rings die with their client — a reconnect
        // must not inherit sends queued for a predecessor. Takes the spot the
        // stateless delta compressor's remove_client used to hold.
        pending_confirms: &Arc<Mutex<HashMap<u32, ClientLayerTrack>>>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        connection_id: u32,
        addr: SocketAddr,
    ) -> bool {
        let removed = connections.write().remove(&connection_id);
        connection_by_addr.write().remove(&addr);
        pending_confirms.lock().remove(&connection_id);
        client_layers.write().remove(&connection_id);
        removed.is_some()
    }

    fn build_snapshot(tick: Tick, time: f64, interest_manager: &Arc<InterestManager>) -> Snapshot {
        let mut entities = Vec::new();
        let mut projectiles = Vec::new();

        for ship in interest_manager.get_ships() {
            entities.push(EntitySnapshot::from(&ship));
        }
        for station in interest_manager.get_stations() {
            entities.push(EntitySnapshot::from(&station));
        }
        for player in interest_manager.get_players() {
            entities.push(EntitySnapshot::from(&player));
        }
        for compartment in interest_manager.get_compartments() {
            entities.push(EntitySnapshot::from(&compartment));
        }
        for projectile in interest_manager.get_projectiles() {
            projectiles.push(ProjectileSnapshot::from(&projectile));
        }

        Snapshot {
            tick,
            time,
            entities,
            projectiles,
            events: Vec::new(),
        }
    }

    // The per-connection shared state is passed explicitly rather than
    // bundled into a context struct, to keep the borrow scopes of the
    // individual locks narrow and obvious at each use site.
    #[allow(clippy::too_many_arguments)]
    async fn send_snapshots(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        // Bug №30 (B): the rings the pass queues into and reads the live diff
        // anchor (the newest queued send) from, plus the self-shield clocks.
        pending_confirms: &Arc<Mutex<HashMap<u32, ClientLayerTrack>>>,
        snapshot_buffer: &Arc<SnapshotBuffer>,
        delta_compressor: &Arc<DeltaCompressor>,
        interest_manager: &Arc<InterestManager>,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
        current_tick: Tick,
        snapshot_packets: &Arc<AtomicU64>,
        // Bug №274: passed so the send path can record what it actually put on
        // the wire, per layer.
        sent_deltas_by_layer: &Arc<[AtomicU64; LAYER_COUNT]>,
        // Bug №700: the last hop of the seal, from the send loop.
        auth: &Arc<WireAuth>,
        // Bug №710: the join admission queue and its counter. Discovery and
        // first-full building are split; the queue is the contract between
        // the per-client pass (discovery only) and `admit_first_fulls`
        // (at most `FIRST_FULL_BUDGET_PER_ROUND` builds per round).
        first_full_queue: &Arc<Mutex<FirstFullQueue>>,
        first_fulls_built: &Arc<AtomicU64>,
    ) {
        // Bug №745: `get_latest` hands out the ring's shared allocation; the
        // empty fallback (nothing written yet) is one extra allocation for
        // the whole publish pass to read through `&Snapshot`.
        let snapshot: Arc<Snapshot> = snapshot_buffer.get_latest().unwrap_or_else(|| {
            Arc::new(Snapshot {
                tick: current_tick,
                time: current_tick.value() as f64 * TICK_DURATION.as_secs_f64(),
                entities: Vec::new(),
                projectiles: Vec::new(),
                events: Vec::new(),
            })
        });

        // Bug #267: at 50+ clients the per-client filter+diff+serialize inside
        // the tick budget is the wall (applied rate collapsed to 0.86 Hz at 200
        // bots; the payload cache alone only reached 3.76 Hz). Two levers land
        // here: a layer-delta payload cache across clients sharing a base tick,
        // and running the per-client work across CPU cores with rayon. The
        // cache is mutex-wrapped — content-equal hits are short and only the
        // shared layers hit, so contention is minimal and the interest-varying
        // player layer runs in parallel. The final UDP flush stays sequential:
        // socket sends are cheap; the CPU work was the bottleneck.
        //
        // Optimization: cache serialized payloads by content hash, not by
        // client ID. Clients on the same ship see identical layer content, so
        // this allows payload reuse across clients. The cache key is now
        // (layer, resync, base_tick, target_tick, content_hash) where
        // content_hash is computed from the actual entity/projectile data.
        type DeltaCacheKey = (u8, bool, u64, u64, u64);
        type DeltaCacheValue = (
            Vec<EntitySnapshot>,
            Vec<ProjectileSnapshot>,
            Vec<EntitySnapshot>,
            Vec<ProjectileSnapshot>,
            Vec<u8>,
        );
        // Bug №714: parking_lot — a poisoned std lock (a panic in another
        // handler) used to surface as a panicking unwrap on the snapshot
        // path; parking_lot reports the plain state instead.
        let delta_cache: Mutex<HashMap<DeltaCacheKey, DeltaCacheValue>> =
            Mutex::new(HashMap::new());

        // Bug №29: snapshot the client list first, then take the layers lock        // per client and only for map access. Serialization, fragmentation
        // and delta diffing (the expensive parts at 200 clients) run without
        // any global lock held, so disconnects/timeouts never stall behind them.
        let clients: Vec<(u32, Arc<Connection>)> = {
            let conns = connections.read();
            conns.iter().map(|(id, conn)| (*id, conn.clone())).collect()
        };

        // Bug №710: admission BEFORE the per-client pass. Every client the
        // queue pop marks ready gets its first full now; the storms' rest
        // stay queued for later rounds. The packets ride in the same flush
        // as everything else the round produced.
        let admitted_batch = admit_first_fulls(
            connections,
            client_layers,
            interest_manager,
            &snapshot,
            first_full_queue,
            first_fulls_built,
        );

        let parallel_batches: Vec<Vec<(Arc<Connection>, OutgoingPacket)>> = clients
            .par_iter()
            .map(|(client_id, conn)| {
                let mut out: Vec<(Arc<Connection>, OutgoingPacket)> = Vec::new();
                if conn.state() != ConnectionState::Connected {
                    return out;
                }

                // Bug №164: the first full used to be built the moment the
                // connection appeared, which is before the game layer has
                // registered the new player. `should_replicate_entity` answers
                // `false` for an unknown viewer, so the client was locked into an
                // empty base and saw nothing until the first resync (up to 5 s).
                // Hold the first full back until the player is tracked. Since
                // №710 the ready client is not BUILT here either: the expensive
                // first full (compute_interest + filtered world pass + full
                // serialize + layer split) is the admission stage's job, at most
                // FIRST_FULL_BUDGET_PER_ROUND builds per round — a storm of 200
                // joins cannot turn one publish round into 200 of those. This
                // path DISCOVERS only: queue the ready client and send it
                // nothing this round; the next rounds find it in `client_layers`
                // once the admission stage has run.
                let initialized = client_layers.read().contains_key(client_id);
                if !initialized {
                    if interest_manager.has_player(*client_id) {
                        first_full_queue.lock().push(*client_id);
                    }
                    return out;
                }

                let filtered = Self::filter_snapshot(&snapshot, interest_manager, *client_id);
                let tick = filtered.tick.value();

                for layer in 0..LAYER_COUNT {
                    let layer_u8 = layer as u8;
                    // Bug №30 (B): the CONFIRMED base — what the client has
                    // reported applied — lives in `client_layers` and moves
                    // only on a report install (or the first full). It is what
                    // the pass knows the client once held. The live diff
                    // anchor, though, is the newest QUEUED send: the client's
                    // applied clock advances with every send it applies, so
                    // until the next report re-anchors, the next delta must
                    // diff from the last send — exactly the ring's newest
                    // entry, content and tick. `base_empty` (the cadence's
                    // "does the client hold anything to heal") stays pinned
                    // to the confirmed copy.
                    let (base_entities, base_projectiles, base_tick, base_empty) = {
                        let layers = client_layers.read();
                        match layers.get(client_id) {
                            Some(entry) => (
                                entry[layer].entities.clone(),
                                entry[layer].projectiles.clone(),
                                entry[layer].base_tick,
                                entry[layer].is_empty(),
                            ),
                            None => continue,
                        }
                    };
                    // Second map read on its own — `client_layers` and
                    // `pending_confirms` are never held in one hand (№79).
                    // While the ring is unconfirmed, its newest entry REPLACES
                    // the confirmed copy as the diff source (same chain the
                    // old per-queue write built, now kept visibly apart from
                    // the confirmed base). `shield` is (oldest unconfirmed
                    // age, ticks since the shield last fired) — None when the
                    // ring is dry: nothing awaits confirmation, nothing stalls.
                    let (base_entities, base_projectiles, last_sent_tick, shield) = {
                        let confirms = pending_confirms.lock();
                        match confirms.get(client_id) {
                            Some(track) => {
                                let ring = &track.pending[layer];
                                match ring.back() {
                                    Some(sent) => (
                                        sent.entities.clone(),
                                        sent.projectiles.clone(),
                                        sent.base_tick,
                                        Some((
                                            tick.saturating_sub(
                                                ring.front()
                                                    .map_or(sent.base_tick, |e| e.base_tick),
                                            ),
                                            tick.saturating_sub(track.last_self_resync[layer]),
                                        )),
                                    ),
                                    None => (base_entities, base_projectiles, base_tick, None),
                                }
                            }
                            None => (base_entities, base_projectiles, base_tick, None),
                        }
                    };
                    let since = tick.saturating_sub(last_sent_tick);

                    // Current content of this layer.
                    let cur_entities: Vec<EntitySnapshot> = filtered
                        .entities
                        .iter()
                        .filter(|e| entity_layer(e.entity_type) == layer_u8)
                        .cloned()
                        .collect();
                    let cur_projectiles: Vec<ProjectileSnapshot> =
                        if layer_u8 == crate::snapshot::LAYER_PROJECTILE {
                            filtered.projectiles.clone()
                        } else {
                            Vec::new()
                        };
                    let content_empty = cur_entities.is_empty() && cur_projectiles.is_empty();

                    // Layer 1 has no fixed rate: it goes out on change, plus resync.
                    let scheduled = since >= LAYER_INTERVAL_TICKS[layer];
                    // Bug №173: resync used to require `!content_empty`, so a layer
                    // that had emptied out could never be resynced again. Its
                    // destroy deltas were then rejected by the client for a base
                    // mismatch and the destroyed entities lingered as ghosts
                    // forever. A resync is exactly the mechanism that prunes an
                    // emptied layer, so allow it whenever the client still holds
                    // base content for the layer — only skip it when both sides
                    // are already empty, where there is nothing to heal.
                    //
                    // Bug №267 (A): an explicit ResyncRequest bypasses the
                    // cadence — the client's base mismatch is a live stall, and
                    // waiting out 15 ticks on a known-stalled layer is the delay
                    // the packet exists to remove. Consumed here, so one request
                    // drives exactly one pass.
                    // Bug №30 (B): the self-shield. A queued send that has
                    // gone unconfirmed for a full span means the client's
                    // applied clock and the send chain have parted ways —
                    // reports lost, or the client stalled mid-chain. Arm the
                    // same resync lane the client's own packet uses; the
                    // cooldown stamp keeps one unreporting client at one full
                    // layer per span instead of a storm.
                    if let Some((oldest_pending_age, since_self_resync)) = shield {
                        if layer_confirm_self_resync_due(
                            layer,
                            oldest_pending_age,
                            since_self_resync,
                        ) {
                            conn.request_resync(layer_u8);
                            // Stamp where it fired; the next chance is a full
                            // span away.
                            if let Some(track) = pending_confirms.lock().get_mut(client_id) {
                                track.last_self_resync[layer] = tick;
                            }
                        }
                    }
                    let requested = conn.take_resync_request(layer);
                    let resync =
                        layer_needs_resync(layer, since, content_empty, base_empty, requested);
                    let on_change = LAYER_INTERVAL_TICKS[layer] == u64::MAX;
                    if !scheduled && !resync && !on_change {
                        continue;
                    }

                    // Compute content hash for cache key. This allows payload reuse
                    // across clients on the same ship with identical layer content.
                    let content_hash = {
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        use std::hash::{Hash, Hasher};
                        for e in &cur_entities {
                            e.entity_id.hash(&mut hasher);
                            e.transform.position.x.to_bits().hash(&mut hasher);
                            e.transform.position.y.to_bits().hash(&mut hasher);
                            e.transform.position.z.to_bits().hash(&mut hasher);
                        }
                        for p in &cur_projectiles {
                            p.entity_id.hash(&mut hasher);
                            p.position.x.to_bits().hash(&mut hasher);
                            p.position.y.to_bits().hash(&mut hasher);
                            p.position.z.to_bits().hash(&mut hasher);
                        }
                        for e in &base_entities {
                            e.entity_id.hash(&mut hasher);
                        }
                        for p in &base_projectiles {
                            p.entity_id.hash(&mut hasher);
                        }
                        hasher.finish()
                    };
                    let key = (layer_u8, resync, last_sent_tick, tick, content_hash);
                    let cached = delta_cache.lock().get(&key).and_then(
                        |(b_e, b_p, c_e, c_p, bytes)| {
                            if *b_e == base_entities
                                && *b_p == base_projectiles
                                && *c_e == cur_entities
                                && *c_p == cur_projectiles
                            {
                                Some(bytes.clone())
                            } else {
                                None
                            }
                        },
                    );
                    if let Some(bytes) = cached {
                        // NC-746-TEMP: probe the fragment share of cached sends too.
                        if layer_u8 == 2 {
                            nc746_probe::live_bytes(bytes.len(), true);
                        }
                        for packet in conn.send_state_delta_bytes(bytes) {
                            out.push((conn.clone(), packet));
                        }
                        // Bug №30 (B): the cached payload was serialized for
                        // another client, but confirmation is per client: what
                        // THIS send owes THIS client is "content as of `tick`".
                        // The unified queue below records it in this client's
                        // ring exactly like the fresh branch. (The compressor's
                        // private ring advance is gone: queued is not applied,
                        // and only the client's report moves a base.)
                        // Bug №274: a cached payload is still a delta that went on
                        // the wire to THIS client. It was left uncounted at first,
                        // which made the send-side figure disagree with the
                        // client's apply count by 4-8x on the layers that hit the
                        // cache most (1 and 2 — the ones whose interest sets differ
                        // per client), and a disagreement like that looks like loss
                        // on the wire when it is really a counter that missed the
                        // fast path. Counted here, next to the send, so both
                        // branches are covered by construction.
                        sent_deltas_by_layer[layer_u8 as usize].fetch_add(1, Ordering::Relaxed);
                    } else {
                        let cached_base_entities = base_entities.clone();
                        let cached_base_projectiles = base_projectiles.clone();
                        let base_snapshot = Snapshot {
                            tick: Tick(last_sent_tick),
                            time: filtered.time,
                            entities: if resync { Vec::new() } else { base_entities },
                            projectiles: if resync { Vec::new() } else { base_projectiles },
                            events: Vec::new(),
                        };
                        let target_snapshot = Snapshot {
                            tick: filtered.tick,
                            time: filtered.time,
                            entities: cur_entities.clone(),
                            projectiles: cur_projectiles.clone(),
                            events: Vec::new(),
                        };
                        let delta = delta_compressor.create_delta(
                            *client_id,
                            layer_u8,
                            &base_snapshot,
                            &target_snapshot,
                        );
                        // Bug №30 (B): nothing on the wire buys nothing in the
                        // ring — the client's base stays exactly where it is.
                        if delta.is_empty() {
                            continue;
                        }
                        // Bug №23/№78: a resync diffed against an empty base is a full
                        // layer replacement. Flag it so the client applies it even after
                        // a lost delta left its base behind, and prunes the old layer.
                        let mut state_delta = delta.to_state_delta();
                        state_delta.is_resync = resync;
                        // NC-746-TEMP: layer-2 composition probe.
                        if layer_u8 == 2 {
                            nc746_probe::live_fresh(&state_delta);
                        }
                        let bytes = match conn.serialize_state_delta(state_delta) {
                            Some(b) => b,
                            None => continue,
                        };
                        // NC-746-TEMP: payload accounting for the fresh branch.
                        if layer_u8 == 2 {
                            nc746_probe::live_bytes(bytes.len(), false);
                        }
                        delta_cache.lock().insert(
                            key,
                            (
                                cached_base_entities,
                                cached_base_projectiles,
                                cur_entities.clone(),
                                cur_projectiles.clone(),
                                bytes.clone(),
                            ),
                        );
                        for packet in conn.send_state_delta_bytes(bytes) {
                            out.push((conn.clone(), packet));
                        }
                        // Bug №274: count deltas actually put on the wire, per
                        // layer. The client's `applied_delta_count_by_layer` is the
                        // matching receive-side number, and the two are only
                        // comparable as a pair. Without the send side, a per-layer
                        // shortfall reads as "the client dropped it" when the truth
                        // can be "the server never sent it" — measured: layer 2 at
                        // 96 deltas where its 1-tick interval implies 900, and
                        // that turned out to be the server diffing an unchanged
                        // layer to nothing, not a client-side loss.
                        sent_deltas_by_layer[layer_u8 as usize].fetch_add(1, Ordering::Relaxed);
                    }
                    // Bug №30 (B): replace the old blind base write ("queued
                    // ⇒ applied") with the pending-confirmation queue: the
                    // send went on the wire, so if the client applies it, it
                    // will hold exactly this content and its report will name
                    // `tick`; that report installs the entry as the confirmed
                    // base. The cap bounds a client that never reports (old
                    // binary worst case): oldest entries give way, and the
                    // self-shield covers the stall.
                    {
                        let mut confirms = pending_confirms.lock();
                        let track = confirms.entry(*client_id).or_default();
                        let ring = &mut track.pending[layer];
                        ring.push_back(LayerBase {
                            entities: cur_entities,
                            projectiles: cur_projectiles,
                            base_tick: tick,
                        });
                        if ring.len() > MAX_PENDING_CONFIRMS {
                            ring.pop_front();
                        }
                    }
                }
                out
            })
            .collect();
        let mut pending_batches = vec![admitted_batch];
        pending_batches.extend(parallel_batches);

        // Bug №266: packets actually put on the wire for this round. Counted
        // before the sends so a round that emits nothing is still visible — an
        // empty round and a missing round look identical from the client side,
        // which is exactly the ambiguity this counter removes.
        snapshot_packets.fetch_add(
            pending_batches.iter().map(|b| b.len() as u64).sum(),
            Ordering::Relaxed,
        );
        for batch in pending_batches {
            for (conn, packet) in batch {
                let _ =
                    Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled, auth).await;
            }
        }
    }

    /// Split a filtered snapshot into per-layer base content.
    fn split_layers(filtered: &Snapshot, tick: u64) -> [LayerBase; LAYER_COUNT] {
        let mut out: [LayerBase; LAYER_COUNT] = Default::default();
        for entity in &filtered.entities {
            let layer = entity_layer(entity.entity_type) as usize;
            if layer < LAYER_COUNT {
                out[layer].entities.push(entity.clone());
            }
        }
        out[crate::snapshot::LAYER_PROJECTILE as usize].projectiles = filtered.projectiles.clone();
        for base in out.iter_mut() {
            // Bug №30 (B): the tick names the state the client holds once it
            // applies THIS full — its first report re-confirms it.
            base.base_tick = tick;
        }
        out
    }

    fn filter_snapshot(
        snapshot: &Snapshot,
        interest_manager: &InterestManager,
        player_id: u32,
    ) -> Snapshot {
        Snapshot {
            tick: snapshot.tick,
            time: snapshot.time,
            entities: snapshot
                .entities
                .iter()
                .filter(|e| interest_manager.should_replicate_entity(player_id, e.entity_id))
                .cloned()
                .collect(),
            projectiles: snapshot
                .projectiles
                .iter()
                .filter(|p| interest_manager.should_replicate_entity(player_id, p.entity_id))
                .cloned()
                .collect(),
            events: snapshot.events.clone(),
        }
    }

    async fn send_heartbeats(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
        auth: &Arc<WireAuth>,
    ) {
        let mut pending_heartbeats: Vec<(Arc<Connection>, OutgoingPacket)> = Vec::new();

        {
            let conns = connections.read();

            for conn in conns.values() {
                if conn.state() == ConnectionState::Connected && conn.should_send_heartbeat() {
                    if let Some(packet) = conn.send_heartbeat() {
                        // Bug №176: the send stamp was never refreshed on the
                        // server, so `last_heartbeat` stayed at construction
                        // time and the measured RTT grew without bound.
                        conn.update_heartbeat();
                        pending_heartbeats.push((conn.clone(), packet));
                    }
                }
            }
        }

        for (conn, packet) in pending_heartbeats {
            let _ = Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled, auth).await;
        }
    }

    /// Bug №215: drain the per-connection reliable ledgers. This runs on the
    /// send-loop cadence (tick rate, ~30 Hz), NOT behind the unchanged-tick
    /// skip: a lost CommandAck must recover even when the world stands still.
    /// The ledger clamps each pass itself (`RETRANSMIT_MAX_PER_PASS`), so a
    /// burst of stale entries throttles across rounds instead of spending a
    /// whole tick writing to one socket.
    async fn send_retransmits(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
        auth: &Arc<WireAuth>,
    ) {
        let mut pending: Vec<(Arc<Connection>, Vec<OutgoingPacket>)> = Vec::new();

        {
            let conns = connections.read();

            for conn in conns.values() {
                if conn.state() != ConnectionState::Connected {
                    continue;
                }
                let retransmits = conn.collect_retransmits();
                if !retransmits.is_empty() {
                    pending.push((conn.clone(), retransmits));
                }
            }
        }

        for (conn, packets) in pending {
            for packet in &packets {
                let _ =
                    Self::send_packet(socket, &conn, packet, bandwidth, limit_enabled, auth).await;
            }
        }
    }

    pub fn add_entity_to_interest(&self, entity: ShipEntity) {
        self.interest_manager.add_ship(entity);
    }

    /// Bug №605: until now this had zero callers, so a hull the sim had
    /// dropped was never taken out of the interest grids. `main` calls it the
    /// moment the simulation no longer knows the ship, passing the interior
    /// that hung off it so the wreck does not keep replicating as orphaned
    /// compartments and stations.
    pub fn remove_entity_from_interest(
        &self,
        entity_id: EntityId,
        compartment_ids: &[EntityId],
        station_ids: &[EntityId],
    ) {
        self.interest_manager
            .retire_ship(entity_id, compartment_ids, station_ids);
    }

    pub fn update_entity_interest(&self, entity: ShipEntity) {
        self.interest_manager.update_ship(entity);
    }

    pub fn add_player(&self, player: PlayerEntity, ship_id: EntityId) {
        self.interest_manager.add_player(player, ship_id);
    }

    pub fn remove_player(&self, player_id: u32, entity_id: EntityId) {
        self.interest_manager.remove_player(player_id, entity_id);
    }

    pub fn update_player(&self, player: PlayerEntity) {
        self.interest_manager.update_player(player);
    }

    /// The entities an event is about, for interest filtering.
    ///
    /// Every variant of `GameEvent` names the entity it happened to, and an
    /// event about a subject the viewer cannot see is exactly the leak Bug №635
    /// describes. The two global variants — team score and match state — name no
    /// entity at all; an empty list means "everyone", which is the only sensible
    /// answer for something that has no position in the world.
    fn event_subjects(event: &GameEvent) -> SmallVec<[EntityId; 2]> {
        let mut subjects: SmallVec<[EntityId; 2]> = SmallVec::new();
        match event {
            GameEvent::ShipHit {
                target,
                projectile,
                hit_compartment,
                ..
            } => {
                subjects.push(*target);
                subjects.push(*projectile);
                if let Some(compartment) = hit_compartment {
                    subjects.push(*compartment);
                }
            }
            GameEvent::ShipSunk { ship, .. } => subjects.push(*ship),
            // Bug №701: the hull's loss is world-visible to exactly the same
            // viewers as the eventual removal is; the crew's own deaths ride
            // `PlayerDied` and need no ship subject of their own.
            GameEvent::ShipSinkingStarted { ship } => subjects.push(*ship),
            GameEvent::ShipRam { rammer, target, .. } => {
                subjects.push(*rammer);
                subjects.push(*target);
            }
            GameEvent::CompartmentFlooded { compartment, .. }
            | GameEvent::CompartmentSealed { compartment }
            | GameEvent::CompartmentBreached { compartment }
            | GameEvent::FireStarted { compartment, .. }
            | GameEvent::FireExtinguished { compartment } => subjects.push(*compartment),
            GameEvent::StationOccupied { station, player }
            | GameEvent::StationVacated { station, player } => {
                subjects.push(*station);
                subjects.push(*player);
            }
            GameEvent::PlayerSpawned { player, ship, .. } => {
                subjects.push(*player);
                subjects.push(*ship);
            }
            GameEvent::PlayerDied { player, killer } => {
                subjects.push(*player);
                if let Some(killer) = killer {
                    subjects.push(*killer);
                }
            }
            GameEvent::ProjectileFired { projectile, .. }
            | GameEvent::ProjectileImpact { projectile, .. } => subjects.push(*projectile),
            GameEvent::BulkheadStateChanged { bulkhead, .. } => subjects.push(*bulkhead),
            GameEvent::PumpStateChanged { pump, .. } => subjects.push(*pump),
            GameEvent::TeamScoreChanged { .. } | GameEvent::MatchStateChanged { .. } => {}
            GameEvent::ChatReceived {
                player, channel, ..
            } => {
                // Bug №634: "All" names no subject — everyone on the server is
                // in the chat. The narrower channels are about the sender, so
                // interest decides: the same-ship viewers can always see the
                // player, far-away viewers cannot. (Team/Whisper never reach
                // the wire as broadcasts — the command path refuses them.)
                match channel {
                    ChatChannel::All => {}
                    _ => subjects.push(*player),
                }
            }
        }
        subjects
    }

    /// Fan an event out to the connections that can actually see what it is
    /// about.
    ///
    /// Bug №635: this used to send to every Connected connection while
    /// snapshots were built per interest. Two costs, both real. The fan-out ran
    /// as O(connections x events) whatever a client could physically see, and
    /// the event itself leaked the state of ships the snapshot deliberately
    /// omitted — a hull at the far end of the map was invisible to the client
    /// and fully described by its events.
    ///
    /// The rule now is the one the snapshot path already keeps: a client is told
    /// about an entity exactly when that entity would be in its snapshot.
    /// `should_replicate_subject` is permissive on purpose (see there) so an
    /// event cannot be silenced by an id the manager has never heard of, and an
    /// event with no subject goes to everyone.
    ///
    /// A viewer that is not tracked yet gets nothing, by the same rule that
    /// holds its first full state back (Bug №164): an event describes a world,
    /// and a client with no world yet should not be handed pieces of one.
    ///
    /// Whether a viewer should receive `event` at all.
    ///
    /// Kept separate from the fan-out so the policy is testable without a
    /// socket: `broadcast_event` decides *who* to write to, this decides *what*
    /// each of them is allowed to learn. One visible subject is enough — a
    /// player who can see the ship that was hit needs the hit even if the shell
    /// that made it has already flown out of range, and a player who can see
    /// the shell needs the impact. Requiring *all* subjects would fail both.
    fn event_reaches(interest: &InterestManager, viewer_player_id: u32, event: &GameEvent) -> bool {
        if !interest.has_player(viewer_player_id) {
            return false;
        }
        let subjects = Self::event_subjects(event);
        if subjects.is_empty() {
            // Nothing is named: match and team state describes the world, not
            // an entity, so every tracked viewer hears it (Bug №635).
            return true;
        }
        subjects
            .iter()
            .any(|s| interest.should_replicate_subject(viewer_player_id, *s))
    }

    pub fn broadcast_event(&self, event: GameEvent) {
        // Bug №172: queue instead of spawning. A mass event to 256 conns is
        // now N queue pushes drained by the single writer task, not a task per
        // connection racing each other onto the wire.
        let conns: Vec<(u32, Arc<Connection>)> = {
            let conns = self.connections.read();
            conns
                .iter()
                .filter(|(_, c)| c.state() == ConnectionState::Connected)
                .map(|(client_id, conn)| (*client_id, conn.clone()))
                .collect()
        };
        for (client_id, conn) in conns {
            if !Self::event_reaches(&self.interest_manager, client_id, &event) {
                continue;
            }
            let packets = conn.send_event(EventPacket {
                events: vec![event.clone()],
            });
            if !packets.is_empty() {
                self.enqueue(conn, packets);
            }
        }
    }

    pub fn send_command_ack(
        &self,
        connection_id: u32,
        command_id: u64,
        success: bool,
        error: Option<String>,
    ) {
        if let Some(conn) = self.get_connection(connection_id) {
            let packets = conn.send_command_ack(command_id, success, error);
            if !packets.is_empty() {
                self.enqueue(conn, packets);
            }
        }
    }

    pub fn send_input_ack(&self, connection_id: u32, ack: InputAckPacket) {
        if let Some(conn) = self.get_connection(connection_id) {
            let packets = conn.send_input_ack(ack);
            if !packets.is_empty() {
                self.enqueue(conn, packets);
            }
        }
    }

    /// Disconnect a client. Returns true if this call actually removed the
    /// connection — callers emit disconnect events only then, so running this
    /// against an already-removed id (or racing the timeout sweep) cannot
    /// double-report (bug №32, №80).
    pub fn disconnect_client(&self, connection_id: u32, reason: DisconnectReason) -> bool {
        let Some(conn) = self.get_connection(connection_id) else {
            return false;
        };

        // Bug №172: queue the farewell instead of spawning a task for it.
        let header = PacketHeader::new(
            PacketType::Disconnect,
            ChannelType::ReliableOrdered,
            conn.next_sequence(),
            0,
            0,
            0,
        );
        if let Some(out) = OutgoingPacket::new(header, DisconnectPacket { reason }) {
            self.enqueue(conn.clone(), vec![out]);
        }

        if Self::remove_connection(
            &self.connection_by_addr,
            &self.connections,
            &self.pending_confirms,
            &self.client_layers,
            connection_id,
            conn.addr,
        ) {
            emit_event(
                &self.event_sender,
                &self.dropped_events,
                ServerEvent::ClientDisconnected {
                    connection_id,
                    reason,
                },
            );
            true
        } else {
            false
        }
    }

    pub fn disconnect_all(&self, reason: DisconnectReason) {
        let ids: Vec<u32> = self.connections.read().keys().copied().collect();
        for id in ids {
            self.disconnect_client(id, reason);
        }
    }

    pub fn check_timeouts(&self) {
        let mut timed_out = Vec::new();
        {
            let conns = self.connections.read();
            for (id, conn) in conns.iter() {
                if conn.is_timed_out() {
                    timed_out.push(*id);
                }
            }
        }

        for id in timed_out {
            // Bug №80: emit ClientTimeout only if we actually removed the
            // connection — the sweep loop or a manual disconnect may have
            // raced us, and double-emission breaks the №32 uniqueness contract.
            if self.disconnect_client(id, DisconnectReason::Timeout) {
                emit_event(
                    &self.event_sender,
                    &self.dropped_events,
                    ServerEvent::ClientTimeout { connection_id: id },
                );
            }
        }
    }

    pub fn get_event_receiver(&self) -> Option<mpsc::Receiver<ServerEvent>> {
        self.event_receiver.lock().take()
    }

    /// Events dropped because the consumer fell behind (Bug №613).
    ///
    /// Exists so a bounded channel is not a new silent failure: the whole point
    /// of bounding the queue is that something has to be dropped, and a drop
    /// nobody can see has simply traded an OOM for a mystery.
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    /// Outbox entries dropped for the same reason (Bug №613).
    pub fn dropped_outbox(&self) -> u64 {
        self.dropped_outbox.load(Ordering::Relaxed)
    }

    pub fn bandwidth_stats(&self) -> BandwidthStats {
        self.bandwidth_tracker.stats()
    }

    /// Snapshot rounds run and packets emitted, for the publication-rate metric.
    ///
    /// Bug №266. Monotonic totals, so the caller differences them over a window
    /// to get a rate — an absolute count says nothing about rate.
    pub fn snapshot_totals(&self) -> (u64, u64) {
        (
            self.snapshot_rounds.load(Ordering::Relaxed),
            self.snapshot_packets.load(Ordering::Relaxed),
        )
    }

    /// Bug №274: deltas actually sent, per layer. Read together with the
    /// client's `applied_delta_count_by_layer`: sent-vs-applied separates "the
    /// server had nothing new to send" from "the client dropped it".
    pub fn sent_deltas_by_layer(&self) -> Vec<u64> {
        self.sent_deltas_by_layer
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect()
    }

    /// Bug №742: the applied-lag gauge, per connection, ordered by client id.
    ///
    /// `tick - reported_applied[layer]` — how far behind the server's clock
    /// each client's last-mounted state actually is, in ticks, summed over
    /// the wire both ways and the client's apply pipeline. This is №30's
    /// confirmation chain turned into an instrument: before it, the only
    /// latency numbers were the heartbeat RTT samples (one per second, no
    /// per-layer picture). A `None` row means the client has not reported
    /// yet — silence is not zero lag.
    pub fn apply_lag_stats(&self) -> Vec<(u32, Option<[u64; LAYER_COUNT]>)> {
        let now = self.current_tick.lock().value();
        applied_lag_rows(now, &self.connections)
    }

    pub fn interest_manager(&self) -> Arc<InterestManager> {
        self.interest_manager.clone()
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn interest_stats(&self) -> crate::interest::InterestStats {
        self.interest_manager.stats()
    }
}

// NC-746-TEMP: layer-2 wire composition probe. Added to measure what a
// layer-2 delta actually carries (records per delta, which fields are
// Some, postcard bytes per record and per player_data, share of payloads
// above the fragmentation threshold). REVERT ENTIRELY after the run.
pub mod nc746_probe {
    use rfs_core::packet::{encode, StateDeltaPacket};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::Duration;
    use tracing::info;

    /// 1400 (MAX_PACKET_SIZE) - 24 (HEADER_SIZE) - 24 (AUTH_WIRE_LEN):
    /// payloads above this are fragmented into two datagrams.
    const FRAG_THRESHOLD: usize = 1352;

    struct Probe {
        deltas_fresh: AtomicU64,
        deltas_cached: AtomicU64,
        bytes_sum: AtomicU64,
        bytes_over: AtomicU64,
        recs: AtomicU64,
        rec_bytes: AtomicU64,
        rec_transform: AtomicU64,
        rec_velocity: AtomicU64,
        rec_angular: AtomicU64,
        rec_health: AtomicU64,
        rec_flags: AtomicU64,
        rec_ship: AtomicU64,
        rec_station: AtomicU64,
        rec_pdata: AtomicU64,
        rec_pdata_bytes: AtomicU64,
        rec_name_bytes: AtomicU64,
        resyncs: AtomicU64,
    }

    impl Probe {
        const fn zeroed() -> Self {
            Self {
                deltas_fresh: AtomicU64::new(0),
                deltas_cached: AtomicU64::new(0),
                bytes_sum: AtomicU64::new(0),
                bytes_over: AtomicU64::new(0),
                recs: AtomicU64::new(0),
                rec_bytes: AtomicU64::new(0),
                rec_transform: AtomicU64::new(0),
                rec_velocity: AtomicU64::new(0),
                rec_angular: AtomicU64::new(0),
                rec_health: AtomicU64::new(0),
                rec_flags: AtomicU64::new(0),
                rec_ship: AtomicU64::new(0),
                rec_station: AtomicU64::new(0),
                rec_pdata: AtomicU64::new(0),
                rec_pdata_bytes: AtomicU64::new(0),
                rec_name_bytes: AtomicU64::new(0),
                resyncs: AtomicU64::new(0),
            }
        }
    }

    static PROBE: OnceLock<Probe> = OnceLock::new();

    pub fn probe() -> &'static Probe {
        PROBE.get_or_init(|| {
            std::thread::spawn(|| {
                loop {
                    std::thread::sleep(Duration::from_secs(5));
                    let Some(p) = PROBE.get() else { continue };
                    let fresh = p.deltas_fresh.swap(0, Ordering::Relaxed);
                    let cached = p.deltas_cached.swap(0, Ordering::Relaxed);
                    let bytes = p.bytes_sum.swap(0, Ordering::Relaxed);
                    let over = p.bytes_over.swap(0, Ordering::Relaxed);
                    let recs = p.recs.swap(0, Ordering::Relaxed);
                    let rec_bytes = p.rec_bytes.swap(0, Ordering::Relaxed);
                    let tr = p.rec_transform.swap(0, Ordering::Relaxed);
                    let v = p.rec_velocity.swap(0, Ordering::Relaxed);
                    let a = p.rec_angular.swap(0, Ordering::Relaxed);
                    let h = p.rec_health.swap(0, Ordering::Relaxed);
                    let f = p.rec_flags.swap(0, Ordering::Relaxed);
                    let sh = p.rec_ship.swap(0, Ordering::Relaxed);
                    let st = p.rec_station.swap(0, Ordering::Relaxed);
                    let pd = p.rec_pdata.swap(0, Ordering::Relaxed);
                    let pdb = p.rec_pdata_bytes.swap(0, Ordering::Relaxed);
                    let nb = p.rec_name_bytes.swap(0, Ordering::Relaxed);
                    let rs = p.resyncs.swap(0, Ordering::Relaxed);
                    let total_d = fresh + cached;
                    let avg_payload = if total_d > 0 { bytes as f64 / total_d as f64 } else { 0.0 };
                    let over_pct = if total_d > 0 { 100.0 * over as f64 / total_d as f64 } else { 0.0 };
                    let avg_rec = if recs > 0 { rec_bytes as f64 / recs as f64 } else { 0.0 };
                    let pca = |n: u64| if recs > 0 { 100.0 * n as f64 / recs as f64 } else { 0.0 };
                    let avg_pd = if pd > 0 { pdb as f64 / pd as f64 } else { 0.0 };
                    let avg_name = if pd > 0 { nb as f64 / pd as f64 } else { 0.0 };
                    info!(
                        "NC746 window: l2 fresh={} cached={} avg_payload={:.0}B fragments={:.1}% recs={} avg_rec={:.1}B \
                         tr%={:.0} vel%={:.0} ang%={:.0} h%={:.0} f%={:.0} ship%={:.0} st%={:.0} pdata%={:.0} \
                         avg_pdb={:.0}B avg_name={:.1} resyncs={}",
                        fresh, cached, avg_payload, over_pct, recs, avg_rec,
                        pca(tr), pca(v), pca(a), pca(h), pca(f), pca(sh), pca(st), pca(pd),
                        avg_pd, avg_name, rs
                    );
                }
            });
            Probe::zeroed()
        })
    }

    /// Fresh (non-cached) delta: full composition. NC-746-TEMP.
    pub fn live_fresh(d: &StateDeltaPacket) {
        let p = probe();
        p.deltas_fresh.fetch_add(1, Ordering::Relaxed);
        if d.is_resync {
            p.resyncs.fetch_add(1, Ordering::Relaxed);
        }
        for u in &d.updated {
            p.recs.fetch_add(1, Ordering::Relaxed);
            if let Ok(b) = encode(u) {
                p.rec_bytes.fetch_add(b.len() as u64, Ordering::Relaxed);
            }
            if u.transform.is_some() {
                p.rec_transform.fetch_add(1, Ordering::Relaxed);
            }
            if u.velocity.is_some() {
                p.rec_velocity.fetch_add(1, Ordering::Relaxed);
            }
            if u.angular_velocity.is_some() {
                p.rec_angular.fetch_add(1, Ordering::Relaxed);
            }
            if u.health.is_some() {
                p.rec_health.fetch_add(1, Ordering::Relaxed);
            }
            if u.flags.is_some() {
                p.rec_flags.fetch_add(1, Ordering::Relaxed);
            }
            if u.ship_data.is_some() {
                p.rec_ship.fetch_add(1, Ordering::Relaxed);
            }
            if u.station_data.is_some() {
                p.rec_station.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(pd) = &u.player_data {
                p.rec_pdata.fetch_add(1, Ordering::Relaxed);
                if let Ok(pb) = encode(pd) {
                    p.rec_pdata_bytes.fetch_add(pb.len() as u64, Ordering::Relaxed);
                }
                p.rec_name_bytes.fetch_add(pd.name.len() as u64, Ordering::Relaxed);
            }
        }
    }

    /// Payload-bytes accounting for both branches (fresh and cache-hit). NC-746-TEMP.
    pub fn live_bytes(bytes_len: usize, cached: bool) {
        let p = probe();
        p.bytes_sum.fetch_add(bytes_len as u64, Ordering::Relaxed);
        if bytes_len > FRAG_THRESHOLD {
            p.bytes_over.fetch_add(1, Ordering::Relaxed);
        }
        if cached {
            p.deltas_cached.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod intro_timeout_tests {
    use super::*;

    fn conn(id: u32, addr: SocketAddr, age: Option<Duration>) -> Connection {
        let c = match age {
            Some(a) => Connection::new_backdated(id, addr, ConnectionConfig::default(), a),
            None => Connection::new(id, addr, ConnectionConfig::default()),
        };
        c.set_state(ConnectionState::Connected);
        c
    }

    /// Bug №219: a Connected client whose game-layer player was never
    /// registered receives no state bytes at all — the snapshot gate waits
    /// for `has_player` forever. The sweep must end that session after
    /// `PLAYER_WAIT_TIMEOUT` and report it, while fresh connections and
    /// registered ones stay untouched.
    #[test]
    fn a_userless_connected_client_is_reaped_after_the_wait() {
        let connections = Arc::new(RwLock::new(HashMap::new()));
        let by_addr = Arc::new(RwLock::new(HashMap::new()));
        // Bug №30 (B): the sweep reaps pending rings, not compressor state.
        let pending: Arc<Mutex<HashMap<u32, ClientLayerTrack>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let layers = Arc::new(RwLock::new(HashMap::new()));
        let pending_challenges = Arc::new(RwLock::new(HashMap::new()));
        let (tx, mut rx) = mpsc::channel(64);
        let dropped = AtomicU64::new(0);

        let fresh_addr: SocketAddr = "127.0.0.1:40001".parse().unwrap();
        let stale_addr: SocketAddr = "127.0.0.1:40002".parse().unwrap();
        let registered_addr: SocketAddr = "127.0.0.1:40003".parse().unwrap();

        let fresh = Arc::new(conn(9, fresh_addr, None));
        let stale = Arc::new(conn(
            5,
            stale_addr,
            Some(PLAYER_WAIT_TIMEOUT + Duration::from_secs(1)),
        ));
        let registered = Arc::new(conn(
            7,
            registered_addr,
            Some(PLAYER_WAIT_TIMEOUT + Duration::from_secs(1)),
        ));
        // The registered one has its layer base initialized: even though it
        // is old, the gate opened long ago, so it is not a candidate.
        layers.write().insert(7, Default::default());

        connections.write().insert(9, fresh.clone());
        connections.write().insert(5, stale.clone());
        connections.write().insert(7, registered.clone());
        by_addr.write().insert(fresh_addr, 9);
        by_addr.write().insert(stale_addr, 5);
        by_addr.write().insert(registered_addr, 7);

        NetServer::sweep_timeouts(
            &connections,
            &by_addr,
            &pending,
            &layers,
            &tx,
            &dropped,
            &pending_challenges,
        );

        assert_eq!(
            dropped.load(Ordering::Relaxed),
            0,
            "queue of 64 must hold all events"
        );

        // The unregistered stale one is gone; the other two remain.
        assert!(
            !connections.read().contains_key(&5),
            "stale userless conn reaped"
        );
        assert!(
            connections.read().contains_key(&9),
            "a fresh waiting conn stays"
        );
        assert!(
            connections.read().contains_key(&7),
            "a registered conn stays"
        );

        // The report names the cause, then the formal pair of lifecycle
        // events — three events for the reaped connection exactly.
        let mut kinds: Vec<String> = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            match ev {
                ServerEvent::Error {
                    connection_id: Some(5),
                    error,
                } => {
                    assert!(
                        error.contains("player never registered"),
                        "the error must name the cause, got: {error}"
                    );
                    kinds.push("Error".into());
                }
                ServerEvent::ClientDisconnected {
                    connection_id: 5, ..
                } => {
                    kinds.push("Disconnected".into());
                }
                ServerEvent::ClientTimeout { connection_id: 5 } => {
                    kinds.push("Timeout".into());
                }
                _ => {} // events for 9/7 would also be a failure below
            }
        }
        assert_eq!(
            kinds,
            ["Error", "Disconnected", "Timeout"],
            "exactly the reaped session's lifecycle must be reported, got {kinds:?}"
        );
    }
}

#[cfg(test)]
mod resync_request_tests {
    //! Bug №267 (A): the client's resync ask must reach the layer, and it must
    //! bypass the cadence — the whole point of the packet is that a stalled
    //! layer is healed now, not in 15 ticks.

    use super::*;

    fn conn() -> Connection {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        Connection::new(1, addr, ConnectionConfig::default())
    }

    #[test]
    fn an_explicit_request_resyncs_immediately_regardless_of_the_cadence() {
        // The cadence has not elapsed at all: without the request the layer
        // would sit and wait.
        assert!(
            !layer_needs_resync(0, 0, false, false, false),
            "without a request the cadence decides"
        );
        assert!(
            layer_needs_resync(0, 0, false, false, true),
            "an explicit request must resync the layer at once"
        );
    }

    #[test]
    fn the_cadence_still_heals_an_emptied_layer_that_holds_a_base() {
        // Bug №173's path: the content is gone, the client still holds a base.
        assert!(layer_needs_resync(
            0,
            LAYER_RESYNC_TICKS[0],
            true,
            false,
            false
        ));
        // Nothing to heal: both sides are empty.
        assert!(!layer_needs_resync(
            0,
            LAYER_RESYNC_TICKS[0],
            true,
            true,
            false
        ));
        // Not due yet.
        assert!(!layer_needs_resync(0, 0, false, false, false));
    }

    #[test]
    fn an_incoming_request_arms_the_asked_layer() {
        let conn = conn();
        let payload = rfs_core::packet::encode(&ResyncRequestPacket { layer: 2 }).unwrap();
        assert!(apply_resync_request(&conn, &payload));
        assert!(conn.take_resync_request(2), "layer 2 must be armed");
        assert!(!conn.take_resync_request(0), "no other layer may be armed");
    }

    #[test]
    fn a_request_for_a_layer_outside_the_range_is_refused() {
        let conn = conn();
        let payload = rfs_core::packet::encode(&ResyncRequestPacket {
            layer: LAYER_COUNT as u8,
        })
        .unwrap();
        assert!(
            !apply_resync_request(&conn, &payload),
            "a layer index from the wire must be bounds-checked"
        );
        for layer in 0..LAYER_COUNT {
            assert!(!conn.take_resync_request(layer));
        }
    }

    #[test]
    fn a_malformed_request_arms_nothing() {
        let conn = conn();
        assert!(!apply_resync_request(&conn, &[0xff, 0xff, 0xff, 0xff]));
        for layer in 0..LAYER_COUNT {
            assert!(!conn.take_resync_request(layer));
        }
    }
}

#[cfg(test)]
mod layer_confirm_tests {
    //! Bug №30 (B): the applied-report path — session gate, merge semantics,
    //! install from the pending ring, the self-shield rule and ring hygiene.

    use super::*;

    fn conn() -> Connection {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        Connection::new(1, addr, ConnectionConfig::default())
    }

    fn report(applied: [u32; 4], session: u128) -> Vec<u8> {
        rfs_core::packet::encode(&LayerSyncPacket { applied, session }).unwrap()
    }

    fn ring(entry_ticks: &[u64]) -> VecDeque<LayerBase> {
        entry_ticks
            .iter()
            .map(|&t| LayerBase {
                entities: Vec::new(),
                projectiles: Vec::new(),
                base_tick: t,
            })
            .collect()
    }

    fn pending_map() -> Arc<Mutex<HashMap<u32, ClientLayerTrack>>> {
        Arc::new(Mutex::new(HashMap::new()))
    }

    fn layers_map() -> Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>> {
        Arc::new(RwLock::new(HashMap::new()))
    }

    #[test]
    fn a_report_gates_on_the_session_echo() {
        let conn = conn();
        let session = conn.session();
        assert!(session != 0, "test prep: the fixture has a session");
        // Bug №30 (B): the report becomes the server's confirmed diff base,
        // so it gates exactly like Input — session echo or nothing.
        let typed = PacketType::LayerSync as u8;
        let good = report([5, 5, 5, 5], session);
        assert!(
            NetServer::session_gate(&conn, typed, &good),
            "a live session echo passes the gate"
        );
        let forged = report([5, 5, 5, 5], session.wrapping_add(1));
        assert!(
            !NetServer::session_gate(&conn, typed, &forged),
            "a wrong session echo must not reach the install"
        );
        assert!(
            !NetServer::session_gate(&conn, typed, &[0xff, 0xff, 0xff]),
            "an unroutable payload is not an identity either"
        );
    }

    #[test]
    fn a_valid_report_merges_max_per_layer() {
        let conn = conn();
        let pending = pending_map();
        let layers = layers_map();
        let session = conn.session();
        assert!(apply_layer_sync_report(
            &conn, &pending, &layers, 1, &report([7, 3, 0, 0], session)
        ));
        assert_eq!(conn.reported_applied(0), 7);
        assert_eq!(conn.reported_applied(1), 3);
        // A duplicate or a reordered stale report is ignored, never rewound.
        assert!(apply_layer_sync_report(
            &conn, &pending, &layers, 1, &report([2, 0, 9, 0], session)
        ));
        assert_eq!(conn.reported_applied(0), 7, "merge is max");
        assert_eq!(conn.reported_applied(1), 3, "merge is max");
        assert_eq!(conn.reported_applied(2), 9);
    }

    #[test]
    fn a_malformed_report_merges_nothing() {
        let conn = conn();
        assert!(!apply_layer_sync_report(
            &conn,
            &pending_map(),
            &layers_map(),
            1,
            &[0xff, 0xff]
        ));
        for layer in 0..LAYER_COUNT {
            assert_eq!(conn.reported_applied(layer), 0);
        }
    }

    #[test]
    fn an_exact_report_installs_the_confirmed_base_and_drains_the_ring() {
        let conn = conn();
        let pending = pending_map();
        pending.lock().entry(1).or_default().pending[2] = ring(&[8, 10, 12]);
        let layers = layers_map();
        let mut row: [LayerBase; LAYER_COUNT] = Default::default();
        row[2].base_tick = 6;
        layers.write().insert(1, row);

        // applied = 10: the middle entry is proof the client holds it.
        assert!(apply_layer_sync_report(
            &conn,
            &pending,
            &layers,
            1,
            &report([0, 0, 10, 0], conn.session())
        ));

        let now: [LayerBase; LAYER_COUNT] = layers.read().get(&1).cloned().unwrap();
        assert_eq!(
            now[2].base_tick, 10,
            "the entry the client applied becomes the confirmed base"
        );
        let left: Vec<u64> = pending.lock().get(&1).unwrap().pending[2]
            .iter()
            .map(|e| e.base_tick)
            .collect();
        assert_eq!(
            left,
            vec![12],
            "consumed and superseded entries are drained"
        );
    }

    #[test]
    fn a_report_without_an_exact_match_moves_nothing() {
        let conn = conn();
        let pending = pending_map();
        pending.lock().entry(1).or_default().pending[0] = ring(&[10, 12]);
        let layers = layers_map();
        let mut row: [LayerBase; LAYER_COUNT] = Default::default();
        row[0].base_tick = 4;
        layers.write().insert(1, row);

        // applied = 11: between entries — nothing installs, but the entry
        // the client has already moved past is consumed.
        assert!(apply_layer_sync_report(
            &conn,
            &pending,
            &layers,
            1,
            &report([11, 0, 0, 0], conn.session())
        ));
        let now: [LayerBase; LAYER_COUNT] = layers.read().get(&1).cloned().unwrap();
        assert_eq!(now[0].base_tick, 4, "no guess, no install");
        let left: Vec<u64> = pending.lock().get(&1).unwrap().pending[0]
            .iter()
            .map(|e| e.base_tick)
            .collect();
        assert_eq!(left, vec![12], "entries below the applied tick are drained");
    }

    #[test]
    fn the_self_shield_arms_one_span_at_a_time() {
        // Due: a stale unconfirmed send and a cold shield.
        assert!(layer_confirm_self_resync_due(1, LAYER_CONFIRM_RESYNC_TICKS[1], u64::MAX));
        // Cooldown hot: wait for the span.
        assert!(!layer_confirm_self_resync_due(
            1,
            LAYER_CONFIRM_RESYNC_TICKS[1],
            LAYER_CONFIRM_RESYNC_TICKS[1] - 1
        ));
        // Nothing pending: the chain is dry; the scheduled cadence owns it.
        assert!(!layer_confirm_self_resync_due(1, 0, u64::MAX));
    }

    #[test]
    fn the_ring_caps_at_the_confirms_limit() {
        let mut track = ClientLayerTrack::default();
        let ring = &mut track.pending[3];
        for t in 1..=6u64 {
            ring.push_back(LayerBase {
                entities: Vec::new(),
                projectiles: Vec::new(),
                base_tick: t,
            });
            if ring.len() > MAX_PENDING_CONFIRMS {
                ring.pop_front();
            }
        }
        let ticks: Vec<u64> = ring.iter().map(|e| e.base_tick).collect();
        assert_eq!(ticks, vec![3, 4, 5, 6], "the oldest give way to the newest");
    }

    #[test]
    fn the_lag_rows_measure_only_clients_that_reported() {
        // Bug №742: two connections — №1 has reported [6,7,8,0] at tick 40
        // (its fresh state sits 34/33/32 ticks behind; the 0 is a real
        // applied-tick-0 datapoint, 40 behind), №2 silent.
        let connections: Arc<RwLock<HashMap<u32, Arc<Connection>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let first = Arc::new(conn());
        first.report_applied([6, 7, 8, 0]);
        connections.write().insert(1, first);
        connections.write().insert(2, Arc::new(conn()));
        // A client that never reported on an empty map — the second read.
        connections.write().insert(3, Arc::new(conn()));

        let rows = applied_lag_rows(40, &connections);
        assert_eq!(rows.len(), 3, "one row per live connection");
        assert_eq!(rows[0].0, 1, "rows are ordered by client id, not map order");
        assert_eq!(rows[0].1, Some([34, 33, 32, 40]));
        assert_eq!(
            rows[1].1, None,
            "no report means no measurement, never zero lag"
        );
        assert_eq!(rows[2].0, 3);

        let empty: Arc<RwLock<HashMap<u32, Arc<Connection>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        assert!(applied_lag_rows(40, &empty).is_empty());
    }
}

#[cfg(test)]
mod bounded_queue_tests {
    use super::*;

    /// Bug №613: the event queue was an `unbounded_channel`, so a producer
    /// outrunning the tick loop grew memory without limit — the failure mode is
    /// an OOM, not a slowdown. Bounding it introduces a new one: a full queue
    /// drops an event. That drop is counted here, because a bounded queue whose
    /// drops are invisible has merely traded an OOM for a mystery.
    #[test]
    fn a_full_event_queue_drops_and_counts() {
        let dropped = AtomicU64::new(0);
        let (tx, rx) = mpsc::channel(2);

        // Two fit; the third has nowhere to go.
        for id in 1..=3 {
            emit_event(
                &tx,
                &dropped,
                ServerEvent::Error {
                    connection_id: Some(id),
                    error: format!("flood {id}"),
                },
            );
        }
        assert_eq!(
            dropped.load(Ordering::Relaxed),
            1,
            "the event the full queue dropped was not counted"
        );

        // A closed channel is NOT counted: nothing is waiting to read it any
        // more, so the number would only grow for a server already going down.
        drop(rx);
        emit_event(
            &tx,
            &dropped,
            ServerEvent::Error {
                connection_id: Some(4),
                error: "after close".into(),
            },
        );
        assert_eq!(
            dropped.load(Ordering::Relaxed),
            1,
            "a closed channel must not inflate the backpressure counter"
        );
    }

    /// The two queues are bounded for the same reason, so the cap has to be a
    /// number rather than a default: a test that cannot fill the queue cannot
    /// tell a bound from no bound.
    #[test]
    fn the_queues_really_are_bounded() {
        let (tx, _rx) = mpsc::channel::<u8>(1);
        assert!(tx.try_send(1).is_ok());
        assert!(
            matches!(tx.try_send(2), Err(mpsc::error::TrySendError::Full(_))),
            "a channel of capacity 1 accepted a second item, so it is not bounded"
        );
    }
}

/// Bug №635: `broadcast_event` sent every event to every connection while
/// snapshots were built per interest.
///
/// These test the policy, not the fan-out. `event_reaches` is the whole decision
/// and is deliberately callable without a socket; the loop that applies it is
/// three lines of plumbing around it.
#[cfg(test)]
mod first_full_queue_tests {
    use super::*;

    // Bug №710: the queue's own contract — dedupe, cap, FIFO admission with
    // a budget, and the pruning of dead ids — tested on the bare type. The
    // admission stage that consumes it is tested in
    // `first_full_admission_tests`.

    #[test]
    fn discovery_of_one_client_queues_it_exactly_once() {
        let mut q = FirstFullQueue::default();
        assert!(q.push(7));
        // The same client is discovered again next round while still waiting:
        // a second copy would double its place in the line.
        assert!(!q.push(7));
        assert_eq!(q.waiting_len(), 1);
        assert_eq!(q.admit(1, |_| true), vec![7]);
        assert_eq!(q.waiting_len(), 0);
    }

    #[test]
    fn the_queue_is_finite_even_under_a_never_ending_storm() {
        let mut q = FirstFullQueue::default();
        for id in 0..FirstFullQueue::CAP as u32 {
            assert!(q.push(id));
        }
        // The 257th simultaneously waiting client is refused instead of
        // growing the line: it will be re-discovered the next round, when
        // earlier clients have been admitted and the queue has room again.
        assert!(!q.push(FirstFullQueue::CAP as u32));
        assert_eq!(q.waiting_len(), FirstFullQueue::CAP);
    }

    #[test]
    fn admission_respects_the_budget_and_keeps_the_line() {
        let mut q = FirstFullQueue::default();
        for id in 1..=6u32 {
            q.push(id);
        }
        assert_eq!(q.admit(4, |_| true), vec![1, 2, 3, 4]);
        assert_eq!(q.waiting_len(), 2);
        // FIFO order is preserved for the rest of the line.
        assert_eq!(q.admit(4, |_| true), vec![5, 6]);
        assert_eq!(q.waiting_len(), 0);
    }

    #[test]
    fn a_dead_queued_client_is_pruned_without_costing_a_slot() {
        let mut q = FirstFullQueue::default();
        for id in 1..=4u32 {
            q.push(id);
        }
        // Client 1 disconnected while queued. It must neither be admitted
        // (an unready base is the №164 shape) nor consume a budget slot —
        // the budget belongs to the clients who can actually start.
        let alive = |id: u32| id != 1;
        assert_eq!(q.admit(2, alive), vec![2, 3]);
        assert_eq!(q.waiting_len(), 1);
        assert_eq!(q.admit(2, alive), vec![4]);
        assert_eq!(q.waiting_len(), 0);
    }
}

#[cfg(test)]
mod first_full_admission_tests {
    use super::*;
    use rfs_core::entity::PlayerPosture;
    use rfs_core::math::{Transform, Vec3f};

    fn storm_ship(id: u64) -> ShipEntity {
        ShipEntity {
            entity_id: EntityId::new(id),
            ship_class_id: 0,
            transform: Transform::from_position(Vec3f::new(0.0, 0.0, 0.0)),
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 100.0,
            max_health: 100.0,
            flags: EntityFlags::empty(),
            fuel: 50.0,
            max_fuel: 50.0,
            speed: 0.0,
            max_speed: 10.0,
            heading: 0.0,
            rudder_angle: 0.0,
            throttle: 0.0,
            last_hit: None,
            compartments: Vec::new(),
            stations: Vec::new(),
            team: 0,
        }
    }

    fn storm_player(entity_id: u64, player_id: u32, ship_id: u64) -> PlayerEntity {
        PlayerEntity {
            entity_id: EntityId::new(entity_id),
            player_id,
            name: format!("storm-{player_id}"),
            team: 0,
            transform: Transform::from_position(Vec3f::new(0.0, 0.0, 0.0)),
            velocity: Vec3f::ZERO,
            posture: PlayerPosture::Standing,
            health: 100.0,
            max_health: 100.0,
            stamina: 100.0,
            max_stamina: 100.0,
            current_station: None,
            current_ship: Some(EntityId::new(ship_id)),
            input_sequence: 0,
            last_acknowledged_tick: 0,
        }
    }

    /// An empty world: nothing is in it, yet the first full still has to be
    /// BUILT for the client to start receiving deltas against a real base.
    fn snapshot() -> Snapshot {
        Snapshot {
            tick: Tick(1),
            time: 0.0,
            entities: Vec::new(),
            projectiles: Vec::new(),
            events: Vec::new(),
        }
    }

    fn connect(id: u32) -> Connection {
        let addr: SocketAddr = format!("127.0.0.1:{}", 41000 + id)
            .parse()
            .expect("test port");
        let conn = Connection::new(id, addr, ConnectionConfig::default());
        conn.set_state(ConnectionState::Connected);
        conn
    }

    struct StormFixture {
        connections: Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        interest_manager: Arc<InterestManager>,
        client_layers: Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        first_full_queue: Arc<Mutex<FirstFullQueue>>,
        first_fulls_built: Arc<AtomicU64>,
    }

    fn storm_fixture(storm: usize) -> StormFixture {
        let interest_manager = Arc::new(InterestManager::new(InterestConfig::default()));
        let connections: Arc<RwLock<HashMap<u32, Arc<Connection>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let client_layers: Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let first_full_queue = Arc::new(Mutex::new(FirstFullQueue::default()));
        let first_fulls_built = Arc::new(AtomicU64::new(0));

        for i in 1..=storm as u32 {
            let ship_id = 1_000u64 + i as u64;
            interest_manager.add_ship(storm_ship(ship_id));
            interest_manager.add_player(storm_player(ship_id, i, ship_id), EntityId::new(ship_id));
            connections.write().insert(i, Arc::new(connect(i)));
            // The per-client pass's discovery step, verbatim: a client
            // becomes ready when its player is registered.
            first_full_queue.lock().push(i);
        }

        StormFixture {
            connections,
            interest_manager,
            client_layers,
            first_full_queue,
            first_fulls_built,
        }
    }

    /// Bug №710: the admission stage under a join storm. Six clients became
    /// ready in the same round; the stage admits the first
    /// `FIRST_FULL_BUDGET_PER_ROUND`, the rest keep their place, and a second
    /// pass drains the line — the burst is absorbed by the queue, not by the
    /// publishing round.
    #[test]
    fn a_storm_of_joins_admits_only_the_budget_per_round() {
        const STORM: usize = 6;
        let fx = storm_fixture(STORM);
        let snapshot = snapshot();

        let first_batch = admit_first_fulls(
            &fx.connections,
            &fx.client_layers,
            &fx.interest_manager,
            &snapshot,
            &fx.first_full_queue,
            &fx.first_fulls_built,
        );
        // Budget: exactly the budget built this round, layers registered for
        // exactly those clients, the rest still queued — and the packets are
        // not empty, the first full is real content for the client to
        // receive.
        assert_eq!(fx.client_layers.read().len(), FIRST_FULL_BUDGET_PER_ROUND);
        assert_eq!(
            fx.first_fulls_built.load(Ordering::Relaxed),
            FIRST_FULL_BUDGET_PER_ROUND as u64
        );
        assert_eq!(
            fx.first_full_queue.lock().waiting_len(),
            STORM - FIRST_FULL_BUDGET_PER_ROUND
        );
        assert!(!first_batch.is_empty(), "an admitted first full is packets");

        // A later round admits the rest of the line in order.
        admit_first_fulls(
            &fx.connections,
            &fx.client_layers,
            &fx.interest_manager,
            &snapshot,
            &fx.first_full_queue,
            &fx.first_fulls_built,
        );
        assert_eq!(fx.client_layers.read().len(), STORM);
        assert_eq!(fx.first_fulls_built.load(Ordering::Relaxed), STORM as u64);
        assert_eq!(fx.first_full_queue.lock().waiting_len(), 0);
    }

    /// A client that lost its player between discovery and admission (quit,
    /// despawn, rejoin) is dropped from the line instead of being admitted —
    /// the same shape bug №164 locked out of the hot path, recreated here
    /// faithfully if the ready re-check is dropped.
    #[test]
    fn a_client_who_lost_its_player_is_dropped_not_admitted() {
        const STORM: usize = 1;
        let fx = storm_fixture(STORM);
        let snapshot = snapshot();

        // The game layer despawns the player while their connection still
        // sits in the queue.
        fx.interest_manager.remove_player(1, EntityId::new(1_001));

        let batch = admit_first_fulls(
            &fx.connections,
            &fx.client_layers,
            &fx.interest_manager,
            &snapshot,
            &fx.first_full_queue,
            &fx.first_fulls_built,
        );
        // Nothing admitted, nothing built, no layer base behind an empty
        // replica — and the line is empty again instead of holding a ghost.
        assert!(batch.is_empty());
        assert!(fx.client_layers.read().get(&1).is_none());
        assert_eq!(fx.first_fulls_built.load(Ordering::Relaxed), 0);
        assert_eq!(fx.first_full_queue.lock().waiting_len(), 0);
    }
}

#[cfg(test)]
mod event_interest_tests {
    use super::*;
    use rfs_core::entity::PlayerPosture;
    use rfs_core::math::{Transform, Vec3f};

    const VIEWER: u32 = 1;
    const VIEWER_SHIP: u64 = 10;
    const VIEWER_PLAYER_ENTITY: u64 = 20;
    const FAR_SHIP: u64 = 30;
    const FAR_PLAYER_ENTITY: u64 = 40;
    const BULKHEAD: u64 = 50;

    fn ship(id: u64, x: f32) -> ShipEntity {
        ShipEntity {
            entity_id: EntityId::new(id),
            ship_class_id: 0,
            transform: Transform::from_position(Vec3f::new(x, 0.0, 0.0)),
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 100.0,
            max_health: 100.0,
            flags: EntityFlags::empty(),
            fuel: 50.0,
            max_fuel: 50.0,
            speed: 0.0,
            max_speed: 10.0,
            heading: 0.0,
            rudder_angle: 0.0,
            throttle: 0.0,
            last_hit: None,
            compartments: Vec::new(),
            stations: Vec::new(),
            team: 0,
        }
    }

    fn player(entity_id: u64, player_id: u32, ship_id: u64, name: &str) -> PlayerEntity {
        PlayerEntity {
            entity_id: EntityId::new(entity_id),
            player_id,
            name: name.to_string(),
            team: 0,
            transform: Transform::from_position(Vec3f::new(0.0, 0.0, 0.0)),
            velocity: Vec3f::ZERO,
            posture: PlayerPosture::Standing,
            health: 100.0,
            max_health: 100.0,
            stamina: 100.0,
            max_stamina: 100.0,
            current_station: None,
            current_ship: Some(EntityId::new(ship_id)),
            input_sequence: 0,
            last_acknowledged_tick: 0,
        }
    }

    /// The viewer at the origin on its own ship, plus a second hull and crewman
    /// parked far beyond `ship_radius` (5000 by default), so anything about them
    /// is invisible by construction.
    fn world() -> InterestManager {
        let mgr = InterestManager::new(InterestConfig::default());
        mgr.add_ship(ship(VIEWER_SHIP, 0.0));
        mgr.add_player(
            player(VIEWER_PLAYER_ENTITY, VIEWER, VIEWER_SHIP, "viewer"),
            EntityId::new(VIEWER_SHIP),
        );
        mgr.add_ship(ship(FAR_SHIP, 20_000.0));
        mgr.add_player(
            player(FAR_PLAYER_ENTITY, 2, FAR_SHIP, "far"),
            EntityId::new(FAR_SHIP),
        );
        mgr.compute_interest(VIEWER);
        mgr
    }

    fn ship_sunk(ship_id: u64) -> GameEvent {
        GameEvent::ShipSunk {
            ship: EntityId::new(ship_id),
            position: Vec3f::ZERO,
        }
    }

    /// Every variant has to name the entity it happened to, or the filter has
    /// nothing to filter on and the leak comes straight back. The two global
    /// variants name none — they cannot be filtered, and they are not meant to
    /// be.
    #[test]
    fn every_event_variant_names_its_subject() {
        let id = EntityId::new(1);
        let empty = |e: GameEvent| NetServer::event_subjects(&e).is_empty();

        assert!(empty(GameEvent::TeamScoreChanged { team: 0, score: 1 }));
        assert!(empty(GameEvent::MatchStateChanged {
            state: MatchState::Waiting
        }));

        let named = [
            GameEvent::ShipHit {
                target: id,
                projectile: id,
                position: Vec3f::ZERO,
                normal: Vec3f::ZERO,
                damage: 1.0,
                penetration: 1.0,
                hit_compartment: None,
            },
            ship_sunk(1),
            // Bug №701: the manifest pin must not rot when a variant smells
            // future — a subjectless variant would mirror the no-leak test
            // here.
            GameEvent::ShipSinkingStarted { ship: id },
            GameEvent::CompartmentFlooded {
                compartment: id,
                water_level: 1.0,
            },
            GameEvent::CompartmentSealed { compartment: id },
            GameEvent::CompartmentBreached { compartment: id },
            GameEvent::StationOccupied {
                station: id,
                player: id,
            },
            GameEvent::StationVacated {
                station: id,
                player: id,
            },
            GameEvent::PlayerSpawned {
                player: id,
                ship: id,
                position: Vec3f::ZERO,
            },
            GameEvent::PlayerDied {
                player: id,
                killer: None,
            },
            GameEvent::ProjectileFired {
                projectile: id,
                position: Vec3f::ZERO,
                velocity: Vec3f::ZERO,
                weapon: id,
            },
            GameEvent::ProjectileImpact {
                projectile: id,
                position: Vec3f::ZERO,
                normal: Vec3f::ZERO,
            },
            GameEvent::BulkheadStateChanged {
                bulkhead: id,
                is_sealed: true,
            },
            GameEvent::PumpStateChanged {
                pump: id,
                is_active: true,
            },
            GameEvent::FireStarted {
                compartment: id,
                intensity: 1.0,
            },
            GameEvent::FireExtinguished { compartment: id },
        ];
        for event in named {
            assert!(
                !NetServer::event_subjects(&event).is_empty(),
                "{event:?} named no subject, so it could never be filtered"
            );
        }
    }

    /// The defect itself: a hull the client's snapshot does not contain must not
    /// be described by its events. Before №635 this reached the viewer.
    #[test]
    fn an_event_about_a_ship_outside_the_viewer_interest_is_not_sent() {
        let mgr = world();
        assert!(
            !mgr.should_replicate_entity(VIEWER, EntityId::new(FAR_SHIP)),
            "the fixture is not actually invisible, so this proves nothing"
        );
        assert!(
            !NetServer::event_reaches(&mgr, VIEWER, &ship_sunk(FAR_SHIP)),
            "a ship beyond interest leaked to the viewer through its events"
        );
    }

    /// The other half: the same event about a hull the viewer can see still
    /// arrives. Without this the filter could be made to pass by dropping
    /// everything.
    #[test]
    fn an_event_about_a_ship_the_viewer_can_see_is_sent() {
        let mgr = world();
        assert!(
            mgr.should_replicate_entity(VIEWER, EntityId::new(VIEWER_SHIP)),
            "the fixture's own ship is not visible, so this proves nothing"
        );
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &ship_sunk(VIEWER_SHIP)),
            "an event about a visible ship was dropped"
        );
    }

    /// Bug №634: chat is routed by its channel, not by the entity in it.
    /// `All` names no subject — everyone on the server is in the chat. A
    /// narrower channel is about the sender: the crew that can see the
    /// player hears them, interest decides; a far player's narrow chat must
    /// not reach a viewer who cannot see them. (The command path refuses
    /// Team/Whisper outright, so Ship is the only narrow channel that
    /// reaches the wire as an event.)
    #[test]
    fn chat_events_are_routed_by_their_channel() {
        let mgr = world();
        let author = EntityId::new(VIEWER_PLAYER_ENTITY);
        let all_chat = GameEvent::ChatReceived {
            player: author,
            message: "hello all".into(),
            channel: ChatChannel::All,
        };
        let ship_chat = GameEvent::ChatReceived {
            player: author,
            message: "hello ship".into(),
            channel: ChatChannel::Ship,
        };
        let far_chat = GameEvent::ChatReceived {
            player: EntityId::new(FAR_PLAYER_ENTITY),
            message: "hello ship".into(),
            channel: ChatChannel::Ship,
        };

        assert!(
            NetServer::event_subjects(&all_chat).is_empty(),
            "all-chat named a subject, so interest would filter what cannot be filtered"
        );
        assert!(
            NetServer::event_subjects(&ship_chat).contains(&author),
            "ship-chat named no subject, so it would broadcast to the whole server"
        );
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &all_chat),
            "a viewer must receive all-chat"
        );
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &ship_chat),
            "a crewmate must receive the ship channel of a player they can see"
        );
        assert!(
            !NetServer::event_reaches(&mgr, VIEWER, &far_chat),
            "a far player's narrow chat leaked through the event path"
        );
    }

    /// `entity_layers` never holds bulkheads or pumps, so a plain
    /// `should_replicate_entity` answers `false` for them and the whole
    /// category goes silent. This is the carve-out that keeps that from
    /// shipping unnoticed.
    #[test]
    fn an_event_about_an_id_the_manager_does_not_track_is_not_silenced() {
        let mgr = world();
        let event = GameEvent::BulkheadStateChanged {
            bulkhead: EntityId::new(BULKHEAD),
            is_sealed: true,
        };
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &event),
            "an event about an untracked id was silently dropped"
        );
    }

    /// The same idea, checked from the other side: the manager must not report
    /// a bulkhead as invisible just because it does not know it.
    #[test]
    fn an_untracked_id_is_not_reported_as_invisible() {
        let mgr = world();
        assert!(
            mgr.should_replicate_subject(VIEWER, EntityId::new(BULKHEAD)),
            "an id the manager has never seen was treated as hidden"
        );
        assert!(
            !mgr.should_replicate_subject(VIEWER, EntityId::new(FAR_SHIP)),
            "a tracked ship outside interest was treated as visible"
        );
    }

    /// One visible subject is enough. A player who can see the hull that was
    /// hit needs the hit even when the shell has already flown out of range.
    #[test]
    fn one_visible_subject_is_enough() {
        let mgr = world();
        let event = GameEvent::PlayerSpawned {
            // Own crewman (visible) aboard a hull nobody can see.
            player: EntityId::new(VIEWER_PLAYER_ENTITY),
            ship: EntityId::new(FAR_SHIP),
            position: Vec3f::ZERO,
        };
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &event),
            "an event was dropped although one of its subjects was visible"
        );

        // Neither subject visible ⇒ dropped.
        let event = GameEvent::PlayerSpawned {
            player: EntityId::new(FAR_PLAYER_ENTITY),
            ship: EntityId::new(FAR_SHIP),
            position: Vec3f::ZERO,
        };
        assert!(
            !NetServer::event_reaches(&mgr, VIEWER, &event),
            "an event about two invisible subjects reached the viewer"
        );
    }

    /// Match and team state name no entity and go to everyone who has a world.
    #[test]
    fn an_event_with_no_subject_reaches_a_tracked_viewer() {
        let mgr = world();
        let event = GameEvent::MatchStateChanged {
            state: MatchState::Starting,
        };
        assert!(NetServer::event_reaches(&mgr, VIEWER, &event));
        assert!(NetServer::event_reaches(
            &mgr,
            VIEWER,
            &GameEvent::TeamScoreChanged { team: 0, score: 3 }
        ));
    }

    /// A connection the game layer has not registered yet has no world, and
    /// Bug №164 already holds its first full state back for that reason. Events
    /// follow the same rule instead of describing a world it has not been given.
    #[test]
    fn a_viewer_the_game_layer_has_not_registered_yet_hears_nothing() {
        let mgr = world();
        let unregistered = 99;
        assert!(!mgr.has_player(unregistered));
        assert!(!NetServer::event_reaches(
            &mgr,
            unregistered,
            &ship_sunk(VIEWER_SHIP)
        ));
        assert!(
            !NetServer::event_reaches(
                &mgr,
                unregistered,
                &GameEvent::MatchStateChanged {
                    state: MatchState::Starting,
                }
            ),
            "an untracked viewer was handed a match event"
        );
    }
}
