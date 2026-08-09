use crate::data_prepare::process_data_into_chunks;
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
    println!(
        "[*] Camera initialized: {}x{} ({})",
        set_fmt.width, set_fmt.height, set_fmt.fourcc
    );

    // 3. Allocate video streaming buffers in memory
    let mut stream = Stream::with_buffers(&dev, Type::VideoCapture, 4)?;

    println!("[*] Starting video loop...");

    transmit_video_frame(&mut stream, socket_fd)?;

    Ok(())
}

fn transmit_video_frame(
    stream: &mut Stream,
    socket_fd: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let (buf, _meta) = stream.next()?;
    println!("[+] Captured Frame: Size = {} bytes", buf.len());

    let encoded_chunks = process_data_into_chunks(buf)?;

    for chunk in encoded_chunks {
        for (i, shard) in chunk.iter().enumerate() {
            send_frame(socket_fd, i as u16, shard.clone())?;
        }
    }

    Ok(())
}

pub fn receive_video_stream(socket_fd: i32) -> Result<(), Box<dyn std::error::Error>> {
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

        // Ensure we received at least a minimal Radiotap Header (8 bytes) + 802.11 Header (24 bytes)
        if packet.len() < 32 {
            continue;
        }

        // --- Basic 802.11 Frame Parsing ---
        // 1. Radiotap header length is stored at byte offset 2 (u16 little endian)
        let radiotap_len = u16::from_le_bytes([packet[2], packet[3]]) as usize;

        if packet.len() < radiotap_len + 24 {
            continue; // Truncated header
        }

        // 2. The 802.11 header starts right after the Radiotap header
        let dot11_header = &packet[radiotap_len..];

        // Frame Control bytes
        let frame_control = u16::from_le_bytes([dot11_header[0], dot11_header[1]]);
        let frame_type = (frame_control >> 2) & 0x3; // Bit 2-3
        let frame_subtype = (frame_control >> 4) & 0xF; // Bit 4-7

        // Extract Destination and Source MAC Addresses
        let dst_mac = &dot11_header[4..10];
        let src_mac = &dot11_header[10..16];

        // Extract Sequence Control (Bits 4-15 = Sequence Number)
        let seq_ctrl = u16::from_le_bytes([dot11_header[22], dot11_header[23]]);
        let seq_num = seq_ctrl >> 4;

        if src_mac != [0x00, 0x11, 0x22, 0x33, 0x44, 0x55] {
            continue; // Ignore frames from other sources
        }

        // Print parsed frame summary
        println!(
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
    }
}

fn receive_video_frame(socket_fd: i32) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
