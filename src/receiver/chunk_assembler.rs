use crate::config::*;
use crate::data_prepare::decode_chunk;

pub struct ChunkAssembler {
    last_frame_id: i32,
    last_chunk_id: i32,
    chunks_ok: i32,
    total_shards_expected: usize,
    total_shards_received: usize,
    current_chunk: [Option<Vec<u8>>; CHUNK_SHARDS],
    jpeg_buffer: Vec<u8>,
}

impl Default for ChunkAssembler {
    fn default() -> Self {
        Self {
            last_frame_id: -1,
            last_chunk_id: -1,
            chunks_ok: 0,
            total_shards_expected: 0,
            total_shards_received: 0,
            current_chunk: Default::default(),
            jpeg_buffer: Vec::new(),
        }
    }
}

impl ChunkAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process_shard(&mut self, payload: &[u8]) -> Option<Vec<u8>> {
        if payload.len() < 8 {
            return None;
        }

        let frame_id = payload[1] as u32;
        let chunk_id = payload[2] as usize;
        let shard_id = payload[3] as usize;

        if shard_id >= CHUNK_SHARDS {
            log::warn!("Received invalid shard_id: {}", shard_id);
            return None;
        }

        let mut completed_jpeg_frame = None;

        // 1. Check for Frame transition
        if frame_id != self.last_frame_id as u32 {
            if self.last_frame_id != -1 {
                // Flush the final chunk of the previous frame
                if self.last_chunk_id != -1 {
                    self.flush_and_decode_chunk();
                }

                // Extract completed frame if we have collected data
                if !self.jpeg_buffer.is_empty() {
                    completed_jpeg_frame = Some(std::mem::take(&mut self.jpeg_buffer));
                }

                let frame_loss_pct = if self.total_shards_expected > 0 {
                    100.0
                        * (1.0
                            - (self.total_shards_received as f32
                                / self.total_shards_expected as f32))
                } else {
                    0.0
                };

                log::info!(
                    "[*] Frame {} completed. Chunks decoded: {}/{}. Total Shard Loss: {:.2}% ({}/{} shards)",
                    self.last_frame_id,
                    self.chunks_ok,
                    if self.last_chunk_id >= 0 {
                        self.last_chunk_id + 1
                    } else {
                        0
                    },
                    frame_loss_pct,
                    self.total_shards_expected
                        .saturating_sub(self.total_shards_received),
                    self.total_shards_expected
                );
            }

            self.chunks_ok = 0;
            self.total_shards_expected = 0;
            self.total_shards_received = 0;
            self.last_frame_id = frame_id as i32;
            self.last_chunk_id = chunk_id as i32; // Set directly to current chunk_id
        } else if chunk_id != self.last_chunk_id as usize {
            // 2. Check for Chunk transition within the same frame
            if self.last_chunk_id != -1 {
                self.flush_and_decode_chunk();
            }
            self.last_chunk_id = chunk_id as i32;
        }

        // 3. Store shard payload (strip 4-byte header and 4-byte trailing FCS)
        let end_idx = payload.len() - 4;
        if end_idx >= 4 {
            self.current_chunk[shard_id] = Some(payload[4..end_idx].to_vec());
        }

        completed_jpeg_frame
    }

    fn flush_and_decode_chunk(&mut self) {
        let received_count = self.current_chunk.iter().filter(|s| s.is_some()).count();
        let missing_count = CHUNK_SHARDS.saturating_sub(received_count);
        let chunk_loss_pct = 100.0 * (missing_count as f32 / CHUNK_SHARDS as f32);

        // Accumulate overall frame statistics
        self.total_shards_expected += CHUNK_SHARDS;
        self.total_shards_received += received_count;

        match decode_chunk(&mut self.current_chunk) {
            Ok(decoded_bytes) => {
                self.chunks_ok += 1;

                // Append the reconstructed chunk data to the overall JPEG buffer
                self.jpeg_buffer.extend_from_slice(&decoded_bytes);

                log::debug!(
                    "Chunk {} decoded OK ({} bytes appended). Shards: {}/{} (Loss: {:.1}%)",
                    self.last_chunk_id,
                    decoded_bytes.len(),
                    received_count,
                    CHUNK_SHARDS,
                    chunk_loss_pct
                );
            }
            Err(err) => {
                log::error!(
                    "Failed decoding Chunk {}: {:?}. Received {}/{} shards (min required: {}, Loss: {:.1}%)",
                    self.last_chunk_id,
                    err,
                    received_count,
                    CHUNK_SHARDS,
                    DATA_SHARDS,
                    chunk_loss_pct
                );
            }
        }

        // Reset buffer for the next chunk
        for shard in self.current_chunk.iter_mut() {
            *shard = None;
        }
    }
}
