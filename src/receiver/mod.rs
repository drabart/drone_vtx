mod chunk_assembler;
mod vtx_packet;

use chunk_assembler::ChunkAssembler;
use image::load_from_memory_with_format;
use minifb::{Window, WindowOptions};
use std::io::Error;
use std::sync::mpsc::channel;
use std::thread::{self, JoinHandle};
use vtx_packet::VtxPacket;

pub struct VideoReceiver {
    socket_fd: i32,
    target_mac: [u8; 6],
    width: usize,
    height: usize,
}

impl VideoReceiver {
    pub fn new(socket_fd: i32, target_mac: [u8; 6]) -> Self {
        Self {
            socket_fd,
            target_mac,
            width: 640,
            height: 480,
        }
    }

    pub fn start(self) -> Result<(), Box<dyn std::error::Error>> {
        // Channel to send raw packet bytes from Socket IO to Worker thread
        let (packet_tx, packet_rx) = channel::<Vec<u8>>();
        // Channel to send fully reconstructed JPEG frames to the GUI main thread
        let (frame_tx, frame_rx) = channel::<Vec<u8>>();

        let target_mac = self.target_mac;

        // Worker processing thread: assembles shards and yields full JPEG byte buffers
        let worker_handle: JoinHandle<()> = thread::spawn(move || {
            let mut assembler = ChunkAssembler::new();

            while let Ok(packet) = packet_rx.recv() {
                if let Some(vtx_packet) = VtxPacket::parse(&packet, &target_mac, false) {
                    match vtx_packet.command_id {
                        0x01 => log::debug!("Received config frame"),
                        0x02 => {
                            // process_shard should return Option<Vec<u8>> when a full frame is finished
                            if let Some(jpeg_bytes) = assembler.process_shard(vtx_packet.payload) {
                                log::info!("Assembled full JPEG frame: {} bytes", jpeg_bytes.len());
                                let _ = frame_tx.send(jpeg_bytes);
                            }
                        }
                        _ => log::debug!("Ignoring Command ID: 0x{:02X}", vtx_packet.command_id),
                    }
                }
            }
        });

        // Spawn Network Reader Thread to free the Main Thread for GUI rendering
        let socket_fd = self.socket_fd;
        let reader_handle: JoinHandle<()> = thread::spawn(move || {
            let mut buf = [0u8; 2048];
            loop {
                let bytes_received = unsafe {
                    libc::recv(
                        socket_fd,
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                        0,
                    )
                };

                if bytes_received < 0 {
                    eprintln!("Error reading frame: {}", Error::last_os_error());
                    continue;
                }

                if packet_tx
                    .send(buf[..bytes_received as usize].to_vec())
                    .is_err()
                {
                    log::error!("Worker thread terminated. Exiting RX loop.");
                    break;
                }
            }
        });

        // Main Thread: GUI Window Loop (Required by macOS/Linux UI frameworks)
        let mut window = Window::new(
            "MJPEG Video Stream",
            self.width,
            self.height,
            WindowOptions::default(),
        )?;

        // minifb expects a buffer of packed 0x00RRGGBB u32 values
        let mut pixel_buffer: Vec<u32> = vec![0; self.width * self.height];

        while window.is_open() {
            if let Ok(jpeg_bytes) = frame_rx.try_recv() {
                // 1. Log size of incoming frame to verify data is arriving
                log::info!("Received JPEG frame: {} bytes", jpeg_bytes.len());

                // 2. Attempt decoding and inspect error if it fails
                match load_from_memory_with_format(&jpeg_bytes, image::ImageFormat::Jpeg) {
                    Ok(img) => {
                        let actual_width = img.width() as usize;
                        let actual_height = img.height() as usize;

                        // 3. Dynamic resizing check if video resolution differs from window size
                        if actual_width != self.width || actual_height != self.height {
                            log::warn!(
                                "Resolution mismatch! Expected {}x{}, got {}x{}",
                                self.width,
                                self.height,
                                actual_width,
                                actual_height
                            );
                            // Reallocate pixel buffer to match actual JPEG dimensions
                            pixel_buffer.resize(actual_width * actual_height, 0);
                        }

                        let rgb = img.to_rgb8();

                        // 4. Pack RGB into 0x00RRGGBB format expected by minifb
                        for (i, pixel) in rgb.pixels().enumerate() {
                            let [r, g, b] = pixel.0;
                            pixel_buffer[i] = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                        }

                        // Update minifb window with actual width and height
                        if let Err(e) =
                            window.update_with_buffer(&pixel_buffer, actual_width, actual_height)
                        {
                            log::error!("minifb update error: {}", e);
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to decode JPEG frame ({} bytes): {}",
                            jpeg_bytes.len(),
                            e
                        );
                    }
                }
            } else {
                // Redraw window buffer to keep event loop alive even without new frames
                let _ = window.update_with_buffer(&pixel_buffer, self.width, self.height);
            }
        }

        let _ = reader_handle.join();
        let _ = worker_handle.join();
        Ok(())
    }
}
