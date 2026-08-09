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

pub fn send_frame(
    socket_fd: i32,
    shard_id: u16,
    frame_bytes: Vec<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data_frame = build_data_frame(shard_id, &frame_bytes);

    let sent_bytes = unsafe {
        libc::send(
            socket_fd,
            data_frame.as_ptr() as *const libc::c_void,
            data_frame.len(),
            0,
        )
    };

    if sent_bytes < 0 {
        return Err("Error sending frame".into());
    }

    Ok(())
}

/// Constructs an 802.11 Data Frame pushing up to ~1400 bytes of raw FEC payload
pub fn build_data_frame(seq_num: u16, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32 + payload.len());

    // --- 1. RADIOTAP HEADER (8 Bytes) ---
    buf.extend_from_slice(&[
        0x00, 0x00, // Header revision & pad
        0x08, 0x00, // Header length: 8 bytes
        0x00, 0x00, 0x00, 0x00, // Present flags
    ]);

    // --- 2. 802.11 DATA FRAME HEADER (24 Bytes) ---
    // Frame Control: Data Frame (Subtype 0x00, Type 0x02 => 0x0008, Little Endian: [0x08, 0x00])
    // Flags: ToDS = 0, FromDS = 0 (Ad-hoc / Direct Injection)
    buf.extend_from_slice(&[0x08, 0x00]);
    // Duration
    buf.extend_from_slice(&[0x00, 0x00]);
    // Destination Addr (MAC): Broadcast (ff:ff:ff:ff:ff:ff) or Receiver MAC
    buf.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    // Source Addr (MAC): 00:11:22:33:44:55
    buf.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    // BSSID: Broadcast
    buf.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);

    // Sequence Control (Fragment 0, Sequence Number)
    let sequence_control = (seq_num % 4096) << 4;
    buf.extend_from_slice(&sequence_control.to_le_bytes());

    // --- 3. CONTIGUOUS PAYLOAD (No IEs, No Tags!) ---
    // Push the entire payload buffer straight into the packet
    buf.extend_from_slice(payload);

    buf
}
