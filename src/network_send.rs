use libc::{AF_PACKET, ETH_P_ALL, SOCK_RAW, bind, sockaddr_ll, socket};
use std::ffi::CString;
use std::mem::zeroed;

pub fn open_socket(interface: &str) -> Result<i32, Box<dyn std::error::Error>> {
    // 1. Create a raw socket (AF_PACKET)
    let socket_fd = unsafe { socket(AF_PACKET, SOCK_RAW, (ETH_P_ALL as u16).to_be() as i32) };
    if socket_fd < 0 {
        return Err("Failed to create raw socket. Are you running as root/sudo?".into());
    }

    // 2. Resolve interface index for wlan1
    let iface_name = CString::new(interface)?;
    let if_index = unsafe { libc::if_nametoindex(iface_name.as_ptr()) };
    if if_index == 0 {
        return Err(format!("Interface {} not found", interface).into());
    }

    // 3. Bind socket to the monitor mode interface
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

pub fn send_frame(socket_fd: i32, frame_bytes: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    let data_frame = build_action_frame(&frame_bytes);

    let sent_bytes = unsafe {
        libc::send(
            socket_fd,
            data_frame.as_ptr() as *const libc::c_void,
            data_frame.len(),
            0,
        )
    };

    println!("[+] Sent Frame: Size = {} bytes", data_frame.len());

    if sent_bytes < 0 {
        return Err("Error sending frame".into());
    }

    Ok(())
}

pub fn build_action_frame(payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(35 + payload.len());

    // 1. Radiotap Header (8 Bytes)
    buf.extend_from_slice(&[0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00]);

    // 2. 802.11 Action Frame Header (24 Bytes)
    // Frame Control: Subtype Action (0xD0 -> Little Endian [0xd0, 0x00])
    buf.extend_from_slice(&[0xd0, 0x00]);
    buf.extend_from_slice(&[0x00, 0x00]); // Duration
    buf.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]); // Dest MAC
    buf.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]); // Src MAC
    buf.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]); // BSSID

    // 3. Action Frame Payload Header (3 Bytes)
    buf.push(127); // Category: Vendor-specific / Experimental (127)
    buf.extend_from_slice(&[0x00, 0x00, 0x00]); // OUI / ID

    // 4. Raw Contiguous Payload (Up to ~1400 bytes)
    buf.extend_from_slice(payload);

    buf
}
