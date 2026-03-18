use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use std::sync::Arc;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Message, UdpSocketExt, get_public_ip};
use crate::nat_detector::nat_detector;

type PeersMap = Arc<Mutex<HashMap<SocketAddr, (String, u64)>>>; // un noeud = [Addr, date dernière connection en sec]

pub async fn main_relay() {
	// Description de ce noeud
	println!("\nLooking for NAT type...");
	let (nat_type, _) = nat_detector().await
		.expect("[ERROR] NAT type not detected");
    println!("   -> {:?}", nat_type);

    // Le relay démarre l'écoute
    let port_relay = 12345;
    let addr_relay = format!("0.0.0.0:{}", port_relay);
    let socket = UdpSocket::bind(&addr_relay).await.expect("Failed to bind");
    let public_addr: SocketAddr = get_public_ip(&socket).await.expect("Public IP not obtained.");
    println!("\nListening on {}...", public_addr);

    // Crée la liste de tous les clients qui ont contacté ce relai
    let peers_list: PeersMap = Arc::new(Mutex::new(HashMap::new()));

    // Suppression automatique des noeuds inactifs
    let peers_cleanup = Arc::clone(&peers_list);
	tokio::spawn(async move {
	    loop {
	        sleep(Duration::from_secs(30)).await;
	        delete_disconnected_peers(&peers_cleanup).await;
	    }
	});

    let mut buf = vec![0; 1024];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((size, sender_addr)) => {
                // Affichage du message
                let msg: Message = bincode::deserialize(&buf[..size]).expect("[ERROR] Deserialization failed");
                println!("{}", msg);

                // Ajout des nouveaux noeuds ou mise à jour de la dernière connection
                let connected_peers_clone = Arc::clone(&peers_list);
                if let Message::Register { src_id, time, .. } = &msg {
                	connected_peers_clone.lock().await
				        .entry(sender_addr)  // La clé existe-t-elle déjà ?
    					.and_modify(|(_, t)| *t = *time)
				        .or_insert((src_id.clone(), *time));
            	}

            	// Relaie le message si c'est un message à relayer
                if let Message::Classic { dst_addr, .. } = &msg {
                	if public_addr != *dst_addr {
                		relay_message(&connected_peers_clone, sender_addr, msg.clone(), &socket).await;
                	}
               	}

            	// Fait le pont entre deux noeuds
                if let Message::Connect { dst_addr, dst_id, .. } = &msg {
                	let map = connected_peers_clone.lock().await;  // lock d'abord
				    if map.contains_key(dst_addr) {
				        drop(map);  // libère le lock avant le send
				        let _ = socket.send_msg(&msg, *dst_addr).await;
                		println!("Sent to {}: '{}'", dst_addr, msg);
				    } else {
				        eprintln!("Peer {} ({}) is logged out", dst_addr, dst_id);
				    }
               	}

               	// Répond aux demandes d'informations
                if let Message::AskForAddr { src_addr, peer_id, .. } = &msg {
                	let map = connected_peers_clone.lock().await;  // lock d'abord
				    if let Some((found_addr, _)) = map.iter().find(|(_, (id, _))| id == peer_id) {
				        let msg = Message::PeerInfo {
				            peer_addr: *found_addr,
				            peer_id: peer_id.clone(),
				        };
				        drop(map);  // libère le lock avant le send
				        let _ = socket.send_msg(&msg, *src_addr).await;
                		println!("{}", msg);
				    } else {
				        eprintln!("Peer {} not found", peer_id);
				    }
               	}
            }
            Err(e) => eprintln!("[ERROR]: a message contain an error ({})", e),
        }
    }
}

async fn relay_message(peers: &PeersMap, sender_addr: SocketAddr, msg: Message, socket: &UdpSocket) {
    let mut peers_map = peers.lock().await;

    for (other_addr, _) in peers_map.iter_mut() {
        if other_addr != &sender_addr {
            if let Err(e) = socket.send_msg(&msg, *other_addr).await {
                eprintln!("Failed to send to {}: {}", other_addr, e);
            } else {
                println!("    Relayed to {}", other_addr);
            }
        }
    }
}

async fn delete_disconnected_peers(peers: &PeersMap) {
    let mut peers_map = peers.lock().await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
    peers_map.retain(|addr, (_, last_seen)| {
        let active = now - *last_seen < 60;
        if !active { println!("[INFO] Peer {} disconnected (timeout)", addr); }
        active
    });
}