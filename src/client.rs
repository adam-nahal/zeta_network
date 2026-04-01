use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration};
use std::net::SocketAddr;

use tokio::sync::Mutex;
use std::sync::Arc;
use tokio::io::{stdin, AsyncBufReadExt, BufReader};
use std::io::Write;
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
	let socket = Arc::new(socket);
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
    println!("Asking the hub relay to be a relay...");
    let msg = Message::BeNewRelay {
    	header: MessageHeader {
	        src_addr: public_addr,
	        src_id: peer_id.clone(),
	        dst_addr: hub_relay_addr,
	        dst_id: "hubrelay".to_string(),
	        time: now_secs(),
	    }
    };
    let _ = socket.send_msg(&msg, hub_relay_addr).await;  
    println!("->({})", msg);

    if !wait_for_ack(&socket).await {
	    println!("[ERROR] No ack from relay, aborting");
	    return;
	}

	// Demande au hub relais l'adresse d'un relais
	let Some((relay_addr, _relay_id)) = connect_to_a_relay(&socket, public_addr, &peer_id, hub_relay_addr)
		.await else {return};

	// Boucle de réception
    let recv_socket = Arc::clone(&socket);
    let recv_peers_list = Arc::clone(&peers_list);
    let recv_peer_id = peer_id.clone();
    tokio::spawn(async move {
    	loop {
	    	let Some((msg, sender_addr)) = recv_msg(&recv_socket).await else {continue};
	    	println!("<-{}", msg);

	        // Ajout des nouveaux noeuds ou mise à jour de la dernière connection
	        let connected_peers_clone = Arc::clone(&recv_peers_list);
	        if let Message::Register { header } = &msg {
	            connected_peers_clone.lock().await
	                .entry(sender_addr)  // La clé existe-t-elle déjà ?
	                .and_modify(|(_, t)| *t = header.time)
	                .or_insert((header.src_id.clone(), header.time));
	        }

	        // Relaie le message si c'est un message à relayer
	        if let Message::Classic { header, .. } = &msg {
	            if public_addr != header.dst_addr {
	                relay_message(&connected_peers_clone, sender_addr, msg.clone(), &recv_socket).await;
	            }
	        }

	        // Fait le pont entre deux noeuds
	        if let Message::Connect { header } = &msg {
	            let map = connected_peers_clone.lock().await;  // lock d'abord
	            if map.contains_key(&header.dst_addr) {
	                drop(map);  // libère le lock avant le send
	                let _ = recv_socket.send_msg(&msg, header.dst_addr).await;
	                println!("->({})", msg);
	                println!("Sent to {}: '{}'", header.dst_addr, msg);
	            } else {
	                eprintln!("Peer {} ({}) is unknown", header.dst_addr, header.dst_id);
	            }
	        }

	        // Ce relais a un nouveau client qui veut se connecter -> hole punching
	        if let Message::RelayHasNewClient { header, peer_addr, peer_id: client_id } = &msg {           
	            connected_peers_clone.lock().await
	                .entry(sender_addr)  // La clé existe-t-elle déjà ?
	                .and_modify(|(_, t)| *t = header.time)
	                .or_insert((recv_peer_id.clone(), header.time));
	            let msg = Message::PunchTheHole {
	            	header: MessageHeader {
		                src_addr: public_addr,
		                src_id: recv_peer_id.clone(),
		                dst_addr: *peer_addr,
		                dst_id: client_id.to_string(),
		                time: now_secs(),
	            	}
	            };
	            let _ = recv_socket.send_msg(&msg, *peer_addr).await;
	            println!("->({})", msg);
	            
	        }

	        // Répond aux demandes d'informations
	        if let Message::AskForAddr { header, peer_id } = &msg {
	            let map = connected_peers_clone.lock().await;  // lock d'abord
	            if let Some((found_addr, _)) = map.iter().find(|(_, (id, _))| *id == *peer_id) {
	                let msg = Message::PeerInfo {
	                	header: MessageHeader {
			                src_addr: public_addr,
			                src_id: recv_peer_id.clone(),
			                dst_addr: header.src_addr,
			                dst_id: header.src_id.clone(),
			                time: now_secs(),
	                	},
	                    peer_addr: *found_addr,
	                    peer_id: peer_id.clone(),
	                };
	                drop(map);  // libère le lock avant le send
	                let _ = recv_socket.send_msg(&msg, header.src_addr).await;
	                println!("->({})", msg);                
	                println!("{}", msg);
	            } else {
	                eprintln!("Peer {} not found", recv_peer_id);
	            }
	        }
	    }
    });

    // Boucle d'envoi
    let send_socket = Arc::clone(&socket);
	tokio::spawn(async move {
	    let mut reader = BufReader::new(stdin());
	    let mut line = String::new();
	    loop {
	        // Affichage du prompt (bloquant mais sans conséquence pour un CLI)
	        print!("> ");
	        std::io::stdout().flush().unwrap();

	        line.clear();
	        match reader.read_line(&mut line).await {
	            Ok(0) => break, // EOF (Ctrl+D)
	            Ok(_) => {
	                let msg_text = line.trim();
	                if msg_text == "/q" {
	                    break;
	                }
	                if msg_text.is_empty() {
	                    continue;
	                }
	                let msg = Message::Classic {
	                	header: MessageHeader {
		                    src_addr: public_addr,
		                    src_id: peer_id.clone(),
		                    dst_addr: "0.0.0.0:0".parse().unwrap(),
		                    dst_id: "all".to_string(),
		                    time: now_secs(),
	                	},
	                    txt: msg_text.to_string(),
	                };
	                if let Err(e) = send_socket.send_msg(&msg, relay_addr).await {
	                    eprintln!("Erreur d’envoi : {}", e);
	                } else {
	                    println!("->({})", msg);
	                }
	            }
	            Err(e) => {
	                eprintln!("Erreur de lecture : {}", e);
	                break;
	            }
	        }
	    }
	});
}

pub async fn user_only(_socket: UdpSocket, _public_addr: SocketAddr, _peer_id: String) {
        
}
