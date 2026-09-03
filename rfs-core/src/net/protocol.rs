/// Maximum allowed payload size in bytes to prevent allocation-based DoS.
/// Raised from 8192 to 65536 so that WorldSnapshots with many entities are not
/// silently dropped by the decoder.  This is still well within UDP limits for
/// LAN and acceptable for internet use.
pub const MAX_PAYLOAD_SIZE: u32 = 65536;

/// Safe payload size for WAN (avoids IP fragmentation at 1500 MTU).
/// Header (17 bytes) + chunk header (12 bytes) + payload fits in single MTU.
pub const WAN_MAX_PAYLOAD: u32 = 1400;

/// Maximum payload per chunk (leaves room for chunk header).
pub const CHUNK_PAYLOAD_SIZE: usize = 1350;

/// Minimum chunk payload size used to cap total_chunks (sanity check).
/// Ensures total_chunks cannot exceed MAX_CHUNKED_MESSAGE_SIZE / MIN_CHUNK_SIZE.
pub const MIN_CHUNK_SIZE: usize = 64 * 1024;

/// Maximum total_chunks allowed per message (alloc-bomb mitigation).
pub const MAX_TOTAL_CHUNKS: u16 = 256;

/// Maximum total message size that can be chunked (64 MB).
pub const MAX_CHUNKED_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// Maximum time to wait for all chunks before discarding (seconds).
pub const CHUNK_TIMEOUT_SECS: u64 = 5;

/// Size of the binary packet header in bytes.
pub const HEADER_SIZE: usize = 17;

/// Size of the chunk header in bytes.
pub const CHUNK_HEADER_SIZE: usize = 12;

/// Current network protocol version.
/// v2 (0.3.2 fork): wire-incompatible with 0.3.x by design — the protocol is
/// slated for full reimplementation, so the version was bumped on day one to
/// fail closed instead of silently misparsing old packets.
pub const PROTOCOL_VERSION: u32 = 2;

/// The type of a network packet, determining its delivery guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PacketType {
    /// Guaranteed delivery with acknowledgement and ordering.
    Reliable,
    /// Sent once per frame with sequence numbers for ordering; dropped packets are not resent.
    UnreliableSequenced,
    /// Acknowledgement of received reliable packets.
    Ack,
    /// Client-to-server latency probe.
    Ping,
    /// Server-to-client latency response.
    Pong,
    /// A chunk of a larger message that was split for transport.
    Chunked,
}

/// Fixed-size header prepended to every network packet.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PacketHeader {
    /// The packet's delivery type.
    pub packet_type: PacketType,
    /// Sequence number of this packet.
    pub sequence: u16,
    /// Sequence number of the last packet received from the peer.
    pub ack_sequence: u16,
    /// Bitmask acknowledging the 32 packets before `ack_sequence`.
    pub ack_bits: u32,
    /// CRC-32 checksum of the payload.
    pub crc: u32,
    /// Length of the payload in bytes.
    pub payload_len: u32,
}

impl PacketHeader {
    /// Creates a new header with the given packet type and sequence number.
    ///
    /// All other fields are initialized to zero.
    pub fn new(packet_type: PacketType, sequence: u16) -> Self {
        Self {
            packet_type,
            sequence,
            ack_sequence: 0,
            ack_bits: 0,
            crc: 0,
            payload_len: 0,
        }
    }
}

/// Header for a chunk of a larger message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChunkHeader {
    /// Unique message ID (all chunks of the same message share this ID).
    pub message_id: u32,
    /// Zero-based index of this chunk.
    pub chunk_index: u16,
    /// Total number of chunks in this message.
    pub total_chunks: u16,
}

impl ChunkHeader {
    pub fn new(message_id: u32, chunk_index: u16, total_chunks: u16) -> Self {
        Self {
            message_id,
            chunk_index,
            total_chunks,
        }
    }
}

fn crc32_bytes(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

/// A complete network packet with a header and payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Packet {
    /// The packet header.
    pub header: PacketHeader,
    /// The raw payload bytes.
    pub payload: Vec<u8>,
}

impl Packet {
    /// Creates a new packet, computing the CRC and payload length automatically.
    pub fn new(packet_type: PacketType, sequence: u16, payload: Vec<u8>) -> Self {
        let mut p = Self {
            header: PacketHeader::new(packet_type, sequence),
            payload,
        };
        p.header.payload_len = p.payload.len() as u32;
        p.header.crc = crc32_bytes(&p.payload);
        p
    }

    /// Serializes the packet into a byte vector for transmission.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(HEADER_SIZE + self.payload.len());
        buf.extend_from_slice(&self.header.sequence.to_le_bytes());
        buf.extend_from_slice(&self.header.ack_sequence.to_le_bytes());
        buf.extend_from_slice(&self.header.ack_bits.to_le_bytes());
        buf.extend_from_slice(&self.header.crc.to_le_bytes());
        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        buf.push(match self.header.packet_type {
            PacketType::Reliable => 0,
            PacketType::UnreliableSequenced => 1,
            PacketType::Ack => 2,
            PacketType::Ping => 3,
            PacketType::Pong => 4,
            PacketType::Chunked => 5,
        });
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Deserializes a packet from raw bytes.
    ///
    /// Returns an error string if the data is too short, has an unknown packet type,
    /// truncated payload, or CRC mismatch.
    pub fn decode(data: &[u8]) -> Result<Self, &'static str> {
        if data.len() < HEADER_SIZE {
            return Err("packet too short");
        }
        let mut offset = 0;
        let sequence = u16::from_le_bytes([data[offset], data[offset + 1]]);
        offset += 2;
        let ack_sequence = u16::from_le_bytes([data[offset], data[offset + 1]]);
        offset += 2;
        let ack_bits = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let crc = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        let payload_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        if payload_len > MAX_PAYLOAD_SIZE {
            return Err("payload length exceeds maximum allowed size");
        }
        offset += 4;
        let packet_type = match data[offset] {
            0 => PacketType::Reliable,
            1 => PacketType::UnreliableSequenced,
            2 => PacketType::Ack,
            3 => PacketType::Ping,
            4 => PacketType::Pong,
            5 => PacketType::Chunked,
            _ => return Err("unknown packet type"),
        };
        offset += 1;
        let end = offset
            .checked_add(payload_len as usize)
            .ok_or("payload length overflow")?;
        if end > data.len() {
            return Err("payload length exceeds available data");
        }
        let payload = data[offset..end].to_vec();
        let expected_crc = crc32_bytes(&payload);
        if crc != expected_crc {
            return Err("CRC mismatch");
        }
        Ok(Self {
            header: PacketHeader {
                packet_type,
                sequence,
                ack_sequence,
                ack_bits,
                crc,
                payload_len,
            },
            payload,
        })
    }
}

/// Utility functions for message chunking.
pub mod chunking {
    use super::*;

    /// Calculates the number of chunks needed for a given payload size.
    pub fn chunk_count(payload_len: usize) -> usize {
        if payload_len == 0 {
            1
        } else {
            payload_len.div_ceil(CHUNK_PAYLOAD_SIZE)
        }
    }

    /// Splits a payload into chunks, each with a chunk header prepended.
    /// Returns a vector of (chunk_header, chunk_payload) tuples.
    pub fn split_into_chunks(message_id: u32, payload: &[u8]) -> Vec<(ChunkHeader, Vec<u8>)> {
        let total = chunk_count(payload.len());
        let mut chunks = Vec::with_capacity(total);

        for i in 0..total {
            let start = i * CHUNK_PAYLOAD_SIZE;
            let end = ((i + 1) * CHUNK_PAYLOAD_SIZE).min(payload.len());
            let chunk_data = payload[start..end].to_vec();
            let header = ChunkHeader::new(message_id, i as u16, total as u16);
            chunks.push((header, chunk_data));
        }

        chunks
    }

    /// Assembler for reassembling chunked messages on the receiver side.
    #[derive(Debug)]
    pub struct ChunkAssembler {
        /// message_id -> (total_chunks, received_chunks_map, creation_time)
        pending: std::collections::HashMap<
            u32,
            (
                u16,
                std::collections::HashMap<u16, Vec<u8>>,
                std::time::Instant,
            ),
        >,
        /// Maximum number of pending messages to prevent OOM DoS
        max_pending_messages: usize,
    }

    impl ChunkAssembler {
        pub fn new() -> Self {
            Self {
                pending: std::collections::HashMap::new(),
                max_pending_messages: 1024,
            }
        }

        /// Adds a chunk. Returns the complete message if all chunks have arrived.
        pub fn add_chunk(&mut self, header: ChunkHeader, data: Vec<u8>) -> Option<Vec<u8>> {
            // Reject obviously invalid chunks before touching state.
            if header.total_chunks == 0 {
                eprintln!(
                    "[ChunkAssembler] Rejected chunk for message_id={}: total_chunks=0",
                    header.message_id,
                );
                return None;
            }

            // Validate total_chunks doesn't exceed safe limits
            if header.total_chunks > MAX_TOTAL_CHUNKS {
                eprintln!(
                    "[ChunkAssembler] Rejected chunk for message_id={}: total_chunks={} exceeds MAX_TOTAL_CHUNKS={}",
                    header.message_id,
                    header.total_chunks,
                    MAX_TOTAL_CHUNKS,
                );
                return None;
            }

            // Validate chunk_index is within bounds
            if header.chunk_index >= header.total_chunks {
                eprintln!(
                    "[ChunkAssembler] Rejected chunk for message_id={}: chunk_index={} >= total_chunks={}",
                    header.message_id,
                    header.chunk_index,
                    header.total_chunks,
                );
                return None;
            }

            let entry = self.pending.entry(header.message_id).or_insert_with(|| {
                (
                    header.total_chunks,
                    std::collections::HashMap::new(),
                    std::time::Instant::now(),
                )
            });

            // Validate that the total_chunks matches the value recorded from the
            // first chunk of this message.  A mismatch would allow an attacker to
            // send total_chunks=1 and truncate a multi-chunk message.
            if entry.0 != header.total_chunks {
                eprintln!(
                    "[ChunkAssembler] Rejected chunk for message_id={}: total_chunks={} does not match recorded {}",
                    header.message_id,
                    header.total_chunks,
                    entry.0,
                );
                return None;
            }

            // Re-validate after we know the authoritative total_chunks.
            if header.chunk_index >= entry.0 {
                eprintln!(
                    "[ChunkAssembler] Rejected chunk for message_id={}: chunk_index={} >= total_chunks={}",
                    header.message_id,
                    header.chunk_index,
                    entry.0,
                );
                return None;
            }

            entry.1.insert(header.chunk_index, data);

            if entry.1.len() >= entry.0 as usize {
                // Verify ALL chunks from 0 to total-1 are present before reassembling
                // This prevents DoS via duplicate chunk indices
                let (total, chunks, _) = self.pending.remove(&header.message_id)?;
                // Cap the pre-allocation at the bytes actually received instead of
                // `total * CHUNK_PAYLOAD_SIZE`, which could over-reserve up to
                // 256 * 1350 ≈ 345 KB per crafted message (PR3 / MAX_CHUNKED).
                let received_bytes = chunks.values().map(|c| c.len()).sum::<usize>();
                let capacity = received_bytes.min(MAX_CHUNKED_MESSAGE_SIZE);
                let mut result = Vec::with_capacity(capacity);
                for i in 0..total {
                    match chunks.get(&i) {
                        Some(chunk) => result.extend_from_slice(chunk),
                        None => {
                            // Log and reject incomplete message - this should never happen
                            // if len >= total, but could occur with duplicate indices
                            eprintln!(
                                "[ChunkAssembler] Message {}: missing chunk index {}",
                                header.message_id, i
                            );
                            return None;
                        }
                    }
                }
                Some(result)
            } else {
                None
            }
        }

        /// Clears incomplete messages older than the timeout or evicts if over limit.
        pub fn cleanup_old(&mut self, now: std::time::Instant) {
            let timeout = std::time::Duration::from_secs(CHUNK_TIMEOUT_SECS);

            // Remove expired messages
            let expired: Vec<_> = self
                .pending
                .iter()
                .filter(|(_, (_, _, created))| now.duration_since(*created) > timeout)
                .map(|(id, _)| *id)
                .collect();
            for id in expired {
                self.pending.remove(&id);
            }

            // Enforce max pending limit to prevent OOM DoS
            // Keep at least 256 messages even under pressure
            if self.pending.len() > self.max_pending_messages {
                let to_remove = self.pending.len() - 256;
                let ids: Vec<_> = self.pending.keys().take(to_remove).copied().collect();
                for id in ids {
                    self.pending.remove(&id);
                }
            }
        }
    }

    impl Default for ChunkAssembler {
        fn default() -> Self {
            Self::new()
        }
    }
}
