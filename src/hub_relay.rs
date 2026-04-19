use tokio::net::UdpSocket;
use tokio::time::{Duration, sleep};
use std::sync::Arc;
use std::net::SocketAddr;

use crate::db::*;
use crate::p2p::*;


pub async fn main_hub_relay(peer_id: String, hub_relay_addr: SocketAddr) {
	// Le hub relay démarre l'écoute
	let socket = UdpSocket::bind("0.0.0.0:55555").await.expect("Failed to bind");
	let public_addr: SocketAddr = get_public_ip(&socket).await.expect("Public IP not obtained.");
	println!("\nThe hub relay listens on {} as {}...", public_addr, peer_id);
	if hub_relay_addr != public_addr {
		println!("[ERROR] The hub relay has an address different as expected");
		return;
	}

	// Création des clés d'authentification
	let auth_keys: AuthKeys = AuthKeys::generate();

	// Initialise la base de données et initialise le compteur msg_id
	let db = DbManager::new("hub_relay.db").await.expect("Cannot open the database");
	let _ = init_msg_id(&db, public_addr).await;

	// Importe les données de la base de données vers le programme
	let relays: PeersMap = db.get_peers_from_db().await.expect("[ERROR] Initialisation of database failed");
	let logs: MessagesMap = db.get_logs_from_db().await.expect("[ERROR] Initialisation of database failed");

    // Suppression automatique des noeuds inactifs
    tokio::spawn({
	    let relays = Arc::clone(&relays);
	    async move {
	        loop {
	            delete_disconnected_peers(&relays).await;
	            sleep(Duration::from_secs(10)).await;
	        }
	    }
	});

    // Actualisation de la base de données
	tokio::spawn({
	    let relays = Arc::clone(&relays);
	    let logs = Arc::clone(&logs);
	    let db = db.clone();
	    async move {
	        loop {
	            sleep(Duration::from_secs(5)).await;

	            if let Err(e) = db.refresh_peers_in_db(&relays).await {
				    eprintln!("[ERROR] refresh_peers_in_db failed: {}", e);
				}
	            if let Err(e) = db.refresh_logs_in_db(&logs).await {
				    eprintln!("[ERROR] refresh_logs_in_db failed: {}", e);
				}
	        }
	    }
	});

	loop {
		let Some((msg_rcv, _)) = recv_msg(&socket).await else {continue};
		logs.lock().await.push(msg_rcv.clone());  // Enregistrement du message
		
		match &msg_rcv.payload {
			// Un relai se déclare : on l'ajoute/met à jour dans la map
			Payload::BeNewRelay { verifying_key } => {
				relays.lock().await
					.entry(peer_id_from_verifying_key(verifying_key))
					.and_modify(|peer_info| peer_info.last_seen = msg_rcv.headers.time)
					.or_insert(PeerInfo {
					    id: peer_id_from_verifying_key(verifying_key),
						username: msg_rcv.headers.src_id.clone(),
					    addr: msg_rcv.headers.src_addr,
					    last_seen: msg_rcv.headers.time,
					    is_relay: true,
					    verifying_key: *verifying_key,
					});

				// On accuse réception
				let mut msg = Message {
					headers: Headers {
						msg_id: new_msg_id(),
						src_addr: public_addr,
						src_id: peer_id.clone(),
						dst_addr: msg_rcv.headers.src_addr,
						dst_id: msg_rcv.headers.src_id.clone(),
						time: now_secs(),
						signature: vec![],         		
					},
					payload: Payload::Ack { reply_to: msg_rcv.headers.msg_id },
					last_hop: public_addr,
				};
				let _ = msg.sign(&auth_keys.signing_key);
				let _ = socket.send_msg(msg, msg_rcv.headers.src_addr, &logs).await;
			}

			// Un peer cherche un relai : on lui en renvoie un
			Payload::NeedRelay => {
				let relay_info = {
					let relays = relays.lock().await;
					relays.iter()
						.find(|(_, peer_info)| peer_info.addr != msg_rcv.headers.src_addr)
						.map(|(_, peer_info)| peer_info.clone())
				};
				if let Some (relay_info) = relay_info {
					let mut msg = Message {
						headers: Headers {
							msg_id: new_msg_id(),
							src_addr: public_addr,
							src_id: "hub".to_string(),
							dst_addr: msg_rcv.headers.src_addr,
							dst_id: msg_rcv.headers.src_id.clone(),
							time: now_secs(),
							signature: vec![], 
						},
						payload: Payload::PeerInfo { peer_info: relay_info.clone() },
						last_hop: public_addr,
					};
					let _ = msg.sign(&auth_keys.signing_key);
					let _ = socket.send_msg(msg, msg_rcv.headers.src_addr, &logs).await;

					// Avertissons le relais concerné
					let mut msg = Message {
						headers: Headers {
							msg_id: new_msg_id(),
							src_addr: public_addr,
							src_id: "hub".to_string(),
							dst_addr: relay_info.addr,
							dst_id: relay_info.username.clone(),
							time: now_secs(),
							signature: vec![], 
						},
						payload: Payload::RelayHasNewClient { peer_addr: msg_rcv.headers.src_addr, peer_id: msg_rcv.headers.src_id.clone()},
						last_hop: public_addr,
					};
					let _ = msg.sign(&auth_keys.signing_key);
					let _ = socket.send_msg(msg, relay_info.addr, &logs).await;
				} else {
					let mut msg = Message {
						headers: Headers {
							msg_id: new_msg_id(),
							src_addr: public_addr,
							src_id: "hub".to_string(),
							dst_addr: msg_rcv.headers.src_addr,
							dst_id: msg_rcv.headers.src_id.clone(),
							time: now_secs(),
							signature: vec![], 
						},
						payload: Payload::NoRelayAvailable,
						last_hop: public_addr,
					};
					let _ = msg.sign(&auth_keys.signing_key);
					let _ = socket.send_msg(msg, msg_rcv.headers.src_addr, &logs).await;
				}
			}
			_ => println!("Unexpected message: '{}'", msg_rcv)
		}
	}
}
