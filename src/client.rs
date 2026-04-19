use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{stdin, AsyncBufReadExt, BufReader};
use std::io::Write;

use crate::nat_detector::nat_detector;
use crate::nat_detector::util::NatType::*;
use crate::p2p::*;
use crate::db::*;

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
            user_and_relay(socket, public_addr, peer_id, hub_relay_addr).await;
        }
        _ => {  // SymmetricUdpFirewall or Symmetric
            user_only(socket, public_addr, peer_id, hub_relay_addr).await;
        }
    }
}

pub async fn user_and_relay(socket: UdpSocket, public_addr: SocketAddr, peer_id: String, hub_relay_addr: SocketAddr) {
	let socket = Arc::new(socket);

	// Création des clés d'authentification
	let auth_keys: AuthKeys = AuthKeys::generate();

	// Initialise la base de données et initialise le compteur msg_id
	let db = DbManager::new("client.db").await.expect("Cannot open the database");
	let _ = init_msg_id(&db, public_addr).await;

	// Importe les données de la base de données vers le programme
	let peers: PeersMap = db.get_peers_from_db().await.expect("[ERROR] Initialisation of database failed");
	let logs: MessagesMap = db.get_logs_from_db().await.expect("[ERROR] Initialisation of database failed");

	// Crée le dispatcher
	let (dispatcher, inbox) = create_node_channels();
	let NodeInbox { ack_waiter, mut hub_rx, mut general_rx } = inbox;
	tokio::spawn(dispatcher.run(Arc::clone(&socket), Arc::clone(&logs), Arc::clone(&peers)));

    // Suppression automatique des noeuds inactifs
    tokio::spawn({
    	let peers = Arc::clone(&peers);
    	async move {
	        loop {
	            sleep(Duration::from_secs(10)).await;
	            delete_disconnected_peers(&peers).await;
	        }
	    }
    });

    // Actualisation automatique de la base de données
	tokio::spawn({
	    let peers = Arc::clone(&peers);
	    let logs = Arc::clone(&logs);
	    let db = db.clone();
	    async move {
	        loop {
	            sleep(Duration::from_secs(5)).await;
	            let _ = db.refresh_peers_in_db(&peers).await.expect("[ERROR] refresh_peers_in_db failed");
	            let _ = db.refresh_logs_in_db(&logs).await.expect("[ERROR] refresh_logs_in_db failed");
	        }
	    }
	});

	// Boucle de réception
    tokio::spawn({
	    let socket = Arc::clone(&socket);
	    let peers = Arc::clone(&peers);
	    let logs = Arc::clone(&logs);
	    let peer_id = peer_id.clone();
	    let auth_keys = auth_keys.clone();
	    async move {
	    	while let Some((msg_rcv, _)) = general_rx.recv().await {
		        // Ajout des nouveaux noeuds ou mise à jour de la dernière connection
		        let peers = Arc::clone(&peers);
		        if let Payload::Register { verifying_key } = &msg_rcv.payload {
		            peers.lock().await
		                .entry(peer_id_from_verifying_key(verifying_key))  // La clé existe-t-elle déjà ?
		                .and_modify(|peer_info| peer_info.last_seen = now_secs())
		                .or_insert(PeerInfo {
		                	id: peer_id_from_verifying_key(verifying_key),
						    addr: msg_rcv.headers.src_addr,
						    username: msg_rcv.headers.src_id.clone(),
						    last_seen: now_secs(),
						    is_relay: false,
						    verifying_key: *verifying_key
					});

		            let mut msg = Message {
			            headers: Headers {
			                msg_id:   new_msg_id(),
			                src_addr: public_addr,
			                src_id:   peer_id.clone(),
			                dst_addr: msg_rcv.headers.src_addr,
			                dst_id:   msg_rcv.headers.src_id.clone(),
			                time:     now_secs(),
							signature: vec![], 
			            },
			            payload: Payload::Ack { reply_to: msg_rcv.headers.msg_id },
						last_hop: public_addr,
			        };
					let _ = msg.sign(&auth_keys.signing_key);
			        let _ = socket.send_msg(msg, msg_rcv.headers.src_addr, &logs).await;
		        }

		        // Relaie le message si c'est un message à relayer
		        if let Payload::Classic { .. } = &msg_rcv.payload {
		            if public_addr != msg_rcv.headers.dst_addr {
		                relay_message(&peers, public_addr, msg_rcv.clone(), &socket, &logs).await;
		            }
		        }

		        // Fait le pont entre deux noeuds
		        if let Payload::Connect = &msg_rcv.payload {
		            let peers = peers.lock().await;
		            if peers.values().any(|info| info.addr == msg_rcv.headers.dst_addr) {
		                drop(peers);  // libère le lock avant le send
		                let _ = socket.send_msg(msg_rcv.clone(), msg_rcv.headers.dst_addr, &logs).await;
		                println!("Sent to {}: '{}'", msg_rcv.headers.dst_addr, msg_rcv);
		            } else {
		                eprintln!("Peer {} ({}) is unknown", msg_rcv.headers.dst_addr, msg_rcv.headers.dst_id);
		            }
		        }

		        // Ce relais a un nouveau client qui veut se connecter -> hole punching
		        if let Payload::RelayHasNewClient { peer_addr, peer_id: client_id, .. } = &msg_rcv.payload {
		            let mut msg = Message {
		            	headers: Headers {
	            			msg_id: new_msg_id(),
			                src_addr: public_addr,
			                src_id: peer_id.clone(),
			                dst_addr: *peer_addr,
			                dst_id: client_id.to_string(),
			                time: now_secs(),
							signature: vec![], 
		            	},
		            	payload: Payload::PunchTheHole,
						last_hop: public_addr,
		            };
					let _ = msg.sign(&auth_keys.signing_key);
		            let _ = socket.send_msg(msg, *peer_addr, &logs).await;
		            
		        }

		        // Répond aux demandes d'informations
		        if let Payload::AskForAddr { username: username_rcv } = &msg_rcv.payload {
		            let peers = peers.lock().await;  // lock d'abord
		            if let Some((_, peer_info)) = peers.iter().find(|(_, peer_info)| peer_info.username == *username_rcv) {
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
		                	payload: Payload::PeerInfo { peer_info: peer_info.clone() },
							last_hop: public_addr,
		                };
		                drop(peers);  // libère le lock avant le send
						let _ = msg.sign(&auth_keys.signing_key);
		                let _ = socket.send_msg(msg, msg_rcv.headers.src_addr, &logs).await;
		            } else {
		                eprintln!("Peer {} not found", username_rcv);
		            }
		        }
		    }
		}
    });

    // S'enregistre auprès du hubrelay en tant que relay
    println!("\nAsking the hub relay to be a relay...");
    let mut msg = Message {
    	headers: Headers {
            msg_id: new_msg_id(),
	        src_addr: public_addr,
	        src_id: peer_id.clone(),
	        dst_addr: hub_relay_addr,
	        dst_id: "hub".to_string(),
	        time: now_secs(),
			signature: vec![], 
	    },
	    payload: Payload::BeNewRelay { verifying_key: auth_keys.verifying_key },
		last_hop: public_addr,
    };
    let _ = msg.sign(&auth_keys.signing_key);
    while !socket.send_and_wait_ack(msg.clone(), hub_relay_addr, &ack_waiter, &logs).await {
    	msg.headers.msg_id = new_msg_id();
    	msg.headers.time = now_secs();
    	let _ = msg.sign(&auth_keys.signing_key);
    }

	// Demande au hub relais l'adresse d'un relais
	let Some((relay_addr, _relay_id)) = connect_to_a_relay (
		&socket, public_addr, &peer_id, hub_relay_addr, 
		&mut hub_rx, &ack_waiter, &logs, &auth_keys
	).await else {return};

    // Boucle d'envoi
	tokio::spawn({
    	let socket = Arc::clone(&socket);
    	let auth_keys = auth_keys.clone();
    	async move {
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
		                let mut msg = Message {
		                	headers: Headers {
	            				msg_id: new_msg_id(),
			                    src_addr: public_addr,
			                    src_id: peer_id.clone(),
			                    dst_addr: "0.0.0.0:0".parse().unwrap(),
			                    dst_id: "all".to_string(),
			                    time: now_secs(),
								signature: vec![], 
		                	},
		                	payload: Payload::Classic { txt: msg_text.to_string() },
		                	last_hop: public_addr,
		                };
		                let _ = msg.sign(&auth_keys.signing_key);
		                let _ = socket.send_msg(msg, relay_addr, &logs).await;
		            }
		            Err(e) => {
		                eprintln!("Erreur de lecture : {}", e);
		                break;
		            }
		        }
		    }
		}
	});

	// Pour éviter que le programme quit (tout est parallèle jusqu'ici...)
	std::future::pending::<()>().await;
}

pub async fn user_only(_socket: UdpSocket, _public_addr: SocketAddr, _peer_id: String, _hub_relay_addr: SocketAddr) {
}
