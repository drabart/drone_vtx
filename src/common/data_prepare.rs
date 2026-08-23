use crate::common::config::*;
use crate::common::vtx_packet::{VtxPacket, VtxPacketHeader};
use reed_solomon_erasure::galois_8::ReedSolomon;
use std::fmt;

// Calculate shard data payload size by filling remaining packet space
pub const PACKET_PAYLOAD_SIZE: usize = MAX_PACKET_PAYLOAD_SIZE - VtxPacketHeader::SIZE;
pub type Shard = Vec<u8>;
pub type Chunk = Vec<u8>;
pub type Frame = Vec<u8>;

#[derive(Debug)]
pub enum AssemblerError {
    PacketParseError(&'static str),
    InvalidShardId {
        shard_id: usize,
        max_allowed: usize,
    },
    InvalidChunkId {
        chunk_id: usize,
        max_allowed: usize,
    },
    ChunkDecodeFailed {
        chunk_id: usize,
        cause: String,
    },
    ChunkMissing {
        chunk_id_prev: usize,
        chunk_id_next: usize,
    },
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
            Self::InvalidChunkId {
                chunk_id,
                max_allowed,
            } => {
                write!(
                    f,
                    "Chunk ID {} exceeds total expected chunks ({})",
                    chunk_id, max_allowed
                )
            }
            Self::ChunkMissing {
                chunk_id_prev,
                chunk_id_next,
            } => {
                write!(
                    f,
                    "Missing chunk(s) between {} and {}",
                    chunk_id_prev, chunk_id_next
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
    reed_solomon: ReedSolomon,
    previous_frame_id: i32,
    previous_chunk_id: i32,
    chunks_ok: usize,
    total_shards_expected: usize,
    total_shards_received: usize,
    chunk_buffer: [Option<Shard>; CHUNK_SHARDS],
    frame_buffer: Vec<Option<Chunk>>,
}

impl DataSharder {
    pub fn new() -> Self {
        Self {
            reed_solomon: ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)
                .expect("Failed to create Reed-Solomon encoder"),
            previous_frame_id: -1,
            previous_chunk_id: -1,
            chunks_ok: 0,
            total_shards_expected: 0,
            total_shards_received: 0,
            chunk_buffer: Default::default(),
            frame_buffer: Vec::new(),
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

    pub fn process_shard(&mut self, raw_payload: &[u8]) -> Result<Option<Frame>, AssemblerError> {
        // Convert raw bytes safely into structured VtxPacket
        let packet = VtxPacket::try_from(raw_payload).map_err(AssemblerError::PacketParseError)?;

        let frame_id = packet.header.frame_id as usize;
        let chunk_id = packet.header.chunk_id as usize;
        let chunk_count = packet.header.chunk_count as usize; // Expected total chunks for this frame
        let shard_id = packet.header.shard_id as usize;
        let shard_count = packet.header.shard_count as usize; // Expected total shards for this chunk

        log::info!(
            "[*] Frame {}: Received shard {} of chunk {} ({} shards expected)",
            frame_id,
            shard_id,
            chunk_id,
            shard_count
        );

        if shard_id >= shard_count || shard_id >= CHUNK_SHARDS {
            return Err(AssemblerError::InvalidShardId {
                shard_id,
                max_allowed: shard_count,
            });
        }

        if chunk_id >= chunk_count {
            return Err(AssemblerError::InvalidChunkId {
                chunk_id,
                max_allowed: chunk_count,
            });
        }

        if frame_id != self.previous_frame_id as usize {
            // Shard belongs to a new frame, process the previous frame's chunk buffer if it exists
            let previous_frame_result: Option<Frame> = self.process_previous_frame()?;

            self.chunks_ok = 0;
            self.total_shards_expected = 0;
            self.total_shards_received = 0;
            self.previous_frame_id = frame_id as i32;
            self.frame_buffer.clear();
            self.frame_buffer.resize(chunk_count, None);

            self.previous_chunk_id = chunk_id as i32;
            self.chunk_buffer = Default::default();
            return Ok(previous_frame_result);
        } else if chunk_id != self.previous_chunk_id as usize {
            // Shard belongs to a new chunk within the same frame, process the previous chunk's buffer if it exists
            self.process_chunk_buffer()?;

            self.previous_chunk_id = chunk_id as i32;
            self.chunk_buffer = Default::default();
            return Ok(None);
        }

        self.chunk_buffer[shard_id] = Some(packet.payload.to_vec());

        Ok(None)
    }

    fn process_previous_frame(&mut self) -> Result<Option<Frame>, AssemblerError> {
        if self.previous_frame_id == -1 {
            return Ok(None);
        }

        self.process_chunk_buffer()?;

        if self.chunks_ok != self.frame_buffer.len() {
            return Err(AssemblerError::ChunkMissing {
                chunk_id_prev: self.previous_chunk_id as usize,
                chunk_id_next: self.frame_buffer.len(),
            });
        }

        Ok(Some(
            self.frame_buffer
                .clone()
                .into_iter()
                .flatten()
                .flatten()
                .collect(),
        ))
    }

    fn process_chunk_buffer(&mut self) -> Result<(), AssemblerError> {
        if self.previous_chunk_id == -1 {
            return Ok(());
        }

        log::info!(
            "[*] Frame: Processing chunk {} ({} shards received, {} expected)",
            self.previous_chunk_id,
            self.chunk_buffer.iter().filter(|s| s.is_some()).count(),
            CHUNK_SHARDS
        );

        let received_count = self.chunk_buffer.iter().filter(|s| s.is_some()).count();

        self.total_shards_expected += CHUNK_SHARDS;
        self.total_shards_received += received_count;

        let result = self
            .decode_chunk()
            .map_err(|err| AssemblerError::ChunkDecodeFailed {
                chunk_id: self.previous_chunk_id as usize,
                cause: err.to_string(),
            });

        // Reset buffer state
        for shard in self.chunk_buffer.iter_mut() {
            *shard = None;
        }

        match result {
            Ok(bytes) => {
                self.chunks_ok += 1;
                self.frame_buffer[self.previous_chunk_id as usize] = Some(bytes);
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn decode_chunk(&mut self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Repair missing data and parity shards in place
        self.reed_solomon.reconstruct(&mut self.chunk_buffer)?;

        // Assemble only the DATA_SHARDS (ignoring PARITY_SHARDS) into one byte stream
        let mut chunk_bytes = Vec::new();
        for shard in self.chunk_buffer.iter().take(DATA_SHARDS) {
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
}
