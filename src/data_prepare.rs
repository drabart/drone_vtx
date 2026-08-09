use reed_solomon_erasure::galois_8::ReedSolomon;

const DATA_SHARDS: usize = 10;
const PARITY_SHARDS: usize = 4;
const PACKET_PAYLOAD_SIZE: usize = 1478; // Maximum payload size

pub fn process_data_into_chunks(
    data: &[u8],
) -> Result<Vec<Vec<Vec<u8>>>, Box<dyn std::error::Error>> {
    let mut encoded_chunks: Vec<Vec<Vec<u8>>> = Vec::new();

    for chunk in data.chunks(DATA_SHARDS * PACKET_PAYLOAD_SIZE) {
        let encoded_chunk = encode_chunk(chunk)?;
        encoded_chunks.push(encoded_chunk);
    }

    Ok(encoded_chunks)
}

pub fn encode_chunk(data: &[u8]) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let mut shards = vec![vec![0u8; PACKET_PAYLOAD_SIZE]; DATA_SHARDS + PARITY_SHARDS];

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
