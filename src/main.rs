mod config;
mod data_prepare;
mod network_send;
mod video_process;

use crate::network_send::{close_socket, open_socket};
use crate::video_process::{receive_video_stream, transmit_video_stream};

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
    transmit_video_stream(socket_fd)?;
    close_socket(socket_fd);

    Ok(())
}

fn run_receiver(interface: &str) -> Result<(), Box<dyn std::error::Error>> {
    let socket_fd = open_socket(interface)?;
    receive_video_stream(socket_fd)?;
    close_socket(socket_fd);

    Ok(())
}
