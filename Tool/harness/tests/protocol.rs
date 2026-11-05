// Moved out of crates/core/src/packet.rs (side-only testing rule).
// Bug №732: payloads on the wire are postcard, the header is the hand-rolled
// fixed codec (`PacketHeader::to_wire`/`decode`); these pins follow the split.
use rfs_core::packet::*;

#[test]
fn header_size_matches_wire_format() {
    let header = PacketHeader {
        magic: PacketHeader::MAGIC,
        version: PROTOCOL_VERSION,
        ..Default::default()
    };
    // Bug №732 is why this reads as a length pin on a hand-rolled writer: the
    // header is no longer a serializer's output whose size could drift with a
    // format, it is `HEADER_SIZE` bytes by construction — and the const has
    // to keep meaning exactly what the writer emits.
    let wire = header.to_wire();
    assert_eq!(
        wire.len(),
        HEADER_SIZE,
        "the hand-rolled header wrote {} bytes but HEADER_SIZE is {HEADER_SIZE}",
        wire.len()
    );
    // And it has to read back field for field: decode agreeing with encode is
    // the whole contract the fixed codec sells.
    assert_eq!(PacketHeader::decode(&wire).unwrap(), header);
}

#[test]
fn packet_roundtrip() {
    let packet = ConnectPacket {
        client_id: uuid::Uuid::nil(),
        protocol_version: PROTOCOL_VERSION,
        player_name: "test".to_string(),
        build_version: "0.1.0".to_string(),
    };
    let header = PacketHeader::new(
        PacketType::Connect,
        ChannelType::ReliableOrdered,
        1,
        0,
        0,
        0,
    );
    let data = serialize_packet(&packet, header).unwrap();
    let (parsed_header, payload) = deserialize_packet(&data).unwrap();
    assert_eq!(parsed_header.packet_type, header.packet_type);
    // Bug №732: the body is postcard — decode through the same trait the
    // production readers (`deserialize_limited` and friends) go through.
    let parsed = <ConnectPacket as Serializable>::deserialize(payload).unwrap();
    assert_eq!(parsed.player_name, "test");
}

#[test]
fn packet_type_wire_discriminants_are_positional_and_pinned() {
    // №60: bincode 1.3 coded enums positionally (variant declaration order),
    // ignoring the explicit `= N` values; postcard (bug №732) keeps the same
    // property — an enum encodes as the varint ordinal of the declaration
    // order. Pin that order here so a reorder, insert or delete fails THIS
    // test instead of silently corrupting the wire — any change below
    // requires a PROTOCOL_VERSION bump.
    let cases: &[(PacketType, u32)] = &[
        (PacketType::Connect, 0),
        (PacketType::ConnectAccept, 1),
        (PacketType::ConnectReject, 2),
        (PacketType::Disconnect, 3),
        (PacketType::Heartbeat, 4),
        (PacketType::HeartbeatAck, 5),
        (PacketType::Input, 6),
        (PacketType::InputAck, 7),
        (PacketType::State, 8),
        (PacketType::StateDelta, 9),
        (PacketType::StateFull, 10),
        (PacketType::Fragment, 11),
        (PacketType::Event, 12),
        (PacketType::EventAck, 13),
        (PacketType::Command, 14),
        (PacketType::CommandAck, 15),
        (PacketType::Rpc, 16),
        (PacketType::RpcResponse, 17),
        // Bug №267 (A): appended after RpcResponse, so every earlier variant
        // keeps its positional code and the pin for them stands untouched.
        // The append does not disturb the wire; that is the assert's point.
        (PacketType::ResyncRequest, 18),
        // Bug №698 (19), №216 (20-21), №30 (B) (22): the later appends. The
        // same rule as №267 (A) — positional, one byte, and nothing before
        // them moves. An older reader stops at the type gate; an older
        // sender never emits the new tails.
        (PacketType::MapDescription, 19),
        (PacketType::ConnectChallenge, 20),
        (PacketType::ConnectConfirm, 21),
        (PacketType::LayerSync, 22),
    ];
    for (variant, expected) in cases {
        let bytes = <PacketType as Serializable>::serialize(variant).unwrap();
        // Varint framing note: every code here is below 128, so the postcard
        // ordinal is exactly one byte. Pinning the length also rejects any
        // widening of the encoding — a wider varint would be its own silent
        // protocol break for every reader that slices a single byte.
        assert_eq!(
            bytes.len(),
            1,
            "variant {variant:?} encoded as {bytes:?}; the one-byte ordinal moved"
        );
        assert_eq!(
            bytes[0], *expected as u8,
            "reordering PacketType variants corrupts the wire (bug №60); bump PROTOCOL_VERSION"
        );
    }
}

/// Bug №30 (B): the applied report is the source the server's confirmed diff
/// bases move from, so its payload must round-trip byte-exact — a drifted
/// codec would attribute layer states to a client that never sent them.
#[test]
fn layer_sync_report_roundtrips_through_the_payload_codec() {
    let report = LayerSyncPacket {
        applied: [401, 402, 403, 404],
        session: 0x0102_0304_0506_0708_1122_3344_5566_7788,
    };
    let bytes = <LayerSyncPacket as Serializable>::serialize(&report).unwrap();
    let back: LayerSyncPacket =
        <LayerSyncPacket as Serializable>::deserialize(&bytes).unwrap();
    assert_eq!(
        back, report,
        "the report must survive one encode/decode cycle unchanged"
    );
    // And the codec is deterministic: the same value serializes to the same
    // bytes, so a cache/table keyed by payload bytes cannot drift.
    let again = <LayerSyncPacket as Serializable>::serialize(&report).unwrap();
    assert_eq!(bytes, again, "serializing the same report twice must not drift");
}

// Bug №746: the transform rides the wire in its own compact form,
// `WireTransform` — position f32×3, the rotation as smallest-three (index of
// the largest component as one byte, the other three as i16 shares of it),
// scale present only when it is not ONE (the probe measured ~99-100% records
// at scale == ONE). These pins keep the byte budget and the precision
// contract on the same side as the other wire pins; a move means a
// PROTOCOL_VERSION bump with a note about who depends on it.
use rfs_core::math::{Quatf, Transform, Vec3f};

/// №746 residual bound, pinned with margin: the quantization step is
/// 1/32767 of the largest component (~3e-5), the reconstruction adds a
/// second-order share, and the server's replication-diff epsilon is 0.001 —
/// so a server-visible rotation change always survives one encode/decode
/// hop. 1.5e-4 keeps three-plus steps of headroom.
const ROTATION_WIRE_EPS: f32 = 1.5e-4;

fn assert_rotation_wire_close(got: Quatf, want: Quatf) {
    // №746: the wire canonizes the binary sign (q ≡ −q are one rotation),
    // so a component-wise compare must align hemispheres first — otherwise
    // the SAME rotation reads as a whole-component sign flip.
    let aligned_dot = got.x * want.x + got.y * want.y + got.z * want.z + got.w * want.w;
    let s = if aligned_dot < 0.0 { -1.0 } else { 1.0 };
    let err = [
        (s * got.x - want.x).abs(),
        (s * got.y - want.y).abs(),
        (s * got.z - want.z).abs(),
        (s * got.w - want.w).abs(),
    ]
    .into_iter()
    .fold(0.0f32, f32::max);
    assert!(
        err <= ROTATION_WIRE_EPS,
        "the wire rotation drifted {err:.2e}, bound is {ROTATION_WIRE_EPS}"
    );
}

#[test]
fn the_no746_wire_transform_pins_the_one_scale_case_under_the_no746_budget() {
    // The common case: scale == ONE stays implicit. The heft is bounded
    // above (postcard frames the i16 shares as zigzag varints — small
    // shares, the常态 for dominant-component rotations, ride 1-2 B each):
    // 12 (position) + 1 (axis byte) + up to 9 (three i16 at the full
    // ±32767 swing) + 1 (None tag) = 23 B worst case, against the 40 B a
    // four-f32 core Transform costs. The №746 layer-2 payloads averaged
    // 1049-1427 B over the 1352 B threshold — this budget carries them
    // back under it.
    let t = Transform::new(
        Vec3f::new(12.5, -3.5, 1021.29),
        Quatf::from_euler(0.3, -1.7, 2.9),
        Vec3f::ONE,
    );
    let bytes = <WireTransform as Serializable>::serialize(&WireTransform::from_transform(&t))
        .unwrap();
    assert!(
        bytes.len() <= 23,
        "ONE-scale WireTransform exceeded the №746 budget: {} > 23",
        bytes.len()
    );
    assert!(
        bytes.len() < 40,
        "the wire form must stay under the core Transform's own 40 B"
    );
}

#[test]
fn the_no746_wire_transform_encodes_an_identity_exactly_seventeen_bytes() {
    // The identity rides the fast path: shares are all zero (one byte each
    // in postcard's varint), axis 3 carries nothing, the None scale tag
    // closes the form — 12 + 1 + 3 + 1 = 17 bytes, bit-stable across
    // encodes. A drift here means the field order or the varint behaviour
    // moved: PROTOCOL_VERSION bump with a note about who depends on it.
    let t = Transform::new(Vec3f::new(7.5, -2.0, 88.0), Quatf::IDENTITY, Vec3f::ONE);
    let bytes = <WireTransform as Serializable>::serialize(&WireTransform::from_transform(&t))
        .unwrap();
    assert_eq!(
        bytes.len(),
        17,
        "the identity WireTransform moved; №746 pins it at 17 B"
    );
    let back = <WireTransform as Serializable>::deserialize(&bytes)
        .unwrap()
        .into_transform();
    assert_eq!(back.scale, Vec3f::ONE);
    assert_eq!(back.rotation, Quatf::IDENTITY, "identity must round-trip exact");
}

#[test]
fn the_no746_wire_transform_carries_a_non_one_scale_losslessly() {
    // A user ship-JSON station may set a scale (№746: station world
    // transforms inherit `ss.local_transform.scale`) — the Option must be
    // honest: Some rides, decodes exactly, and costs 1 + 12 on the wire
    // (total 32; still under the old 40-byte core form).
    let t = Transform::new(
        Vec3f::new(0.0, 1.5, 40.0),
        Quatf::from_euler(0.0, 0.0, 90.0_f32.to_radians()),
        Vec3f::new(2.0, 3.0, 4.0),
    );
    let wire = <WireTransform as Serializable>::serialize(&WireTransform::from_transform(&t))
        .unwrap();
    assert!(
        wire.len() <= 36,
        "non-ONE-scale WireTransform exceeded the №746 budget: {} > 36",
        wire.len()
    );
    let back = <WireTransform as Serializable>::deserialize(&wire)
        .unwrap()
        .into_transform();
    assert_eq!(back.position, t.position, "position must round-trip exact");
    assert_eq!(back.scale, t.scale, "a carried scale must come back exact");
    assert_rotation_wire_close(back.rotation, t.rotation);
}

#[test]
fn the_no746_wire_transform_rotation_round_trips_within_the_replication_epsilon() {
    // A spread of rotations from across the real range: identity, yaw,
    // pitch-roll mix, and a double-transformed rotor. The smallest-three
    // reconstruction must stay far below the server's replication-diff
    // epsilon (0.001) — otherwise an applied delta could hide a rotation
    // change the server did report.
    let cases = [
        Quatf::IDENTITY,
        Quatf::from_axis_angle(Vec3f::UP, 0.75),
        Quatf::from_euler(0.9, -2.3, 1.1),
        (Quatf::from_euler(0.4, 0.4, 0.4) * Quatf::from_euler(-1.3, 0.2, 0.8)),
    ];
    for q in cases {
        let t = Transform::new(Vec3f::new(5.0, -6.0, 7.0), q, Vec3f::ONE);
        let wire = <WireTransform as Serializable>::serialize(&WireTransform::from_transform(&t))
            .unwrap();
        let back =
            <WireTransform as Serializable>::deserialize(&wire).unwrap().into_transform();
        assert_eq!(back.position, t.position, "position must round-trip exact");
        assert_eq!(back.scale, t.scale, "scale must round-trip exact");
        assert_rotation_wire_close(back.rotation, t.rotation);
    }
}

#[test]
fn the_no746_wire_transform_encodes_canonical_bytes_for_one_rotation() {
    // q and −q are the same rotation; the wire form must not see the
    // difference (the server's delta payload cache keys on serialized
    // bytes), and the same value must serialize twice to the same bytes.
    let q = Quatf::from_euler(1.1, -0.6, 2.2);
    let t = Transform::new(Vec3f::new(1.0, 2.0, 3.0), q, Vec3f::ONE);
    let flipped = Transform::new(Vec3f::new(1.0, 2.0, 3.0), Quatf::new(-q.x, -q.y, -q.z, -q.w), Vec3f::ONE);
    let a = <WireTransform as Serializable>::serialize(&WireTransform::from_transform(&t)).unwrap();
    let b =
        <WireTransform as Serializable>::serialize(&WireTransform::from_transform(&flipped))
            .unwrap();
    let a_again =
        <WireTransform as Serializable>::serialize(&WireTransform::from_transform(&t)).unwrap();
    assert_eq!(a, a_again, "encoding one value twice must not drift");
    assert_eq!(a, b, "q and −q are one rotation: the canonical bytes must agree");
}

#[test]
fn a_transform_only_delta_record_stays_far_below_the_fragmentation_threshold() {
    // One updated record that only moves a transform. Every Option field on
    // the wire form costs its tag byte; compacting the transform drops the
    // per-record heft by ~20 B — the record itself must sit orders below the
    // fragmentation threshold (MAX_PACKET_SIZE − HEADER_SIZE − AUTH_WIRE_LEN
    // = 1400 − 24 − 24 = 1352), so a delta built of such records and a
    // serialized variance of a few hundred bytes stay in one datagram.
    let update = EntityStateUpdate {
        entity_id: EntityId::new(5),
        transform: Some(WireTransform::from_transform(&Transform::new(
            Vec3f::new(320.0, 40.0, -128.5),
            Quatf::from_euler(-0.7, 1.4, 0.02),
            Vec3f::ONE,
        ))),
        velocity: None,
        angular_velocity: None,
        health: None,
        flags: None,
        ship_data: None,
        station_data: None,
        player_data: None,
    };
    let bytes = <EntityStateUpdate as Serializable>::serialize(&update).unwrap();
    // Worst-case budget: id varint (1) + Some-transform (1 + 23) + seven
    // None tags (7) = 32; typicallys smaller as dominant-component
    // rotations ride the short varints.
    assert!(
        bytes.len() <= 32,
        "the transform-only record exceeded the №746 budget: {} > 32",
        bytes.len()
    );
    assert!(
        bytes.len() <= MAX_PACKET_SIZE - HEADER_SIZE - AUTH_WIRE_LEN,
        "a transform-only delta record must never fragment"
    );
}

// Stage 2 (`server-interface-plan.md`, Приложение А, А.2 item 1): the
// operator's `say` rides a new GameEvent variant. Appended after
// ShipSinkingStarted per the LayerSync precedent — every prior variant keeps
// its positional code, so no PROTOCOL_VERSION bump is required (an older
// receiver gets decode Err on the tail only, which net/client.rs already
// survives).

/// The Announcement pin, on the №267 shape: the variant's positional code,
/// the one-byte warn (every prior code below it is pinned by construction of
/// the append rule), and the payload round-trip.
#[test]
fn the_announcement_pins_the_appended_slot_and_roundtrips() {
    let announcement = GameEvent::Announcement {
        text: "server stops in 10 s".to_string(),
    };
    let bytes = <GameEvent as Serializable>::serialize(&announcement).unwrap();
    // Ordinal 20: the previous appends — ShipRam (18), ShipSinkingStarted
    // (19) — hold their slots, Announcement takes 20. One varint byte; the
    // event keeps the same one-byte framing class the pinned appends fixed.
    assert_eq!(
        bytes[0], 20,
        "Announcement's positional code moved; bump PROTOCOL_VERSION and rebase every reader"
    );
    // And the earlier tail holds: ShipSinkingStarted is still 19 — an
    // insertion before it would have shifted Announcement without moving the
    // wire (the decode of an old sender would read the wrong event).
    let sink_started = GameEvent::ShipSinkingStarted {
        ship: EntityId::new(7),
    };
    let sink_bytes = <GameEvent as Serializable>::serialize(&sink_started).unwrap();
    assert_eq!(sink_bytes[0], 19, "ShipSinkingStarted's own slot moved");

    // The payload round-trips through the EventPacket carrier the server
    // actually sends (net/server.rs), so the words a kicked player gets are
    // the words the operator typed. (GameEvent itself names no PartialEq —
    // the pin reads the payload back field by field.)
    let packet = EventPacket {
        events: vec![announcement],
    };
    let wire = <EventPacket as Serializable>::serialize(&packet).unwrap();
    let back = <EventPacket as Serializable>::deserialize(&wire).unwrap();
    match &back.events[0] {
        GameEvent::Announcement { text } => {
            assert_eq!(text, "server stops in 10 s", "the text drifted on the wire");
        }
        other => panic!("decode gave {other:?}; the announcement became something else"),
    }
}
