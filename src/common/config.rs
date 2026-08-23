pub const MAX_PACKET_PAYLOAD_SIZE: usize = 1480;
pub const DATA_SHARDS: usize = 10;
pub const PARITY_SHARDS: usize = 4;
pub const CHUNK_SHARDS: usize = DATA_SHARDS + PARITY_SHARDS;

pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 480;
