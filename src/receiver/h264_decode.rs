use crate::receiver::frame_decoder::FrameDecoder;
use openh264::OpenH264API;
use openh264::decoder::{Decoder, DecoderConfig};
use openh264::formats::YUVSource;
use std::error::Error;

pub struct H264FrameDecoder {
    decoder: Decoder,
    rgb_scratch: Vec<u8>,
    pixel_buffer: Vec<u32>,
}

impl H264FrameDecoder {
    pub fn new(width: usize, height: usize) -> Result<Self, Box<dyn Error>> {
        let api = OpenH264API::from_source();
        let config = DecoderConfig::new().debug(true);
        let decoder = Decoder::with_api_config(api, config)
            .map_err(|e| format!("Failed to create OpenH264 decoder: {:?}", e))?;

        Ok(Self {
            decoder,
            rgb_scratch: Vec::new(),
            pixel_buffer: vec![0u32; width * height],
        })
    }
}

impl FrameDecoder for H264FrameDecoder {
    fn decode_frame(&mut self, payload: &[u8]) -> Result<Option<Vec<u32>>, Box<dyn Error>> {
        match self.decoder.decode(payload) {
            Ok(Some(yuv)) => {
                // Resize scratch buffer to hold raw 24-bit RGB8 (R, G, B per pixel)
                let rgb_len = yuv.rgb8_len(); //
                if self.rgb_scratch.len() != rgb_len {
                    self.rgb_scratch.resize(rgb_len, 0);
                }

                // Write decoded YUV image directly into RGB8 buffer using OpenH264
                yuv.write_rgb8(&mut self.rgb_scratch); //[cite: 2]

                // Pack RGB triplets into minifb's 0x00RRGGBB u32 format
                for (chunk, pixel) in self
                    .rgb_scratch
                    .chunks_exact(3)
                    .zip(self.pixel_buffer.iter_mut())
                {
                    let r = chunk[0] as u32;
                    let g = chunk[1] as u32;
                    let b = chunk[2] as u32;
                    *pixel = (r << 16) | (g << 8) | b;
                }

                Ok(Some(self.pixel_buffer.clone())) //[cite: 1]
            }
            Ok(None) => Ok(None), //[cite: 1]
            Err(e) => {
                log::error!("H.264 decode error: {:?}", e); //[cite: 1]
                Err(format!("H.264 decode error: {:?}", e).into()) //[cite: 1]
            }
        }
    }

    fn reset(&mut self) {
        if let Ok(new_decoder) =
            Decoder::with_api_config(OpenH264API::from_source(), DecoderConfig::default())
        {
            //[cite: 1]
            self.decoder = new_decoder; //[cite: 1]
        }
    }
}
