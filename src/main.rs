use std::net::SocketAddr;
use clap::Parser;
mod client;
mod nat_detector;
mod p2p;
mod hub_relay;
mod db;
mod lightning;

use crate::p2p::*;
//use crate::lightning::keys::LdkKeysManager;

#[tokio::main]
async fn main() {
	let hub_relay_addr: SocketAddr = "65.75.200.180:55555".parse().unwrap();

	// Récupération des arguments en ligne de commande
    let opts = Opts::parse();

    match opts.mode {
        Mode::Client { username } => client::main_client(username, hub_relay_addr).await,
        Mode::HubRelay => hub_relay::main_hub_relay("hub".to_string(), hub_relay_addr).await,
    }
    
    //Futur test
    //let km = LdkKeysManager::load("identity.key").unwrap();
    //println!("LN node_id = {}", km.node_id_hex());
    
    println!("\nSee you soon!");
}
