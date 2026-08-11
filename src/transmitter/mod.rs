use crate::data_prepare::process_frame_into_chunks;
use crate::network_send::send_frame;
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
    frame_counter: u32,
}

impl<'a> VideoTransmitter<'a> {
    pub fn connect(dev_index: usize, socket_fd: i32) -> Result<Self, Box<dyn std::error::Error>> {
        let dev = Device::new(dev_index)?;

        let mut fmt = dev.format()?;
        fmt.width = 640;
        fmt.height = 480;
        fmt.fourcc = FourCC::new(b"MJPG");

        let set_fmt = dev.set_format(&fmt)?;
        log::info!(
            "[*] Camera initialized: {}x{} ({})",
            set_fmt.width,
            set_fmt.height,
            set_fmt.fourcc
        );

        let stream = Stream::with_buffers(&dev, Type::VideoCapture, 4)?;

        Ok(Self {
            socket_fd,
            stream,
            frame_counter: 0,
        })
    }

    pub fn transmit_next_frame(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (buf, _meta) = self.stream.next()?;

        let start_time = Instant::now();
        let encoded_chunks = process_frame_into_chunks(self.frame_counter, &buf)?;

        // 1. Blast all shards for the entire video frame while accumulating stats in RAM
        for chunk in encoded_chunks {
            for shard in chunk {
                send_frame(self.socket_fd, shard)?;
                thread::sleep(std::time::Duration::from_micros(2400));
            }
        }

        let elapsed = start_time.elapsed();

        log::info!("[Frame #{}] Tx Time: {:.2?}", self.frame_counter, elapsed,);

        self.frame_counter = self.frame_counter.wrapping_add(1);
        Ok(())
    }
}
