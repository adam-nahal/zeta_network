use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Message, UdpSocketExt, get_public_ip};
use crate::nat_detector::nat_detector;

type PeersMap = Arc<Mutex<HashMap<SocketAddr, u64>>>; // un noeud = [Addr, date dernière connection en sec]

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

    let mut buf = vec![0; 1024];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((size, peer_addr)) => {
                // Affichage du message
                let msg: Message = bincode::deserialize(&buf[..size]).expect("[ERROR] Deserialization failed");
                println!("{}", msg);

                // Ajout du client dans le repertoire
                let connected_peers_clone = Arc::clone(&peers_list);
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();
                connected_peers_clone.lock().await.insert(peer_addr, now);

                if public_addr != msg.dst {
                	relay_message(&connected_peers_clone, peer_addr, msg, &socket).await;
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