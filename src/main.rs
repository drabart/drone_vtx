mod config;
mod data_prepare;
mod network_send;
mod receiver;
mod transmitter;

use crate::network_send::{close_socket, open_socket};
use crate::receiver::VideoReceiver;
use crate::transmitter::VideoTransmitter;

use clap::{Parser, ValueEnum};
use env_logger::{Builder, Env};

#[derive(Parser, Debug)]
#[command(name = "drone-vtx")]
#[command(author = "DIY Drone VTX")]
#[command(version = "1.0")]
#[command(about = "Low-latency Wi-Fi raw injection video system", long_about = None)]
struct Cli {
    /// Operation mode: transmitter (tx) or receiver (rx)
    #[arg(short, long, value_enum, default_value_t = Mode::Tx)]
    mode: Mode,

    /// Network interface name to use for raw socket injection/sniffing
    #[arg(short, long, default_value = "wlan1")]
    interface: String,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Mode {
    Tx,
    Rx,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    log::info!("=== DIY Drone VTX Starting ===");

    match cli.mode {
        Mode::Tx => {
            log::info!("[*] Running in TRANSMITTER (Air Unit) mode");
            run_transmitter(&cli.interface)?;
        }
        Mode::Rx => {
            log::info!("[*] Running in RECEIVER (Ground Station) mode");
            run_receiver(&cli.interface)?;
        }
    }

    Ok(())
}

fn run_transmitter(interface: &str) -> Result<(), Box<dyn std::error::Error>> {
    let socket_fd = open_socket(interface)?;

    // Initialize the struct (camera index 0)
    let mut transmitter = VideoTransmitter::connect(0, socket_fd)?;

    log::info!("[*] Starting transmitter loop...");
    transmitter.start()?;

    // Cleanup (unreachable loop, but good practice if broken out)
    #[allow(unreachable_code)]
    {
        close_socket(socket_fd);
        Ok(())
    }
}

fn run_receiver(interface: &str) -> Result<(), Box<dyn std::error::Error>> {
    let socket_fd = open_socket(interface)?;

    // Target MAC address to filter
    let target_mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
    let receiver = VideoReceiver::new(socket_fd, target_mac);

    log::info!("[*] Starting receiver loop...");
    receiver.start()?;

    close_socket(socket_fd);
    Ok(())
}
