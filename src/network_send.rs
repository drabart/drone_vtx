use libc::{AF_PACKET, ETH_P_ALL, SOCK_RAW, bind, sockaddr_ll, socket};
use std::ffi::CString;
use std::mem::zeroed;

pub fn open_socket(interface: &str) -> Result<i32, Box<dyn std::error::Error>> {
    // 1. Create raw socket
    let socket_fd = unsafe { socket(AF_PACKET, SOCK_RAW, (ETH_P_ALL as u16).to_be() as i32) };
    if socket_fd < 0 {
        return Err("Failed to create raw socket. Are you running as root/sudo?".into());
    }

    // 2. Set Socket Buffers to 4MB to prevent drops during bursts
    let buf_size: libc::c_int = 4 * 1024 * 1024;
    unsafe {
        libc::setsockopt(
            socket_fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &buf_size as *const _ as *const libc::c_void,
            std::mem::size_of_val(&buf_size) as libc::socklen_t,
        );
        libc::setsockopt(
            socket_fd,
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &buf_size as *const _ as *const libc::c_void,
            std::mem::size_of_val(&buf_size) as libc::socklen_t,
        );
    }

    // 3. Resolve interface index
    let iface_name = CString::new(interface)?;
    let if_index = unsafe { libc::if_nametoindex(iface_name.as_ptr()) };
    if if_index == 0 {
        return Err(format!("Interface {} not found", interface).into());
    }

    // 4. Bind socket
    let mut sa: sockaddr_ll = unsafe { zeroed() };
    sa.sll_family = AF_PACKET as u16;
    sa.sll_protocol = (ETH_P_ALL as u16).to_be();
    sa.sll_ifindex = if_index as i32;

    let bind_res = unsafe {
        bind(
            socket_fd,
            &sa as *const sockaddr_ll as *const libc::sockaddr,
            std::mem::size_of::<sockaddr_ll>() as u32,
        )
    };

    if bind_res < 0 {
        return Err("Failed to bind socket to interface.".into());
    }

    Ok(socket_fd)
}

pub fn close_socket(socket_fd: i32) {
    unsafe { libc::close(socket_fd) };
}

pub fn send_video_frame(
    socket_fd: i32,
    encoded_chunks: Vec<Vec<Vec<u8>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    for chunk in encoded_chunks {
        for frame in chunk {
            send_frame(socket_fd, frame)?;
        }
    }
    Ok(())
}

pub fn send_frame(socket_fd: i32, frame_bytes: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    let data_frame = build_action_frame(&frame_bytes);

    send_packet_nonblocking(socket_fd, &data_frame)?;

    Ok(())
}

pub fn send_packet_nonblocking(
    socket_fd: i32,
    packet: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let sent_bytes = unsafe {
        libc::send(
            socket_fd,
            packet.as_ptr() as *const libc::c_void,
            packet.len(),
            0,
        )
    };

    if sent_bytes < 0 {
        return Err("Error sending frame".into());
    }

    Ok(())
}

pub fn build_action_frame(payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + 24 + payload.len());

    // 1. Radiotap Header (8 Bytes)
    buf.extend_from_slice(&[0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00]);

    // 2. 802.11 Action Frame Header (24 Bytes)
    buf.extend_from_slice(&[0xd0, 0x00]); // Frame Control: Subtype Action
    buf.extend_from_slice(&[0x00, 0x00]); // Duration
    buf.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]); // Dest MAC
    buf.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]); // Src MAC
    buf.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]); // BSSID
    buf.extend_from_slice(&[0x00, 0x00]); // Sequence Control - Gets overwritten by the Wi-Fi driver

    // 4. Raw Contiguous Payload (Up to ~1400 bytes)
    buf.extend_from_slice(payload);

    buf
}
