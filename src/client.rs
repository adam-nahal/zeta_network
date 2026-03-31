use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration, timeout};
use std::net::SocketAddr;

use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;

use crate::nat_detector::nat_detector;
use crate::nat_detector::util::NatType::*;
use crate::lib_p2p::*;


pub async fn main_client(peer_id: String, hub_relay_addr: SocketAddr) {
    // Initialisation du noeud
    println!("\nLooking for NAT type...");
    let (nat_type, _) = nat_detector().await
        .expect("[ERROR] NAT type not detected");
    println!("   -> {:?}\n", nat_type);  

    // Vérifie l'accès réseau dès le début
    if matches!(nat_type, Unknown | UdpBlocked) {
        println!("This node can't access the network ({:?})", nat_type);
        return;
    }
    
    // Crée le socket pour envoyer des messages
    let socket = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind");
    let public_addr:SocketAddr = get_public_ip(&socket).await
        .expect("Public IP not obtained.");
    println!("Socket created on public address {:?}", public_addr);

    // Demande un relais disponible au hubRelay
    println!("Asking the hub relay an available relay...");
    let msg = Message::NeedRelay {
        src_addr: public_addr,
        src_id: peer_id.clone(),
        time: now_secs(),
    };
    let _ = socket.send_msg(&msg, hub_relay_addr).await;
    println!("->({})", msg);

    // Attends l'adresse d'un relais, de la part du hub relais
    let (mut relay_addr, mut relay_id) = (None, None);
    loop {
        let Some((msg, _)) = recv_msg(&socket).await else { continue };
        println!("<-({})", msg);
        match &msg {
            Message::PeerInfo { peer_addr, peer_id, .. } => {
                println!("Received relay address {} ({})", peer_addr, peer_id);
	            relay_addr = Some(*peer_addr);
	            relay_id = Some(peer_id.clone());
                break;
            }
            Message::NoRelayAvailable { .. } => {
                println!("[WARN] No relays available on the network");
                break;
            }
            _ => println!("Unexpected message: '{}'", msg),
        }
    };

    // Envoi du premier message au relais
    if let (Some(relay_addr), Some(relay_id)) = (relay_addr, relay_id) {
        let msg = Message::Register {
            src_addr: public_addr,
            src_id: peer_id.clone(),
            dst_addr: relay_addr,
            dst_id: relay_id.clone(),
            time: now_secs(),
        };
        let _ = socket.send_msg(&msg, relay_addr).await;
        println!("->({})", msg);
        
		if !wait_for_ack(&socket).await {
		    println!("[ERROR] No ack from relay, aborting");
		    return;
		} 
    } else {
        println!("[WARN] No relay, skipping registration");
    }

    // Ajout de ce noeud au réseau Zeta Network
    match nat_type {
        OpenInternet | FullCone | RestrictedCone | PortRestrictedCone => {
            println!("This node is become a relay ({:?})", nat_type);
            user_and_relay(socket, public_addr, peer_id, hub_relay_addr).await;
        }
        _ => {  // SymmetricUdpFirewall or Symmetric
            println!("This node can't be a relay ({:?})", nat_type);
            user_only(socket, public_addr, peer_id).await;
        }
    }
}

pub async fn user_and_relay(socket: UdpSocket, public_addr: SocketAddr, peer_id: String, hub_relay_addr: SocketAddr) {
    // Crée la liste de tous les clients qui ont contacté ce relai
    let peers_list: PeersMap = Arc::new(Mutex::new(HashMap::new()));

    // Suppression automatique des noeuds inactifs
    let peers_cleanup = Arc::clone(&peers_list);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(10)).await;
            delete_disconnected_peers(&peers_cleanup).await;
        }
    });

    // S'enregistre auprès du hubrelay en tant que relay
    println!("Asking the hub relay to be relay...");
    let msg = Message::BeNewRelay {
        src_addr: public_addr,
        src_id: peer_id.clone(),
        time: now_secs(),
    };
    let _ = socket.send_msg(&msg, hub_relay_addr).await;  
    println!("->({})", msg);

    if !wait_for_ack(&socket).await {
	    println!("[ERROR] No ack from relay, aborting");
	    return;
	}   

    loop {
    	let Some((msg, sender_addr)) = recv_msg(&socket).await else {continue};
    	println!("<-{}", msg);

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
                println!("->({})", msg);
                println!("Sent to {}: '{}'", dst_addr, msg);
            } else {
                eprintln!("Peer {} ({}) is unknown", dst_addr, dst_id);
            }
        }

        // Ce relais a un nouveau client qui veut se connecter -> hole punching
        if let Message::RelayHasNewClient { peer_addr, peer_id: client_id, time, .. } = &msg {           
            connected_peers_clone.lock().await
                .entry(sender_addr)  // La clé existe-t-elle déjà ?
                .and_modify(|(_, t)| *t = *time)
                .or_insert((peer_id.clone(), *time));
            let msg = Message::PunchTheHole {
                src_addr: public_addr,
                src_id: peer_id.clone(),
                dst_addr: *peer_addr,
                dst_id: client_id.to_string(),
                time: now_secs(),
            };
            let _ = socket.send_msg(&msg, *peer_addr).await;
            println!("->({})", msg);
            
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
                println!("->({})", msg);                
                println!("{}", msg);
            } else {
                eprintln!("Peer {} not found", peer_id);
            }
        }
    }
}

pub async fn user_only(socket: UdpSocket, public_addr: SocketAddr, peer_id: String) {
        
}
