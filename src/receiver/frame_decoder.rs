use std::error::Error;

/// Strategy interface for decoding complete frame buffers
pub trait FrameDecoder {
    /// Consumes a raw reconstructed payload and attempts to produce a decoded frame buffer
    fn decode_frame(&mut self, payload: &[u8]) -> Result<Option<Vec<u32>>, Box<dyn Error>>;

    /// Resets decoder state (e.g., clearing frame buffers on packet loss)
    fn reset(&mut self);
}
