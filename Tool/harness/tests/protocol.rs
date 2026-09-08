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
