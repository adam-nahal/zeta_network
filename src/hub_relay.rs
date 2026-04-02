use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use std::sync::Arc;
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::lib_p2p::*;


pub async fn main_hub_relay(peer_id: String, hub_relay_addr: SocketAddr) {
    // Le hub relay démarre l'écoute
    let socket = UdpSocket::bind("0.0.0.0:55555").await.expect("Failed to bind");
    let public_addr: SocketAddr = get_public_ip(&socket).await.expect("Public IP not obtained.");
    println!("\nThe hub relay listens on {} as {}...", public_addr, peer_id);
    if hub_relay_addr != public_addr {
    	println!("[ERROR] The hub relay has an address different as expected");
    	return;
    }

    // Crée la liste de tous les relais
    let relays_list: PeersMap = Arc::new(Mutex::new(HashMap::new()));

    // Suppression automatique des noeuds inactifs
    let relays_cleanup = Arc::clone(&relays_list);
	tokio::spawn(async move {
	    loop {
	        sleep(Duration::from_secs(10)).await;
	        delete_disconnected_peers(&relays_cleanup).await;
	    }
	});

    loop {
    	let Some((msg, sender_addr)) = recv_msg(&socket).await else {continue};
		
		match &msg {
            // Un relai se déclare : on l'ajoute/met à jour dans la map
            Message::BeNewRelay { header } => {
                relays_list.lock().await
                    .entry(header.src_addr)
                    .and_modify(|(_, t)| *t = header.time)
                    .or_insert((header.src_id.clone(), header.time));

                // On accuse réception
                let msg = Message::Ack {
                	header: MessageHeader {
	                    src_addr: public_addr,
	                    src_id: peer_id.clone(),
	                    dst_addr: header.src_addr,
	                    dst_id: header.src_id.clone(),
	                    time: now_secs(),                		
                	}
                };
                let _ = socket.send_msg(&msg, sender_addr).await;
                println!("->{}", msg);
            }

            // Un peer cherche un relai : on lui en renvoie un
            Message::NeedRelay { header } => {
                let relays = relays_list.lock().await;
                let chosen_relay = relays
			        .iter()
			        .find(|(addr, _)| **addr != header.src_addr);
                if let Some((relay_addr, (relay_id, _))) = chosen_relay {
                    let msg = Message::PeerInfo {
                    	header: MessageHeader {
	                        src_addr: public_addr,
	                        src_id: "hub".to_string(),
	                        dst_addr: header.src_addr,
	                        dst_id: header.src_id.clone(),
	                    	time: now_secs(),
	                    },
                        peer_addr: *relay_addr,
                        peer_id: relay_id.to_string(),
                    };
                    let _ = socket.send_msg(&msg, header.src_addr).await;
                    println!("->{}", msg);

                    // Avertissons le relais concerné
                    let msg = Message::RelayHasNewClient {
                    	header: MessageHeader {
	                        src_addr: public_addr,
	                        src_id: "hub".to_string(),
	                        dst_addr: *relay_addr,
	                        dst_id: relay_id.clone(),
	                    	time: now_secs(),
                    	},
                        peer_addr: header.src_addr,
                        peer_id: header.src_id.clone(),
                    };
                    let _ = socket.send_msg(&msg, *relay_addr).await;
                    println!("->{}", msg);
                } else {
                    let msg = Message::NoRelayAvailable {
                    	header: MessageHeader {
	                        src_addr: public_addr,
	                        src_id: "hub".to_string(),
	                        dst_addr: header.src_addr,
	                        dst_id: header.src_id.clone(),
	                    	time: now_secs(),
                    	},
                    };
                    let _ = socket.send_msg(&msg, header.src_addr).await;
                    println!("->{}", msg);
                }
            }
            _ => println!("Unexpected message: '{}'", msg)
        }
    }
}