use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration, timeout};
use std::net::SocketAddr;

use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;

use crate::nat_detector::nat_detector;
use crate::nat_detector::util::NatType::*;
use crate::lib_p2p::*;


pub async fn main_client(peer_id: String, hubRelay_addr: SocketAddr) {
    // Initialisation du noeud
    println!("\nLooking for NAT type...");
    let (nat_type, _) = nat_detector().await
        .expect("[ERROR] NAT type not detected");
    println!("   -> {:?}\n", nat_type);  
    
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
    let _ = socket.send_msg(&msg, hubRelay_addr).await;  

    // Attends l'adresse d'un relais, de la part du hub relais
    let relay_addr = loop {
        // Récupération des messages reçu
        let Some((msg, _)) = recv_msg(&socket).await else { return };
        if let Message::PeerInfo {peer_addr, peer_id, ..} = &msg {
            println!("Received a relay address {} ({})", *peer_addr, peer_id.clone());
            break *peer_addr;
        }
        println!("'{}'", msg);
    };

    // Envoi du premier message au relai
    let msg = Message::Register {
        src_addr: public_addr,
        src_id: peer_id.clone(),
        dst_addr: relay_addr,
        dst_id: "relay1".to_string(),
        time: now_secs(),
    };
    let _ = socket.send_msg(&msg, relay_addr).await;

    // Ajout de ce noeud au réseau Zeta Network
    match nat_type {
        OpenInternet | FullCone | RestrictedCone | PortRestrictedCone => {
            println!("This node is become a relay ({:?})", nat_type);
            user_and_relay(socket, public_addr, peer_id).await;
        }
        SymmetricUdpFirewall | Symmetric => {
            println!("This node can't be a relay ({:?})", nat_type);
            user_only(socket, public_addr, peer_id).await;
        }
        Unknown | UdpBlocked => {
            println!("This node can't access the network ({:?})", nat_type);
            return;
        }
    }

    // match opts.mode {
    //     Mode::Listen => {
    //         listen_mode(socket, relay_addr, public_addr, peer_id.clone()).await;
    //     }
    //     Mode::Dial => {
    //         dial_mode(
    //             socket, relay_addr, 
    //             public_addr, peer_id,
    //             opts.listen_peer_id.expect("--listen-peer-id required").parse().expect("Wrong address format"),
    //             ).await;
    //     }
    //     _ => unreachable!()
    // }
}

pub async fn user_and_relay(socket: UdpSocket, public_addr: SocketAddr, peer_id: String) {
    // Le relay démarre l'écoute
    let socket = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind");
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

pub async fn user_only(socket: UdpSocket, public_addr: SocketAddr, peer_id: String) {
        
}


// // ==================== MODE LISTEN ====================
// async fn listen_mode(socket: UdpSocket, relay_addr: SocketAddr, public_addr: SocketAddr, peer_id: String) {
//     // Étape 1 : Écouter jusqu'à recevoir l'adresse/id du peer Dial via le relai
//     println!("Waiting for the dial's address (LISTEN MODE)...");
//     let (dial_peer_addr, dial_peer_id) = loop {
//         // Récupération des message reçu
//         let Some((msg, _)) = recv_msg(&socket).await else { return };
//         if let Message::Connect {src_addr, src_id, ..} = &msg {
//             println!("Received dial peer address {} ({}) from:\n    {}", *src_addr, src_id.clone(), msg);
//             break (*src_addr, src_id.clone());
//         }
//         println!("'{}'", msg);
//     };
    
//     // Étape 2 : Hole Punching (envoyer un message au DIAL même s'il va 
//     // certainement être intercepté par le NAT de ce dernier)
//     let msg = Message::Classic {
//         src_addr: public_addr,
//         src_id: peer_id.clone(),
//         dst_addr: dial_peer_addr,
//         dst_id: dial_peer_id.clone(),
//         time: now_secs(),
//         txt: "Hello dial, I am punching you, sorry".to_string(),
//     };
//     let _ = socket.send_msg(&msg, dial_peer_addr).await.unwrap();
//     println!("Sent '{}' to dial", msg);

//     // Étape 3 : Annoncer au dial qu'on est prêt à recevoir
//     let msg = Message::Classic {
//         src_addr: public_addr,
//         dst_addr: dial_peer_addr,
//         src_id: peer_id.clone(),
//         dst_id: dial_peer_id.clone(),
//         time: now_secs(),
//         txt: "Hello dial, I am waiting for your direct connection".to_string(),
//     };
//     let _ = socket.send_msg(&msg, relay_addr).await.unwrap();
//     println!("Sent '{}' to relay", msg);
    
//     // Étape 4 : Test de connexion directe (reception)
//     let timeout_result = timeout(Duration::from_secs(5), async { 
//         loop {
//             // Récupération des messages reçus
//             let Some((msg, _)) = recv_msg(&socket).await else { return };

//             if let Message::Classic {src_addr, src_id, ..} = &msg {
//                 if *src_addr == dial_peer_addr && src_id.clone() == dial_peer_id {  // Est-ce le dial ?
//                     println!("[SUCCEED] We can receive messages from {}", dial_peer_addr);
//                     break;
//                 }
//             }
//             // Sinon, affichage du message reçu
//             println!("{}", msg);
//         }
//     }).await;

//     if timeout_result.is_err() {
//         println!("[FAIL] We can not receive messages from {} (timeout)", dial_peer_addr);
//     }

//     // Étape 5 : Test de connexion directe (envoi)
//     sleep(Duration::from_secs(3)).await;
//     let msg = Message::Classic {
//         src_addr: public_addr,
//         dst_addr: dial_peer_addr,
//         src_id: peer_id.clone(),
//         dst_id: dial_peer_id.clone(),
//         time: now_secs(),
//         txt: "Hello dial, it is a direct connection".to_string(),
//     };
//     let _ = socket.send_msg(&msg, dial_peer_addr).await.unwrap();
//     println!("Sent '{}' to dial", msg);

//     return;
// }

// // ==================== MODE DIAL ====================
// async fn dial_mode(socket: UdpSocket, relay_addr: SocketAddr, public_addr: SocketAddr, peer_id: String, listen_peer_id: String) {
//     // Étape 0 : Demander au relai les informations sur le listen
//     println!("\nInitiating connection to {} (DIAL MODE)...", listen_peer_id);
//     let msg = Message::AskForAddr {
//         src_addr: public_addr,
//         src_id: peer_id.clone(),
//         time: now_secs(),
//         peer_id: listen_peer_id.clone(),
//     };
//     let _ = socket.send_msg(&msg, relay_addr).await.unwrap();
//     println!("Sent '{}' to relay", msg);
    
//     // Étape 1 : Recevoir l'adresse du peer Listen via le relai
//     let listen_peer_addr: SocketAddr = loop { 
//         // Récupération des message reçu
//         let Some((msg, _)) = recv_msg(&socket).await else { return };

//         if let Message::PeerInfo { peer_addr, peer_id, .. } = &msg {
//             if peer_id.clone() == listen_peer_id.clone() {
//                 println!("{}", msg);
//                 break *peer_addr;
//             }
//         } else {
//             println!("Received a message:\n    {}", msg);
//         }
//     };

//     // Étape 2 : Demander au relai de nous connecter au peer Listen
//     let msg = Message::Connect {
//         src_addr: public_addr,
//         src_id: peer_id.clone(),
//         dst_addr: listen_peer_addr,
//         dst_id: listen_peer_id.clone(),
//         time: now_secs(),
//     };
//     let _ = socket.send_msg(&msg, relay_addr).await.unwrap();
//     println!("Sent '{}' to relay", msg);

//     // Étape 3 : Test de connexion directe (envoi)
//     sleep(Duration::from_secs(1)).await;
//     let msg = Message::Classic {
//         src_addr: public_addr,
//         src_id: peer_id.clone(),
//         dst_addr: listen_peer_addr,
//         dst_id: listen_peer_id.clone(),
//         time: now_secs(),
//         txt: "Hello listen, it is a direct connection".to_string(),
//     };
//     socket.send_msg(&msg, listen_peer_addr).await.unwrap();
//     println!("Sent '{}' to relay", msg);

//     // Étape 4 : Test de connexion directe (reception)
//     let timeout_result = timeout(Duration::from_secs(10), async { 
//         loop {
//             // Récupération des messages reçus
//             let Some((msg, _)) = recv_msg(&socket).await else { return };

//             if let Message::Classic {src_addr, ..} = &msg {
//                 if *src_addr == listen_peer_addr {  // Est-ce le dial ?
//                     println!("[SUCCEED] We can receive messages from {}", listen_peer_addr);
//                     break;
//                 }
//             }
//             // Sinon, affichage du message reçu
//             println!("{}", msg);
//         }
//     }).await;

//     if timeout_result.is_err() {
//         println!("[FAIL] We can not receive messages from {} (timeout)", listen_peer_addr);
//     }
//     return;
// }