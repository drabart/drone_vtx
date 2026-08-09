use crate::config::*;
use reed_solomon_erasure::galois_8::ReedSolomon;

const MAX_PACKET_PAYLOAD_SIZE: usize = 1480; // Maximum payload size
const PACKET_PAYLOAD_SIZE: usize = MAX_PACKET_PAYLOAD_SIZE - 3;

pub fn process_frame_into_chunks(
    frame_id: u32,
    data: &[u8],
) -> Result<Vec<Vec<Vec<u8>>>, Box<dyn std::error::Error>> {
    let mut encoded_chunks: Vec<Vec<Vec<u8>>> = Vec::new();

    for (chunk_id, chunk) in data.chunks(DATA_SHARDS * PACKET_PAYLOAD_SIZE).enumerate() {
        let encoded_chunk = encode_chunk(chunk)?;

        let framed_chunk: Vec<Vec<u8>> = encoded_chunk
            .into_iter()
            .enumerate()
            .map(|(shard_id, shard)| {
                let mut packet = Vec::with_capacity(3 + shard.len());
                packet.push((frame_id % 256) as u8);
                packet.push(chunk_id as u8);
                packet.push(shard_id as u8);
                packet.extend_from_slice(&shard);
                packet
            })
            .collect();

        encoded_chunks.push(framed_chunk);
    }

    log::info!(
        "[*] Frame {} processed into {} chunks ({} bytes each)",
        frame_id,
        encoded_chunks.len(),
        CHUNK_SHARDS * PACKET_PAYLOAD_SIZE
    );

    Ok(encoded_chunks)
}

pub fn encode_chunk(data: &[u8]) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let mut shards = vec![vec![0u8; PACKET_PAYLOAD_SIZE]; CHUNK_SHARDS];

    for i in 0..DATA_SHARDS {
        let start = i * PACKET_PAYLOAD_SIZE;
        let mut end = start + PACKET_PAYLOAD_SIZE;
        if end > data.len() {
            end = data.len();
        }

        for j in start..end {
            shards[i][j - start] = data[j];
        }
    }

    let r = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    r.encode(&mut shards).unwrap();

    Ok(shards)
}

pub fn decode_chunk(chunk: &mut [Option<Vec<u8>>]) -> Result<(), Box<dyn std::error::Error>> {
    let r = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    r.reconstruct(chunk)?;

    Ok(())
}
