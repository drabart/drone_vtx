mod h264_encode;
mod yuv_convert;

use crate::config::{HEIGHT, WIDTH};
use crate::data_prepare::process_frame_into_chunks;
use crate::network_send::send_video_frame;
use crate::transmitter::h264_encode::H264Encoder;
use std::thread;
use std::time::Instant;
use v4l::{
    buffer::Type,
    device::Device,
    format::FourCC,
    io::{mmap::Stream, traits::CaptureStream},
    video::Capture,
};

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
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();

        let socket_fd = self.socket_fd;

        let _sender_thread = thread::spawn(move || {
            let mut frame_counter = 0;
            while let Ok(frame) = rx.recv() {
                log::info!("[*] Received frame");

                let encoded_chunks =
                    process_frame_into_chunks(frame_counter, &frame).expect("Failed to split");

                send_video_frame(socket_fd, encoded_chunks).expect("Failed to send");

                frame_counter += 1;
            }
        });

        loop {
            self.process_frame(&tx)?;
        }
    }

    fn process_frame(
        &mut self,
        tx: &std::sync::mpsc::Sender<Vec<u8>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (raw_frame, _meta) = self.stream.next()?;

        let start_time = Instant::now();

        let bitstream = self.encoder.encode(raw_frame)?;
        let bitstream_vec = bitstream.to_vec();
        log::info!(
            "[Frame #{}] Encoded H.264 bitstream size: {} bytes",
            self.frame_counter,
            bitstream.raw_info().iFrameSizeInBytes
        );

        let elapsed = start_time.elapsed();

        log::info!(
            "[Frame #{}] Encode Time: {:.2?}",
            self.frame_counter,
            elapsed,
        );

        tx.send(bitstream_vec)?;

        self.frame_counter += 1;
        Ok(())
    }
}
