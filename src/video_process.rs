use crate::config::*;
use crate::data_prepare::{decode_chunk, process_frame_into_chunks};
use crate::network_send::send_frame;
use radiotap::Radiotap;
use std::io::Error;
use std::sync::mpsc::channel;
use std::thread::{self, JoinHandle};
use std::time::Instant;
use v4l::{
    buffer::Type,
    device::Device,
    format::FourCC,
    io::{mmap::Stream, traits::CaptureStream},
    video::Capture,
};

// ============================================================================
// 1. CHUNK ASSEMBLER STATE MACHINE
// ============================================================================

pub struct ChunkAssembler {
    last_frame_id: i32,
    last_chunk_id: i32,
    chunks_ok: i32,
    total_shards_expected: usize,
    total_shards_received: usize,
    current_chunk: [Option<Vec<u8>>; CHUNK_SHARDS],
}

impl Default for ChunkAssembler {
    fn default() -> Self {
        Self {
            last_frame_id: -1,
            last_chunk_id: -1,
            chunks_ok: 0,
            total_shards_expected: 0,
            total_shards_received: 0,
            current_chunk: Default::default(),
        }
    }
}

impl ChunkAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process_shard(&mut self, payload: &[u8]) {
        // Minimum payload size: Command (1) + FrameID (1) + ChunkID (1) + ShardID (1) + FCS (4)
        if payload.len() < 8 {
            return;
        }

        let frame_id = payload[1] as u32;
        let chunk_id = payload[2] as usize;
        let shard_id = payload[3] as usize;

        if shard_id >= CHUNK_SHARDS {
            log::warn!("Received invalid shard_id: {}", shard_id);
            return;
        }

        // Frame transition: flush pending chunk & log stats
        if frame_id != self.last_frame_id as u32 {
            if self.last_frame_id != -1 {
                if self.last_chunk_id != -1 {
                    self.flush_and_decode_chunk();
                }

                let frame_loss_pct = if self.total_shards_expected > 0 {
                    100.0
                        * (1.0
                            - (self.total_shards_received as f32
                                / self.total_shards_expected as f32))
                } else {
                    0.0
                };

                log::info!(
                    "[*] Frame {} completed. Chunks decoded: {}/{}. Total Shard Loss: {:.2}% ({}/{} shards)",
                    self.last_frame_id,
                    self.chunks_ok,
                    if self.last_chunk_id >= 0 {
                        self.last_chunk_id + 1
                    } else {
                        0
                    },
                    frame_loss_pct,
                    self.total_shards_expected
                        .saturating_sub(self.total_shards_received),
                    self.total_shards_expected
                );
            }

            self.chunks_ok = 0;
            self.total_shards_expected = 0;
            self.total_shards_received = 0;
            self.last_frame_id = frame_id as i32;
            self.last_chunk_id = -1;
        }

        // Chunk transition: flush completed chunk
        if chunk_id != self.last_chunk_id as usize {
            if self.last_chunk_id != -1 {
                self.flush_and_decode_chunk();
            }
            self.last_chunk_id = chunk_id as i32;
        }

        // Store shard payload (strip 4-byte header and 4-byte trailing FCS)
        let end_idx = payload.len() - 4;
        if end_idx >= 4 {
            self.current_chunk[shard_id] = Some(payload[4..end_idx].to_vec());
        }
    }

    fn flush_and_decode_chunk(&mut self) {
        let received_count = self.current_chunk.iter().filter(|s| s.is_some()).count();
        let missing_count = CHUNK_SHARDS.saturating_sub(received_count);
        let chunk_loss_pct = 100.0 * (missing_count as f32 / CHUNK_SHARDS as f32);

        // Accumulate overall frame statistics
        self.total_shards_expected += CHUNK_SHARDS;
        self.total_shards_received += received_count;

        match decode_chunk(&mut self.current_chunk) {
            Ok(_) => {
                self.chunks_ok += 1;
                log::debug!(
                    "Chunk {} decoded OK. Shards: {}/{} (Loss: {:.1}%)",
                    self.last_chunk_id,
                    received_count,
                    CHUNK_SHARDS,
                    chunk_loss_pct
                );
            }
            Err(err) => {
                log::error!(
                    "Failed decoding Chunk {}: {:?}. Received {}/{} shards (min required: {}, Loss: {:.1}%)",
                    self.last_chunk_id,
                    err,
                    received_count,
                    CHUNK_SHARDS,
                    DATA_SHARDS,
                    chunk_loss_pct
                );
            }
        }

        // Reset buffer
        for shard in self.current_chunk.iter_mut() {
            *shard = None;
        }
    }
}

// ============================================================================
// 2. ZERO-COPY PACKET PARSER STRUCT
// ============================================================================

pub struct VtxPacket<'a> {
    pub command_id: u8,
    pub payload: &'a [u8],
}

impl<'a> VtxPacket<'a> {
    pub fn parse(packet: &'a [u8], filter_mac: &[u8; 6], debug: bool) -> Option<Self> {
        if packet.len() < 4 {
            return None;
        }

        let radiotap_len = u16::from_le_bytes([packet[2], packet[3]]) as usize;
        let min_len = radiotap_len + 24 + 4; // Radiotap + 802.11 Header (24) + VTX Header (4)

        if packet.len() < min_len {
            return None;
        }

        let radiotap_bytes = &packet[..radiotap_len];
        let dot11_header = &packet[radiotap_len..];

        // Filter Source MAC
        let src_mac: &[u8; 6] = dot11_header[10..16].try_into().ok()?;
        if src_mac != filter_mac {
            return None;
        }

        // Filter Frame Type (Management Action Frame: Type 0, Subtype 13)
        let frame_control = u16::from_le_bytes([dot11_header[0], dot11_header[1]]);
        let frame_type = (frame_control >> 2) & 0x3;
        let frame_subtype = (frame_control >> 4) & 0xF;
        if frame_type != 0 || frame_subtype != 13 {
            return None;
        }

        let payload = &dot11_header[24..];
        let command_id = payload[0];

        if debug {
            Self::log_radiotap_debug(radiotap_bytes);
        }

        Some(Self {
            command_id,
            payload,
        })
    }

    fn log_radiotap_debug(radiotap_bytes: &[u8]) {
        match Radiotap::from_bytes(radiotap_bytes) {
            Ok(rt) => {
                let rate_str = rt
                    .rate
                    .map_or("N/A".to_string(), |r| format!("{:.1} Mbps", r.value));

                let ch_str = rt
                    .channel
                    .map_or("N/A".to_string(), |c| format!("{} MHz", c.freq));

                let rssi_str = rt
                    .antenna_signal
                    .map_or("N/A".to_string(), |s| format!("{} dBm", s.value));

                log::info!(
                    "Radiotap Decoded | Rate: {} | Ch: {} | RSSI: {}",
                    rate_str,
                    ch_str,
                    rssi_str
                );
            }
            Err(e) => {
                log::warn!(
                    "Failed to parse Radiotap header ({} bytes): {:?}. Raw hex: {}",
                    radiotap_bytes.len(),
                    e,
                    radiotap_bytes
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
        }
    }
}

// ============================================================================
// 3. RECEIVER WORKER & NETWORK LOOP
// ============================================================================

pub struct VideoReceiver {
    socket_fd: i32,
    target_mac: [u8; 6],
}

impl VideoReceiver {
    pub fn new(socket_fd: i32, target_mac: [u8; 6]) -> Self {
        Self {
            socket_fd,
            target_mac,
        }
    }

    pub fn start(self) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = channel::<Vec<u8>>();
        let target_mac = self.target_mac;

        // Worker processing thread
        let worker_handle: JoinHandle<()> = thread::spawn(move || {
            let mut assembler = ChunkAssembler::new();

            while let Ok(packet) = rx.recv() {
                if let Some(vtx_packet) = VtxPacket::parse(&packet, &target_mac, false) {
                    match vtx_packet.command_id {
                        0x01 => log::debug!("Received config frame"),
                        0x02 => assembler.process_shard(vtx_packet.payload),
                        _ => log::debug!("Ignoring Command ID: 0x{:02X}", vtx_packet.command_id),
                    }
                }
            }
        });

        // Main Socket I/O Loop
        let mut buf = [0u8; 2048];
        loop {
            let bytes_received = unsafe {
                libc::recv(
                    self.socket_fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    0,
                )
            };

            if bytes_received < 0 {
                eprintln!("Error reading frame: {}", Error::last_os_error());
                continue;
            }

            if tx.send(buf[..bytes_received as usize].to_vec()).is_err() {
                log::error!("Worker thread terminated. Exiting RX loop.");
                break;
            }
        }

        let _ = worker_handle.join();
        Ok(())
    }
}

// ============================================================================
// 4. TRANSMITTER STRUCT
// ============================================================================

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
