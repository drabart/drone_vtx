use super::yuv_convert::yuyv_to_yuv420p;
use crate::config::{HEIGHT, WIDTH};
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, EncodedBitStream, Encoder, EncoderConfig, FrameRate, IntraFramePeriod,
};
use openh264::formats::YUVSlices;

pub struct H264Encoder {
    encoder: Encoder,
}

impl H264Encoder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Initialize the H.264 encoder
        let config = EncoderConfig::new()
            .max_frame_rate(FrameRate::from_hz(30.0))
            .bitrate(BitRate::from_bps(2_500_000))
            .intra_frame_period(IntraFramePeriod::from_num_frames(30))
            .skip_frames(false)
            .debug(false);
        let api: OpenH264API = OpenH264API::from_source();
        let encoder = Encoder::with_api_config(api, config)
            .map_err(|e| format!("Failed to create encoder: {:?}", e))?;

        Ok(Self { encoder })
    }

    pub fn encode(
        &mut self,
        yuyv_bytes: &[u8],
    ) -> Result<EncodedBitStream, Box<dyn std::error::Error>> {
        let (width, height) = (WIDTH, HEIGHT);
        // Buffers for Planar YUV420P
        let y_len = WIDTH * HEIGHT;
        let uv_len = (WIDTH / 2) * (HEIGHT / 2);

        let mut y_plane = vec![0u8; y_len];
        let mut u_plane = vec![0u8; uv_len];
        let mut v_plane = vec![0u8; uv_len];

        // Convert YUYV (Packed 4:2:2) -> YUV420P (Planar 4:2:0) using pre-allocated vectors
        yuyv_to_yuv420p(
            yuyv_bytes,
            &mut y_plane,
            &mut u_plane,
            &mut v_plane,
            width,
            height,
        );

        // Wrap references in YUVSlices
        let yuv_slices = YUVSlices::new(
            (&y_plane, &u_plane, &v_plane),
            (width, height),
            (width, width / 2, width / 2),
        );

        // Encode to H.264
        let bitstream = self.encoder.encode(&yuv_slices)?;
        Ok(bitstream)
    }
}
