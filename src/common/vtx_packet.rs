use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(Debug, Clone, Copy, KnownLayout, Immutable, IntoBytes, FromBytes)]
#[repr(C, packed)]
pub struct VtxPacketHeader {
    pub command: u8,
    pub frame_id: u8,
    pub chunk_id: u8,
    pub chunk_count: u8,
    pub shard_id: u8,
    pub shard_count: u8,
}

impl VtxPacketHeader {
    pub const SIZE: usize = std::mem::size_of::<Self>(); // 6 bytes
}

#[derive(Debug, Clone)]
pub struct VtxPacket {
    pub header: VtxPacketHeader,
    pub payload: Vec<u8>,
}

impl VtxPacket {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(VtxPacketHeader::SIZE + self.payload.len());
        buf.extend_from_slice(self.header.as_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }
}

impl TryFrom<&[u8]> for VtxPacket {
    type Error = &'static str;

    /// Attempts to parse raw frame payload bytes into a structured VtxPacket wrapper
    fn try_from(payload: &[u8]) -> Result<Self, Self::Error> {
        // Must contain at least header size + 4-byte trailing FCS
        if payload.len() < VtxPacketHeader::SIZE + 4 {
            return Err("Payload buffer too short for VtxPacket Header and FCS");
        }

        // Extract header using zerocopy parsing
        let header = VtxPacketHeader::read_from_bytes(&payload[..VtxPacketHeader::SIZE])
            .map_err(|_| "Failed to parse VtxPacketHeader layout")?;

        // Slice out inner payload body (strip header and 4-byte trailing FCS)
        let end_idx = payload.len() - 4;
        let body = &payload[VtxPacketHeader::SIZE..end_idx];

        Ok(VtxPacket {
            header,
            payload: body.to_vec(),
        })
    }
}
