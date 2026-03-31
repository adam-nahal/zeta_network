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
    println!("\nThe hub relay listens on {} ({})...", public_addr, peer_id);
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

    let mut buf = vec![0; 1024];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((size, sender_addr)) => {
                // Affichage du message
                if let Err(e) = bincode::deserialize::<Message>(&buf[..size]) {
    				eprintln!("[ERROR] Deserialization failed: {} (message from {})", e, sender_addr);
    				continue; // passer au message suivant
				}
                let msg: Message = bincode::deserialize(&buf[..size]).expect("[ERROR] Deserialization failed");
                println!("<-{}", msg);
				
				match &msg {
                    // Un relai se déclare : on l'ajoute/met à jour dans la map
                    Message::BeNewRelay { src_addr, src_id, time, .. } => {
                        relays_list.lock().await
                            .entry(*src_addr)
                            .and_modify(|(_, t)| *t = *time)
                            .or_insert((src_id.clone(), *time));

                        // On accuse réception
                        let ack = Message::Ack {
                            src_addr: public_addr,
                            src_id: public_addr.to_string(),
                            time: now_secs(),
                        };
                        let _ = socket.send_msg(&ack, sender_addr).await;
                        println!("->{}", ack);
                    }

                    // Un peer cherche un relai : on lui en renvoie un
                    Message::NeedRelay { src_addr, src_id, .. } => {
                        let relays = relays_list.lock().await;
                        if let Some((relay_addr, (relay_id, _))) = relays.iter().next() {
                            let msg = Message::PeerInfo {
                                peer_addr: *relay_addr,
                                peer_id: relay_id.to_string(),
                            };
                            let _ = socket.send_msg(&msg, *src_addr).await;
                            println!("->{}", msg);

                            // Avertissons le relais concerné
                            let msg = Message::RelayHasNewClient {
	                            src_addr: public_addr,
	                            src_id: "hubRelay".to_string(),
                                peer_addr: *src_addr,
                                peer_id: src_id.clone(),
                            	time: now_secs(),
                            };
                            let _ = socket.send_msg(&msg, *relay_addr).await;
                            println!("->{}", msg);
                        } else {
                            let msg = Message::NoRelayAvailable {
	                            src_addr: public_addr,
	                            src_id: "hubRelay".to_string(),
                                dst_addr: *src_addr,
                                dst_id: src_id.clone(),
                            	time: now_secs(),
                            };
                            let _ = socket.send_msg(&msg, *src_addr).await;
                            println!("->{}", msg);
                        }
                    }

                    _ => eprintln!("[WARN] Unrecognize message from {}", sender_addr),
                }
            }
            Err(e) => eprintln!("[ERROR] a message contain an error ({})", e),
        }
    }
}