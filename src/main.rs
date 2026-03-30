use std::net::SocketAddr;
use clap::Parser;
mod client;
mod nat_detector;
mod lib_p2p;
mod hubRelay;

use crate::lib_p2p::*;


#[tokio::main]
async fn main() {
	// Adresse du hub relay
	let hubRelay_addr: SocketAddr = "65.75.200.180:55555".parse().unwrap();

	// Récupération des arguments en ligne de commande
    let opts = Opts::parse();

    match opts.mode {
        Mode::HubRelay => hubRelay::main_hubRelay(opts.peer_id, hubRelay_addr).await,
        Mode::Client => client::main_client(opts.peer_id, hubRelay_addr).await,
    }
    
    println!("\nSee you soon!");

}
