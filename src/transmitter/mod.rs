mod yuv_convert;

use crate::data_prepare::process_frame_into_chunks;
use crate::network_send::send_frame;
use openh264::OpenH264API;
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod};
use openh264::formats::YUVSlices;
use std::thread;
use std::time::Instant;
use v4l::{
    buffer::Type,
    device::Device,
    format::FourCC,
    io::{mmap::Stream, traits::CaptureStream},
    video::Capture,
};
use yuv_convert::yuyv_to_yuv420p;

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

pub struct VideoTransmitter<'a> {
    socket_fd: i32,
    stream: Stream<'a>,
    encoder: H264Encoder,
    frame_counter: u32,
}

impl<'a> VideoTransmitter<'a> {
    pub fn connect(dev_index: usize, socket_fd: i32) -> Result<Self, Box<dyn std::error::Error>> {
        let dev = Device::new(dev_index)?;

        let mut fmt = dev.format()?;
        fmt.width = WIDTH as u32;
        fmt.height = HEIGHT as u32;
        fmt.fourcc = FourCC::new(b"YUYV");

        let set_fmt = dev.set_format(&fmt)?;
        log::info!(
            "[*] Camera initialized: {}x{} ({})",
            set_fmt.width,
            set_fmt.height,
            set_fmt.fourcc
        );

        let stream = Stream::with_buffers(&dev, Type::VideoCapture, 1)?;
        let encoder = H264Encoder::new()?;

        // Return the VideoTransmitter instance
        Ok(Self {
            socket_fd,
            stream,
            encoder,
            frame_counter: 0,
        })
    }

    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            self.transmit_next_frame()?;
        }
    }

    fn transmit_next_frame(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (buf, _meta) = self.stream.next()?;

        let start_time = Instant::now();
        let encoded_chunks = process_frame_into_chunks(self.frame_counter, &buf)?;

        let bitstream = self.encoder.do_h264_stuff(&buf)?;
        log::info!(
            "[Frame #{}] Encoded H.264 bitstream size: {} bytes",
            self.frame_counter,
            bitstream.len()
        );

        // 1. Blast all shards for the entire video frame while accumulating stats in RAM
        // for chunk in encoded_chunks {
        //     for shard in chunk {
        //         send_frame(self.socket_fd, shard)?;
        //         thread::sleep(std::time::Duration::from_micros(2400));
        //     }
        // }

        let elapsed = start_time.elapsed();

        log::info!("[Frame #{}] Tx Time: {:.2?}", self.frame_counter, elapsed,);

        self.frame_counter = self.frame_counter.wrapping_add(1);
        Ok(())
    }
}

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
            .debug(true);
        let api: OpenH264API = OpenH264API::from_source();
        let encoder = Encoder::with_api_config(api, config)
            .map_err(|e| format!("Failed to create encoder: {:?}", e))?;

        Ok(Self { encoder })
    }

    pub fn do_h264_stuff(
        &mut self,
        yuyv_bytes: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
        Ok(bitstream.to_vec())
    }
}
