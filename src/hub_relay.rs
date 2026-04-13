use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use std::sync::Arc;
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::db::*;
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

	// Initialise la base de données
	let db = DbManager::new("hub_relay.db").await.expect("Cannot open the database");

	// Importe les données de la base de données vers le programme
	let peers_in_db = db.get_all_peers().await.unwrap_or_default();
    let relays_list: PeersMap = Arc::new(Mutex::new(HashMap::new()));
    for peer in peers_in_db {
    	if peer.is_relay {
	        relays_list.lock().await.insert(peer.addr, PeerInfo {
	        	addr: peer.addr,
	        	id: peer.id, 
	        	last_seen: peer.last_seen, 
	        	is_relay: peer.is_relay
	        });
	    }
    }

    // Suppression automatique des noeuds inactifs
    tokio::spawn({
	    let relays_list = Arc::clone(&relays_list);
	    async move {
	        loop {
	            sleep(Duration::from_secs(10)).await;
	            delete_disconnected_peers(&relays_list).await;
	        }
	    }
	});

    // Actualisation de la base de données
	tokio::spawn({
	    let relays_list = Arc::clone(&relays_list);
	    let db = db.clone();
	    async move {
	        loop {
	            sleep(Duration::from_secs(5)).await;
	            db.refresh_peers(Arc::clone(&relays_list)).await;
	        }
	    }
	});

	loop {
		let Some((msg, sender_addr)) = recv_msg(&socket).await else {continue};
		
		match &msg {
			// Un relai se déclare : on l'ajoute/met à jour dans la map
			Message::BeNewRelay { header } => {
				relays_list.lock().await
					.entry(header.src_addr)
					.and_modify(|peer_info| peer_info.last_seen = header.time)
					.or_insert(PeerInfo {
					    addr: header.src_addr,
					    id: header.src_id.clone(),
					    last_seen: header.time,
					    is_relay: true,
					});

				// On accuse réception
				let msg = Message::Ack {
					header: MessageHeader {
						msg_id: new_msg_id(),
						src_addr: public_addr,
						src_id: peer_id.clone(),
						dst_addr: header.src_addr,
						dst_id: header.src_id.clone(),
						time: now_secs(),                		
					},
					reply_to: header.msg_id
				};
				let _ = socket.send_msg(&msg, sender_addr).await;
			}

			// Un peer cherche un relai : on lui en renvoie un
			Message::NeedRelay { header } => {
				let relay_info = {
					let relays = relays_list.lock().await;
					relays.iter()
						.find(|(addr, _)| **addr != header.src_addr)
						.map(|(addr, peer_info)| (*addr, peer_info.id.clone()))
				};
				if let Some((relay_addr, relay_id)) = relay_info {
					let msg = Message::PeerInfo {
						header: MessageHeader {
							msg_id: new_msg_id(),
							src_addr: public_addr,
							src_id: "hub".to_string(),
							dst_addr: header.src_addr,
							dst_id: header.src_id.clone(),
							time: now_secs(),
						},
						peer_addr: relay_addr,
						peer_id: relay_id.clone(),
					};
					let _ = socket.send_msg(&msg, header.src_addr).await;

					// Avertissons le relais concerné
					let msg = Message::RelayHasNewClient {
						header: MessageHeader {
							msg_id: new_msg_id(),
							src_addr: public_addr,
							src_id: "hub".to_string(),
							dst_addr: relay_addr,
							dst_id: relay_id,
							time: now_secs(),
						},
						peer_addr: header.src_addr,
						peer_id: header.src_id.clone(),
					};
					let _ = socket.send_msg(&msg, relay_addr).await;
				} else {
					let msg = Message::NoRelayAvailable {
						header: MessageHeader {
							msg_id: new_msg_id(),
							src_addr: public_addr,
							src_id: "hub".to_string(),
							dst_addr: header.src_addr,
							dst_id: header.src_id.clone(),
							time: now_secs(),
						},
					};
					let _ = socket.send_msg(&msg, header.src_addr).await;
				}
			}
			_ => println!("Unexpected message: '{}'", msg)
		}
	}
}
