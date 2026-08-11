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
