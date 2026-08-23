use crate::common::config::*;
use crate::common::vtx_packet::{VtxPacket, VtxPacketHeader};
use reed_solomon_erasure::galois_8::ReedSolomon;
use std::fmt;

// Calculate shard data payload size by filling remaining packet space
pub const PACKET_PAYLOAD_SIZE: usize = MAX_PACKET_PAYLOAD_SIZE - VtxPacketHeader::SIZE;

#[derive(Debug)]
pub enum AssemblerError {
    PacketParseError(&'static str),
    InvalidShardId { shard_id: usize, max_allowed: usize },
    ChunkDecodeFailed { chunk_id: usize, cause: String },
}

impl fmt::Display for AssemblerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PacketParseError(err) => write!(f, "Packet parsing failed: {}", err),
            Self::InvalidShardId {
                shard_id,
                max_allowed,
            } => {
                write!(
                    f,
                    "Shard ID {} exceeds total expected shards ({})",
                    shard_id, max_allowed
                )
            }
            Self::ChunkDecodeFailed { chunk_id, cause } => {
                write!(f, "Failed decoding chunk {}: {}", chunk_id, cause)
            }
        }
    }
}

impl std::error::Error for AssemblerError {}

#[derive(Debug, Clone)]
pub struct DataSharder {
    frame_id: u32,
    reed_solomon: ReedSolomon,
    last_frame_id: i32,
    last_chunk_id: i32,
    chunks_ok: usize,
    total_shards_expected: usize,
    total_shards_received: usize,
    current_chunk: [Option<Vec<u8>>; CHUNK_SHARDS],
}

impl DataSharder {
    pub fn new() -> Self {
        Self {
            frame_id: 0,
            reed_solomon: ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)
                .expect("Failed to create Reed-Solomon encoder"),
            last_frame_id: -1,
            last_chunk_id: -1,
            chunks_ok: 0,
            total_shards_expected: 0,
            total_shards_received: 0,
            current_chunk: Default::default(),
        }
    }

    pub fn process_frame_into_packets(
        &mut self,
        frame_id: u32,
        data: &[u8],
    ) -> Result<Vec<Vec<VtxPacket>>, Box<dyn std::error::Error>> {
        let mut encoded_chunks: Vec<Vec<VtxPacket>> = Vec::new();

        let chunks: Vec<&[u8]> = data.chunks(DATA_SHARDS * PACKET_PAYLOAD_SIZE).collect();
        let chunk_count = chunks.len() as u8;

        for (chunk_id, chunk) in chunks.into_iter().enumerate() {
            let encoded_chunk = self.encode_chunk(chunk)?;
            let shard_count = encoded_chunk.len() as u8;

            let framed_chunk: Vec<VtxPacket> = encoded_chunk
                .into_iter()
                .enumerate()
                .map(|(shard_id, shard)| {
                    let header = VtxPacketHeader {
                        command: 0x02,
                        frame_id: (frame_id % 256) as u8,
                        chunk_id: chunk_id as u8,
                        chunk_count,
                        shard_id: shard_id as u8,
                        shard_count,
                    };

                    VtxPacket {
                        header,
                        payload: shard,
                    }
                })
                .collect();

            encoded_chunks.push(framed_chunk);
        }

        log::info!(
            "[*] Frame {} processed into {} chunks",
            frame_id,
            encoded_chunks.len(),
        );

        Ok(encoded_chunks)
    }

    fn encode_chunk(&self, data: &[u8]) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
        let mut shards = vec![vec![0u8; PACKET_PAYLOAD_SIZE]; CHUNK_SHARDS];

        for i in 0..DATA_SHARDS {
            let start = i * PACKET_PAYLOAD_SIZE;
            let mut end = start + PACKET_PAYLOAD_SIZE;
            if end > data.len() {
                end = data.len();
            }

            if start < data.len() {
                shards[i][..end - start].copy_from_slice(&data[start..end]);
            }
        }

        self.reed_solomon.encode(&mut shards)?;

        Ok(shards)
    }

    fn decode_chunk(&mut self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Repair missing data and parity shards in place
        self.reed_solomon.reconstruct(&mut self.current_chunk)?;

        // Assemble only the DATA_SHARDS (ignoring PARITY_SHARDS) into one byte stream
        let mut chunk_bytes = Vec::new();
        for shard in self.current_chunk.iter().take(DATA_SHARDS) {
            if let Some(data) = shard {
                chunk_bytes.extend_from_slice(data);
            } else {
                return Err(
                    "Reconstruction reported success, but a data shard was still None".into(),
                );
            }
        }

        Ok(chunk_bytes)
    }

    pub fn process_shard(&mut self, raw_payload: &[u8]) -> Result<Option<Vec<u8>>, AssemblerError> {
        // Convert raw bytes safely into structured VtxPacket
        let packet = VtxPacket::try_from(raw_payload).map_err(AssemblerError::PacketParseError)?;

        let frame_id = packet.header.frame_id as usize;
        let chunk_id = packet.header.chunk_id as usize;
        let shard_id = packet.header.shard_id as usize;
        let shard_count = packet.header.shard_count as usize; // Expected total shards for this chunk

        if shard_id >= shard_count || shard_id >= CHUNK_SHARDS {
            return Err(AssemblerError::InvalidShardId {
                shard_id,
                max_allowed: shard_count,
            });
        }

        let mut decoded_bytes = None;

        // 1. Check for Frame transition
        if frame_id != self.last_frame_id as usize {
            if self.last_frame_id != -1 && self.last_chunk_id != -1 {
                decoded_bytes = self.flush_and_decode_chunk()?;
            }

            self.chunks_ok = 0;
            self.total_shards_expected = 0;
            self.total_shards_received = 0;
            self.last_frame_id = frame_id as i32;
            self.last_chunk_id = chunk_id as i32;
        } else if chunk_id != self.last_chunk_id as usize {
            // 2. Check for Chunk transition within the same frame
            if self.last_chunk_id != -1 {
                decoded_bytes = self.flush_and_decode_chunk()?;
            }
            self.last_chunk_id = chunk_id as i32;
        }

        // 3. Store shard payload
        self.current_chunk[shard_id] = Some(packet.payload.to_vec());

        Ok(decoded_bytes)
    }

    fn flush_and_decode_chunk(&mut self) -> Result<Option<Vec<u8>>, AssemblerError> {
        let received_count = self.current_chunk.iter().filter(|s| s.is_some()).count();
        if received_count == 0 {
            return Ok(None);
        }

        self.total_shards_expected += CHUNK_SHARDS;
        self.total_shards_received += received_count;

        let result = self
            .decode_chunk()
            .map_err(|err| AssemblerError::ChunkDecodeFailed {
                chunk_id: self.last_chunk_id as usize,
                cause: err.to_string(),
            });

        // Reset buffer state
        for shard in self.current_chunk.iter_mut() {
            *shard = None;
        }

        match result {
            Ok(bytes) => {
                self.chunks_ok += 1;
                Ok(Some(bytes))
            }
            Err(err) => Err(err),
        }
    }
}
