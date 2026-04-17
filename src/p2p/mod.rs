pub mod types;
pub mod network;
pub mod dispatch;
pub mod peers;
pub mod utils;

pub use types::*;
pub use network::*;
pub use dispatch::*;
pub use peers::*;
pub use utils::*;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
pub struct Opts {
    #[command(subcommand)]
    pub mode: Mode,
}

#[derive(Debug, Subcommand)]
pub enum Mode {
    Client {
        #[arg(long)]
        peer_id: String,
    },
    HubRelay,
}
