mod frame_decoder;
pub mod h264_decode;
mod vtx_packet;

use crate::common::config::*;
use crate::common::data_prepare::DataSharder;
use crate::receiver::frame_decoder::FrameDecoder;
use minifb::{Window, WindowOptions};
use std::io::Error;
use std::sync::mpsc::channel;
use std::thread::{self, JoinHandle};
use vtx_packet::VtxPacket;

pub struct VideoReceiver<D: FrameDecoder + Send + 'static> {
    socket_fd: i32,
    target_mac: [u8; 6],
    decoder: D,
}

impl<D: FrameDecoder + Send + 'static> VideoReceiver<D> {
    pub fn new(socket_fd: i32, target_mac: [u8; 6], decoder: D) -> Self {
        Self {
            socket_fd,
            target_mac,
            decoder,
        }
    }

    pub fn start(self) -> Result<(), Box<dyn std::error::Error>> {
        let (packet_tx, packet_rx) = channel::<Vec<u8>>();
        // Transfer ready-to-display 0x00RRGGBB pixel buffers to the GUI thread
        let (frame_tx, frame_rx) = channel::<Vec<u32>>();

        let target_mac = self.target_mac;
        let mut decoder = self.decoder;

        // Worker thread: parses packets, reconstructs raw chunks, and decodes to RGB pixels
        let worker_handle: JoinHandle<()> = thread::spawn(move || {
            let mut sharder = DataSharder::new();
            let mut vtx_packet_counter = 0u64;
            let mut non_vtx_packet_counter = 0u64;

            while let Ok(packet) = packet_rx.recv() {
                if let Some(vtx_packet) = VtxPacket::parse(&packet, &target_mac, false) {
                    match vtx_packet.command_id {
                        0x01 => log::debug!("Received config frame"),
                        0x02 => {
                            // process_shard returns reconstructed raw frame bytes upon completion
                            match sharder.process_shard(vtx_packet.payload) {
                                Ok(Some(raw_frame_bytes)) => {
                                    // Strategy decodes raw bytes directly to pixel buffer
                                    match decoder.decode_frame(&raw_frame_bytes) {
                                        Ok(pixel_buffer) => {
                                            let _ = frame_tx.send(pixel_buffer);
                                        }
                                        Err(err) => {
                                            log::error!("Decoder strategy error: {}", err);
                                            decoder.reset();
                                        }
                                    }
                                }
                                Ok(None) => {}
                                Err(err) => log::warn!("Chunk assembly warning: {}", err),
                            }
                        }
                        _ => log::debug!("Ignoring Command ID: 0x{:02X}", vtx_packet.command_id),
                    }
                    vtx_packet_counter += 1;
                } else {
                    non_vtx_packet_counter += 1;
                }

                if vtx_packet_counter % 100 == 0 {
                    log::info!(
                        "Processed {} VTX packets, ignored {} non-VTX packets ({}% ignored)",
                        vtx_packet_counter,
                        non_vtx_packet_counter,
                        (non_vtx_packet_counter as f64
                            / (vtx_packet_counter + non_vtx_packet_counter) as f64
                            * 100.0)
                            .round() as u64
                    );
                }
            }
        });

        // Network Reader Thread
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
                    log::error!("Error reading frame: {}", Error::last_os_error());
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

        // Main GUI Thread
        let mut window = Window::new("Video Stream", WIDTH, HEIGHT, WindowOptions::default())?;

        let mut current_pixels = vec![0u32; WIDTH * HEIGHT];

        while window.is_open() {
            if let Ok(new_pixel_buffer) = frame_rx.try_recv() {
                current_pixels = new_pixel_buffer;
                if let Err(e) = window.update_with_buffer(&current_pixels, WIDTH, HEIGHT) {
                    log::error!("minifb update error: {}", e);
                }
            } else {
                let _ = window.update_with_buffer(&current_pixels, WIDTH, HEIGHT);
            }
        }

        let _ = reader_handle.join();
        let _ = worker_handle.join();
        Ok(())
    }
}
