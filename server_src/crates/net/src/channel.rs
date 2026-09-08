use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};

pub use rfs_core::packet::ChannelType;

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub channel_type: ChannelType,
    pub send_buffer_size: usize,
    pub receive_buffer_size: usize,
    pub max_fragments: usize,
}

impl ChannelConfig {
    pub fn new(channel_type: ChannelType, send_buffer: usize, receive_buffer: usize) -> Self {
        Self {
            channel_type,
            send_buffer_size: send_buffer,
            receive_buffer_size: receive_buffer,
            max_fragments: 16,
        }
    }
}

#[derive(Debug, Clone)]
struct QueuedPacket {
    sequence: u32,
    data: Vec<u8>,
    sent_time: std::time::Instant,
    retry_count: u32,
    acked: bool,
}

#[derive(Debug, Clone)]
struct ReceivedPacket {
    #[allow(dead_code)]
    sequence: u32,
    data: Vec<u8>,
}

pub struct Channel {
    config: ChannelConfig,
    send_queue: Mutex<VecDeque<QueuedPacket>>,
    receive_buffer: Mutex<HashMap<u32, ReceivedPacket>>,
    local_sequence: Mutex<u32>,
    remote_sequence: Mutex<u32>,
    pending_acks: Mutex<VecDeque<u32>>,
    ack_bitfield: Mutex<u32>,
    stats: Mutex<ChannelStats>,
}

#[derive(Debug, Default, Clone)]
pub struct ChannelStats {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_lost: u64,
    pub packets_retransmitted: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub avg_rtt: f32,
}

impl Channel {
    pub fn new(config: ChannelConfig) -> Self {
        Self {
            config,
            send_queue: Mutex::new(VecDeque::new()),
            receive_buffer: Mutex::new(HashMap::new()),
            local_sequence: Mutex::new(0),
            remote_sequence: Mutex::new(0),
            pending_acks: Mutex::new(VecDeque::new()),
            ack_bitfield: Mutex::new(0),
            stats: Mutex::new(ChannelStats::default()),
        }
    }

    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }

    pub fn next_sequence(&self) -> u32 {
        let mut seq = self.local_sequence.lock();
        *seq = seq.wrapping_add(1);
        *seq
    }

    pub fn queue_packet(&self, data: Vec<u8>) -> Result<u32, ChannelError> {
        if data.len() > 1400 {
            return Err(ChannelError::PacketTooLarge);
        }

        let sequence = self.next_sequence();
        let mut queue = self.send_queue.lock();
        
        if queue.len() >= self.config.send_buffer_size {
            return Err(ChannelError::SendBufferFull);
        }

        queue.push_back(QueuedPacket {
            sequence,
            data,
            sent_time: std::time::Instant::now(),
            retry_count: 0,
            acked: false,
        });

        self.stats.lock().packets_sent += 1;
        Ok(sequence)
    }

    pub fn get_pending_packets(&self, max_packets: usize) -> Vec<(u32, Vec<u8>)> {
        let mut queue = self.send_queue.lock();
        let mut result = Vec::new();
        
        for packet in queue.iter_mut().take(max_packets) {
            if !packet.acked {
                result.push((packet.sequence, packet.data.clone()));
                packet.sent_time = std::time::Instant::now();
                packet.retry_count += 1;
                if packet.retry_count > 1 {
                    self.stats.lock().packets_retransmitted += 1;
                }
            }
        }
        
        result
    }

    pub fn handle_ack(&self, ack: u32, ack_bitfield: u32) {
        let mut queue = self.send_queue.lock();
        let mut pending = self.pending_acks.lock();
        let mut bitfield = self.ack_bitfield.lock();
        
        *bitfield = ack_bitfield;
        
        while let Some(&front_seq) = pending.front() {
            if front_seq <= ack || (ack_bitfield & (1 << (front_seq.wrapping_sub(ack) & 31))) != 0 {
                if let Some(idx) = queue.iter().position(|p| p.sequence == front_seq) {
                    queue[idx].acked = true;
                }
                pending.pop_front();
            } else {
                break;
            }
        }

        queue.retain(|p| !p.acked);
    }

    pub fn receive_packet(&self, sequence: u32, data: Vec<u8>) -> Result<Option<Vec<u8>>, ChannelError> {
        let mut buffer = self.receive_buffer.lock();
        let mut remote_seq = self.remote_sequence.lock();
        let mut pending = self.pending_acks.lock();
        let mut stats = self.stats.lock();

        if sequence <= *remote_seq {
            return Ok(None);
        }

        *remote_seq = sequence;
        stats.packets_received += 1;
        stats.bytes_received += data.len() as u64;

        match self.config.channel_type {
            ChannelType::Unreliable => {
                Ok(Some(data))
            }
            ChannelType::UnreliableSequenced => {
                if sequence == *remote_seq {
                    Ok(Some(data))
                } else {
                    stats.packets_lost += (sequence - *remote_seq - 1) as u64;
                    Ok(None)
                }
            }
            ChannelType::ReliableUnordered => {
                buffer.insert(sequence, ReceivedPacket { sequence, data: data.clone() });
                pending.push_back(sequence);
                Ok(Some(data))
            }
            ChannelType::ReliableOrdered => {
                buffer.insert(sequence, ReceivedPacket { sequence, data: data.clone() });
                pending.push_back(sequence);
                
                let mut output = Vec::new();
                let mut expected = *remote_seq;
                
                loop {
                    expected = expected.wrapping_add(1);
                    if let Some(packet) = buffer.remove(&expected) {
                        output.push(packet.data);
                    } else {
                        break;
                    }
                }
                
                if output.is_empty() {
                    Ok(None)
                } else if output.len() == 1 {
                    Ok(Some(output.into_iter().next().unwrap()))
                } else {
                    let mut combined = Vec::new();
                    for d in output {
                        combined.extend_from_slice(&d);
                    }
                    Ok(Some(combined))
                }
            }
        }
    }

    pub fn get_ack_info(&self) -> (u32, u32) {
        let remote_seq = *self.remote_sequence.lock();
        let bitfield = *self.ack_bitfield.lock();
        
        let mut pending = self.pending_acks.lock();
        while let Some(&front) = pending.front() {
            if front <= remote_seq || (bitfield & (1 << (front.wrapping_sub(remote_seq) & 31))) != 0 {
                pending.pop_front();
            } else {
                break;
            }
        }
        
        let mut new_bitfield = 0u32;
        for &seq in pending.iter() {
            let diff = seq.wrapping_sub(remote_seq);
            if diff < 32 {
                new_bitfield |= 1 << diff;
            }
        }
        
        *self.ack_bitfield.lock() = new_bitfield;
        (remote_seq, new_bitfield)
    }

    pub fn clear_send_queue(&self) {
        self.send_queue.lock().clear();
        self.pending_acks.lock().clear();
    }

    pub fn clear_receive_buffer(&self) {
        self.receive_buffer.lock().clear();
    }

    pub fn stats(&self) -> ChannelStats {
        self.stats.lock().clone()
    }

    pub fn pending_count(&self) -> usize {
        self.send_queue.lock().len()
    }

    pub fn update_rtt(&self, rtt: std::time::Duration) {
        self.stats.lock().avg_rtt = rtt.as_secs_f32();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("Packet too large")]
    PacketTooLarge,
    #[error("Send buffer full")]
    SendBufferFull,
    #[error("Receive buffer full")]
    ReceiveBufferFull,
    #[error("Invalid sequence")]
    InvalidSequence,
}