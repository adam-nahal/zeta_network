use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration, timeout};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::{Opts, Mode};
use crate::{Message, UdpSocketExt, get_public_ip};

use crate::nat_detector::nat_detector;


pub async fn main_client(opts: Opts) {
    // Initialisation du noeud
    println!("\nLooking for NAT type...");
    let (nat_type, _) = nat_detector().await
        .expect("[ERROR] NAT type not detected");
    println!("   -> {:?}\n", nat_type);

    let relay_addr = opts.relay_addr.expect("--relay-addr required").parse().expect("Wrong address format");
    let socket = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind");
    let public_addr:SocketAddr = get_public_ip(&socket).await.expect("Public IP not obtained.");
    println!("Socket created on public address {:?}", public_addr);

    let peer_id = opts.peer_id;

    // Envoi du premier message au relai
    let msg = Message::Register {
        src_addr: public_addr,
        src_id: peer_id.clone(),
        dst_addr: relay_addr,
        dst_id: "relay1".to_string(),
        time: SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs(),
    };
    let _ = socket.send_msg(&msg, relay_addr).await;

    // Ajout de ce noeud au réseau Zeta Network
    println!("\n\n## Let's create direct connection with other peers ##");
    match opts.mode {
        Mode::Listen => {
            listen_mode(socket, relay_addr, public_addr, peer_id.clone()).await;
        }
        Mode::Dial => {
            dial_mode(
                socket, relay_addr, 
                public_addr, peer_id,
                opts.listen_peer_id.expect("--listen-peer-id required").parse().expect("Wrong address format"),
                ).await;
        }
        _ => unreachable!()
    }
}

// ==================== MODE LISTEN ====================
async fn listen_mode(socket: UdpSocket, relay_addr: SocketAddr, public_addr: SocketAddr, peer_id: String) {
    // Étape 1 : Écouter jusqu'à recevoir l'adresse/id du peer Dial via le relai
    println!("Waiting for the dial's address (LISTEN MODE)...");
    let (dial_peer_addr, dial_peer_id) = loop {
        // Récupération des message reçu
        let mut buf = [0; 1024];
        let (size, sender_addr) = socket.recv_from(&mut buf).await.expect("Nothing received");
        if size <= 0 || size >= 1024  {
            println!("The message's size is incorrect({})", size); 
            return; 
        }
        let msg: Message = bincode::deserialize(&buf[..size]).expect("[ERROR] Deserialization failed");
        if let Message::Register {src_addr, src_id, ..} = &msg {
            if sender_addr == relay_addr {
                println!("Received dial peer address:\n    {}\n    -> {}\n    -> {}", msg, *src_addr, src_id.clone());
                break (*src_addr, src_id.clone());
            }
        }
        println!("'{}'", msg);
    };
    
    // Étape 2 : Hole Punching (envoyer un message au DIAL même s'il va 
    // certainement être intercepté par le NAT de ce dernier)
    let msg = Message::Classic {
        src_addr: public_addr,
        src_id: peer_id.clone(),
        dst_addr: dial_peer_addr,
        dst_id: dial_peer_id.clone(),
        time: SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs(),
        txt: "Hello dial, I am punching you, sorry".to_string(),
    };
    let _ = socket.send_msg(&msg, dial_peer_addr).await.unwrap();
    println!("Sent '{}' to dial", msg);

    // Étape 3 : Annoncer au dial qu'on est prêt à recevoir
    let msg = Message::Classic {
        src_addr: public_addr,
        dst_addr: dial_peer_addr,
        src_id: peer_id.clone(),
        dst_id: dial_peer_id.clone(),
        time: SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs(),
        txt: "Hello dial, I am waiting for your direct connection".to_string(),
    };
    let _ = socket.send_msg(&msg, relay_addr).await.unwrap();
    println!("Sent '{}' to relay", msg);
    
    // Étape 4 : Test de connexion directe (reception)
    let timeout_result = timeout(Duration::from_secs(5), async { 
        loop {
            // Récupération des messages reçus
            let mut buf = [0; 1024];
            let (size, _) = socket.recv_from(&mut buf).await.expect("Nothing received");
            if size <= 0 || size >= 1024  {
                println!("The message's size is incorrect({})", size); 
                return; 
            }
            let msg: Message = bincode::deserialize(&buf[..size]).expect("[ERROR] Deserialization failed");

            if let Message::Classic {src_addr, src_id, ..} = &msg {
                if *src_addr == dial_peer_addr && src_id.clone() == dial_peer_id {  // Est-ce le dial ?
                    println!("[SUCCEED] We can receive messages from {}", dial_peer_addr);
                    break;
                }
            }
            // Sinon, affichage du message reçu
            println!("{}", msg);
        }
    }).await;

    if timeout_result.is_err() {
        println!("[FAIL] We can not receive messages from {} (timeout)", dial_peer_addr);
    }

    // Étape 5 : Test de connexion directe (envoi)
    sleep(Duration::from_secs(3)).await;
    let msg = Message::Classic {
        src_addr: public_addr,
        dst_addr: dial_peer_addr,
        src_id: peer_id.clone(),
        dst_id: dial_peer_id.clone(),
        time: SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs(),
        txt: "Hello dial, it is a direct connection".to_string(),
    };
    let _ = socket.send_msg(&msg, dial_peer_addr).await.unwrap();
    println!("Sent '{}' to dial", msg);

}

// ==================== MODE DIAL ====================
async fn dial_mode(socket: UdpSocket, relay_addr: SocketAddr, public_addr: SocketAddr, peer_id: String, listen_peer_id: String) {
    // Étape 1 : Demander au relai de nous connecter au peer Listen
    println!("\nInitiating connection to {} (DIAL MODE)...", listen_peer_id);
    let msg = Message::AskForAddr {
        src_addr: public_addr,
        src_id: peer_id.clone(),
        time: SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs(),
        peer_id: listen_peer_id.clone(),
    };
    let _ = socket.send_msg(&msg, relay_addr).await.unwrap();
    println!("Sent '{}' to relay", msg);
    
    // Étape 2 : Recevoir l'adresse du peer Listen via le relai
    let listen_peer_addr: SocketAddr = loop { 
        // Récupération des message reçu
        let mut buf = [0; 1024];
        let (size, _) = socket.recv_from(&mut buf).await.expect("Nothing received");
        if size <= 0 || size >= 1024  {
            println!("The message's size is incorrect({})", size); 
            return; 
        }
        let msg: Message = bincode::deserialize(&buf[..size]).expect("[ERROR] Deserialization failed");

        if let Message::PeerInfo { peer_addr, peer_id, .. } = &msg {
            if peer_id.clone() == listen_peer_id.clone() {
                break *peer_addr;
            }
        } else {
            println!("Received a message:\n    {}", msg);
        }
    };

    // Étape 3 : Test de connexion directe (envoi)
    sleep(Duration::from_secs(1)).await;
    let msg = Message::Classic {
        src_addr: public_addr,
        src_id: peer_id.clone(),
        dst_addr: listen_peer_addr,
        dst_id: listen_peer_id.clone(),
        time: SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs(),
        txt: "Hello listen, it is a direct connection".to_string(),
    };
    socket.send_msg(&msg, listen_peer_addr).await.unwrap();
    println!("Sent 'IS_HOLE_PUNCHED' to relay");

    // Étape 4 : Test de connexion directe (reception)
    let timeout_result = timeout(Duration::from_secs(10), async { 
        loop {
            // Récupération des messages reçus
            let mut buf = [0; 1024];
            let (size, _) = socket.recv_from(&mut buf).await.expect("Nothing received");
            if size <= 0 || size >= 1024  {
                println!("The message's size is incorrect({})", size); 
                return; 
            }
            let msg: Message = bincode::deserialize(&buf[..size]).expect("[ERROR] Deserialization failed");

            if let Message::Classic {src_addr, ..} = &msg {
                if *src_addr == listen_peer_addr {  // Est-ce le dial ?
                    println!("[SUCCEED] We can receive messages from {}", listen_peer_addr);
                    break;
                }
            }
            // Sinon, affichage du message reçu
            println!("{}", msg);
        }
    }).await;

    if timeout_result.is_err() {
        println!("[FAIL] We can not receive messages from {} (timeout)", listen_peer_addr);
    }
}