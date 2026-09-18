// Moved out of crates/core/src/packet.rs (side-only testing rule).
use rfs_core::packet::*;

#[test]
fn header_size_matches_wire_format() {
    let header = PacketHeader {
        magic: PacketHeader::MAGIC,
        version: PROTOCOL_VERSION,
        ..Default::default()
    };
    let wire_size = bincode::serialized_size(&header).unwrap() as usize;
    assert_eq!(
        wire_size, HEADER_SIZE,
        "serialized PacketHeader is {wire_size} bytes but HEADER_SIZE is {HEADER_SIZE}"
    );
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
    let parsed: ConnectPacket = bincode::deserialize(payload).unwrap();
    assert_eq!(parsed.player_name, "test");
}

#[test]
fn packet_type_wire_discriminants_are_positional_and_pinned() {
    // №60: bincode 1.3 codes enums positionally (variant declaration order),
    // ignoring the explicit `= N` values. Pin that order here so a reorder,
    // insert or delete fails THIS test instead of silently corrupting the
    // wire — any change below requires a PROTOCOL_VERSION bump.
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
    ];
    for (variant, expected) in cases {
        let bytes = bincode::serialize(variant).unwrap();
        let encoded = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        assert_eq!(
            encoded, *expected,
            "reordering PacketType variants corrupts the wire (bug №60); bump PROTOCOL_VERSION"
        );
    }
}
