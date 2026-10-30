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
