use radiotap::Radiotap;

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
        let min_len = radiotap_len + 24; // Base Radiotap + 802.11 Header

        if packet.len() < min_len {
            return None;
        }

        let radiotap_bytes = &packet[..radiotap_len];
        let dot11_header = &packet[radiotap_len..];

        // 1. Decode Frame Control (Type & Subtype)
        let frame_control = u16::from_le_bytes([dot11_header[0], dot11_header[1]]);
        let frame_type = (frame_control >> 2) & 0x3; // 0 = Mgmt, 1 = Control, 2 = Data
        let frame_subtype = (frame_control >> 4) & 0xF;

        // 2. Extract Addresses
        let dest_mac: &[u8; 6] = dot11_header[4..10].try_into().ok()?;
        let src_mac: &[u8; 6] = dot11_header[10..16].try_into().ok()?;

        // If in debug mode, log metadata for ALL captured background packets before filtering out
        if debug {
            Self::log_packet_inspection(
                radiotap_bytes,
                frame_type,
                frame_subtype,
                src_mac,
                dest_mac,
            );
        }

        // Filter for your specific Source MAC
        if src_mac != filter_mac {
            return None;
        }

        // Filter for Management Action Frames (Type 0, Subtype 13)
        if frame_type != 0 || frame_subtype != 13 {
            return None;
        }

        // VTX Header check
        if packet.len() < min_len + 4 {
            return None;
        }

        let payload = &dot11_header[24..];
        let command_id = payload[0];

        Some(Self {
            command_id,
            payload,
        })
    }

    fn log_packet_inspection(
        radiotap_bytes: &[u8],
        f_type: u16,
        f_subtype: u16,
        src: &[u8; 6],
        dst: &[u8; 6],
    ) {
        let type_str = match f_type {
            0 => "MGMT",
            1 => "CTRL",
            2 => "DATA",
            _ => "UNKN",
        };

        let subtype_str = match (f_type, f_subtype) {
            (1, 11) => "RTS",
            (1, 12) => "CTS",
            (1, 13) => "ACK",
            (0, 4) => "ProbeReq",
            (0, 5) => "ProbeResp",
            (0, 8) => "Beacon",
            (0, 13) => "Action",
            (2, 0) => "Data",
            (2, 8) => "QoS Data",
            _ => "Other",
        };

        let mut signal_info = String::from("N/A");
        if let Ok(rt) = Radiotap::from_bytes(radiotap_bytes) {
            if let Some(sig) = rt.antenna_signal {
                signal_info = format!("{} dBm", sig.value);
            }
        }

        log::info!(
            "[BG Traffic] Type: {:4} ({:10}) | Src: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X} | Dst: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X} | RSSI: {}",
            type_str,
            subtype_str,
            src[0],
            src[1],
            src[2],
            src[3],
            src[4],
            src[5],
            dst[0],
            dst[1],
            dst[2],
            dst[3],
            dst[4],
            dst[5],
            signal_info
        );
    }
}
