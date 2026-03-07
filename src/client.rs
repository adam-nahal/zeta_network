use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration, timeout};
use std::net::SocketAddr;
use crate::{Opts, Mode};
use crate::{Message, UdpSocketExt, get_public_ip};

use crate::nat_detector::nat_detector;


pub async fn main_client(opts: Opts) {
	// Initialisation du noeud
	println!("\nLooking for NAT type...");
	let (nat_type, _) = nat_detector().await
		.expect("[ERROR] NAT type not detected");
    println!("   -> {:?}\n", nat_type);

    let addr_relay = opts.relay_addr.expect("--relay-addr est requis").parse().expect("Wrong address format");
    let socket = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind");
    let public_addr:SocketAddr = get_public_ip(&socket).await.expect("Public IP not obtained.");
    println!("Socket created on public address {:?}", public_addr);

    // Envoi du premier message au relai
    let _ = socket.send_txt(public_addr, addr_relay, "HELLO_RELAY", addr_relay).await;

    // Ajout de ce noeud au réseau Zeta Network
    println!("\n\n## Let's create direct connection with other peers ##");
    match opts.mode {
        Mode::Listen => {
            listen_mode(socket, addr_relay, public_addr).await;
        }
        Mode::Dial => {
            dial_mode(
                socket, addr_relay, 
                public_addr,
                opts.remote_peer_addr.expect("--remote-peer-addr required").parse().expect("Wrong address format"),
                ).await;
        }
        _ => unreachable!()
    }
}

// ==================== MODE LISTEN ====================
async fn listen_mode(socket: UdpSocket, addr_relay: SocketAddr, public_addr: SocketAddr) {
	// Étape 1 : Écouter jusqu'à recevoir l'adresse du peer Dial via le relai
    println!("Waiting for the dial's address (LISTEN MODE)...");
    let dial_peer_addr: SocketAddr = loop {
    	// Récupération des message reçu
    	let mut buf = [0; 1024];
    	let (size, _) = socket.recv_from(&mut buf).await.expect("Nothing received");
	    if size <= 0 || size >= 1024  {
	    	println!("The message's size is incorrect({})", size); 
	    	return; 
	    }
	    let msg: Message = bincode::deserialize(&buf[..size]).expect("[ERROR] Deserialization failed");
	    println!("Received dial peer address:\n    {}", msg);
	    break msg.src;
	};

    // Étape 2 : Annoncer au relai qu'on est prêt à recevoir
    let msg = format!("LISTEN_READY:{}", dial_peer_addr);
    let _ = socket.send_txt(public_addr, addr_relay, &msg, addr_relay).await.unwrap();
    println!("Sent '{}' to relay", msg);
    
    // Étape 3 : Hole Punching (envoyer un message au DIAL même s'il va 
    // certainement être intercepté par le NAT de ce dernier)
    let msg = format!("PUNCHING_THE_HOLE:{}", dial_peer_addr);
    let _ = socket.send_txt(public_addr, dial_peer_addr, &msg, dial_peer_addr).await.unwrap();
    println!("Sent '{}' to dial", msg);
    
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

        	if msg.src == dial_peer_addr {  // Est-ce le dial ?
            	println!("[SUCCEED] We can receive messages from {}", dial_peer_addr);
            	break;
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
    let msg = format!("HELLO_DIAL_FROM_LISTEN:{}", dial_peer_addr);
    let _ = socket.send_txt(public_addr, dial_peer_addr, &msg, dial_peer_addr).await.unwrap();
    println!("Sent '{}' to dial", msg);

}

// ==================== MODE DIAL ====================
async fn dial_mode(socket: UdpSocket, addr_relay: SocketAddr, public_addr: SocketAddr, remote_peer_addr: SocketAddr) {
    // Étape 1 : Demander au relai de nous connecter au peer Listen
    println!("\nInitiating connection to {} (DIAL MODE)...", remote_peer_addr);
    let msg = format!("DIAL_REQUEST:{}", remote_peer_addr);
    let _ = socket.send_txt(public_addr, addr_relay, &msg, addr_relay).await.unwrap();
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
	    println!("Received listen peer address:\n    {}", msg);
	    break msg.src;
	};

    // Étape 3 : Test de connexion directe (envoi)
    sleep(Duration::from_secs(1)).await;
    let msg = format!("IS_HOLE_PUNCHED");
    socket.send_txt(public_addr, listen_peer_addr, &msg, listen_peer_addr).await.unwrap();
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

        	if msg.src == listen_peer_addr {  // Est-ce le dial ?
            	println!("[SUCCEED] We can receive messages from {}", listen_peer_addr);
            	break;
            }
		    // Sinon, affichage du message reçu
		    println!("{}", msg);
		}
	}).await;

	if timeout_result.is_err() {
		println!("[FAIL] We can not receive messages from {} (timeout)", listen_peer_addr);
	}
}