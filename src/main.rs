use std::net::SocketAddr;
use clap::Parser;
mod client;
mod nat_detector;
mod lib_p2p;
mod hub_relay;

use crate::lib_p2p::*;


#[tokio::main]
async fn main() {
	// Adresse du hub relay
	let hub_relay_addr: SocketAddr = "65.75.200.180:55555".parse().unwrap();

	// Récupération des arguments en ligne de commande
    let opts = Opts::parse();

    match opts.mode {
        Mode::HubRelay => hub_relay::main_hub_relay(opts.peer_id, hub_relay_addr).await,
        Mode::Client => client::main_client(opts.peer_id, hub_relay_addr).await,
    }
    
    println!("\nSee you soon!");

}
