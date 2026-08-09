use crate::config::*;
use crate::data_prepare::{decode_chunk, process_frame_into_chunks};
use crate::network_send::send_frame;
use std::io::Error;
use v4l::{
    buffer::Type,
    device::Device,
    format::FourCC,
    io::{mmap::Stream, traits::CaptureStream},
    video::Capture,
};

pub fn transmit_video_stream(socket_fd: i32) -> Result<(), Box<dyn std::error::Error>> {
    let dev = Device::new(0)?;

    // 2. Query and set camera format
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

    // 3. Allocate video streaming buffers in memory
    let mut stream = Stream::with_buffers(&dev, Type::VideoCapture, 4)?;

    log::info!("[*] Starting video loop...");

    transmit_video_frame(&mut stream, socket_fd)?;

    Ok(())
}

fn transmit_video_frame(
    stream: &mut Stream,
    socket_fd: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let (buf, _meta) = stream.next()?;
    log::debug!("[+] Captured Frame: Size = {} bytes", buf.len());

    let encoded_chunks = process_frame_into_chunks(0, &buf)?; // Replace 0 with actual frame ID if available

    for chunk in encoded_chunks {
        for shard in chunk {
            send_frame(socket_fd, shard)?;
        }
    }

    Ok(())
}

pub fn receive_video_stream(socket_fd: i32) -> Result<(), Box<dyn std::error::Error>> {
    let mut last_chunk_id: i32 = -1;
    let mut chunk: [Option<Vec<u8>>; CHUNK_SHARDS] = Default::default();

    let mut buf = [0u8; 2048];

    loop {
        // Read incoming frame from raw socket
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

        let packet = &buf[..bytes_received as usize];

        // --- Basic 802.11 Frame Parsing ---
        // 1. Radiotap header length is stored at byte offset 2 (u16 little endian)
        let radiotap_len = u16::from_le_bytes([packet[2], packet[3]]) as usize;

        if packet.len() < radiotap_len + 24 {
            continue;
        }

        // 2. The 802.11 header starts right after the Radiotap header
        let dot11_header = &packet[radiotap_len..];

        // Frame Control bytes
        let frame_control = u16::from_le_bytes([dot11_header[0], dot11_header[1]]);

        let frame_type = (frame_control >> 2) & 0x3; // Bit 2-3
        let frame_subtype = (frame_control >> 4) & 0xF; // Bit 4-7

        // Extract Destination and Source MAC Addresses
        let _dst_mac = &dot11_header[4..10];
        let src_mac = &dot11_header[10..16];
        let _bssid_mac = &dot11_header[16..22];

        // Extract Sequence Control (Bits 4-15 = Sequence Number)
        let seq_ctrl = u16::from_le_bytes([dot11_header[22], dot11_header[23]]);
        let seq_num = seq_ctrl >> 4;

        if src_mac != [0x00, 0x11, 0x22, 0x33, 0x44, 0x55] {
            continue; // Ignore frames from other sources
        }

        let payload = &dot11_header[24..];

        // Print parsed frame summary
        log::debug!(
            "[{} bytes] Type: {} Subtype: {:2} | Src: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} | Seq: {}",
            bytes_received,
            match frame_type {
                0 => "Management",
                1 => "Control",
                2 => "Data",
                _ => "Unknown",
            },
            frame_subtype,
            src_mac[0],
            src_mac[1],
            src_mac[2],
            src_mac[3],
            src_mac[4],
            src_mac[5],
            seq_num
        );

        let frame_id = payload[0] as u32;
        let chunk_id = payload[1] as usize;
        let shard_id = payload[2] as usize;

        log::debug!(
            "Frame ID: {}, Chunk ID: {}, Shard ID: {}",
            frame_id,
            chunk_id,
            shard_id
        );

        if chunk_id != last_chunk_id as usize {
            if last_chunk_id != -1 {
                // New chunk received
                let result = decode_chunk(&mut chunk);

                if let Err(e) = result {
                    log::error!("Error decoding chunk: {}", e);
                    log::info!(
                        "Chunk has {} shards, expected {} (minimum {})",
                        chunk.iter().filter(|s| s.is_some()).count(),
                        CHUNK_SHARDS,
                        DATA_SHARDS
                    );
                }

                // Reset the chunk vector
                for shard in chunk.iter_mut() {
                    *shard = None;
                }
            }

            last_chunk_id = chunk_id as i32;
        } else {
            chunk[shard_id] = Some(payload[3..(payload.len() - 4)].to_vec());
        }
    }
}
