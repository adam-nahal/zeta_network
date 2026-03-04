use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration, timeout};
use std::net::SocketAddr;
use crate::{Opts, Mode};

use crate::nat_detector::nat_detector;


pub async fn main_client(opts: Opts) {
	// Description de ce noeud
	println!("\nLooking for NAT type and public IP address...");
	let (nat_type, public_addr) = nat_detector().await
		.expect("[ERROR] NAT type not detected");
    println!("   -> {:?}\n   -> {}", nat_type, public_addr);

    // Envoi du premier message au relai
    let ip_relay = opts.relay_ip.expect("--relay-ip est requis");
    let port_relay = opts.relay_port.expect("--relay-port est requis");
    let addr_relay = format!("{}:{}", ip_relay, port_relay).parse().expect("Wrong address format");
    let socket = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind");
    let message = "HELLO_RELAY";
    let _ = socket.send_to(message.as_bytes(), &addr_relay).await;

    println!("\n\n## Let's create direct connection with other peers ##");
    match opts.mode {
        Mode::Listen => {
            listen_mode(socket, addr_relay).await;
        }
        Mode::Dial => {
            dial_mode(
                socket, addr_relay, 
                &opts.remote_peer_ip.expect("--remote-peer-ip requis"), 
                &opts.remote_peer_port.expect("--remote-peer-port requis")
                ).await;
        }
        _ => unreachable!()
    }
}

// ==================== MODE LISTEN ====================
async fn listen_mode(socket: UdpSocket, addr_relay: SocketAddr) {
	// Étape 1 : Écouter jusqu'à recevoir l'adresse du peer Dial via le relai
    println!("Waiting for the dial's address (LISTEN MODE)...");
    let dial_peer_addr: SocketAddr = loop {
    	// Récupération des message reçu
    	let mut buf = [0; 1024];
    	let (size, peer_addr) = socket.recv_from(&mut buf).await.expect("Nothing received");
	    if size <= 0 || size >= 1024  {
	    	println!("The message's size is incorrect({})", size); 
	    	return; 
	    }
	    let message = String::from_utf8_lossy(&buf[..size]).trim().to_string();

	    // Recherche de l'adresse dans le message du dial
	    if let Some(addr_str) = message.split('[').nth(1).and_then(|s| s.split(']').next()) {
	        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
	            println!("Received dial peer address: {}", addr);
	            break addr;
	        }
	    }

	    // Affichage du message reçu, si ce n'est pas celui attendu
	    println!("[{}] {}", peer_addr, message);
	};

    // Étape 2 : Annoncer au relai qu'on est prêt à recevoir
    let msg = format!("LISTEN_READY:{}", dial_peer_addr);
    let _ = socket.send_to(msg.as_bytes(), addr_relay).await.unwrap();
    println!("Sent '{}' to relay", msg);
    
    // Étape 3 : Hole Punching (envoyer un message au DIAL même s'il va 
    // certainement être intercepté par le NAT de ce dernier)
    let msg = format!("PUNCHING_THE_HOLE:{}", dial_peer_addr);
    let _ = socket.send_to(msg.as_bytes(), dial_peer_addr).await.unwrap();
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
		    let message = String::from_utf8_lossy(&buf[..size]).trim().to_string();

		    // Recherche de l'adresse dans le message du dial
		    if let Some(addr_sender) = message.split('[').nth(1).and_then(|s| s.split(']').next()) {
		        if let Ok(addr_sender) = addr_sender.parse::<SocketAddr>() {
		        	if addr_sender == dial_peer_addr {
		            	println!("[SUCCEED] We can receive messages from {}", dial_peer_addr);
		            	break;
		            }
				    // Affichage du message reçu, si ce n'est pas celui attendu
				    println!("[{}] {}", addr_sender, message);
		        }
		    }
		}
	}).await;

	if timeout_result.is_err() {
		println!("[FAIL] We can not receive messages from {} (timeout)", dial_peer_addr);
	}

	// Étape 5 : Test de connexion directe (envoi)
    sleep(Duration::from_secs(3)).await;
    let msg = format!("HELLO_DIAL_FROM_LISTEN:{}", dial_peer_addr);
    let _ = socket.send_to(msg.as_bytes(), dial_peer_addr).await.unwrap();
    println!("Sent '{}' to dial", msg);

}

// ==================== MODE DIAL ====================
async fn dial_mode(socket: UdpSocket, addr_relay: SocketAddr, remote_peer_ip: &str, remote_peer_port: &str) {
    // Étape 1 : Demander au relai de nous connecter au peer Listen
    println!("\nInitiating connection to {}:{} (DIAL MODE)...", remote_peer_ip, remote_peer_port);
    let msg = format!("DIAL_REQUEST:{}:{}", remote_peer_ip, remote_peer_port);
    let _ = socket.send_to(msg.as_bytes(), addr_relay).await.unwrap();
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
	    let message = String::from_utf8_lossy(&buf[..size]).trim().to_string();

	    // Recherche de l'adresse dans le message du dial
	    if let Some(addr_str) = message.split('[').nth(1).and_then(|s| s.split(']').next()) {
	        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
	            println!("Received listen peer address: {}", addr);
	            break addr;
	        }
	    }

	    // Affichage du message reçu, si ce n'est pas celui attendu
	    println!("[{}] '{}'",addr_relay, message);
	};

    // Étape 3 : Test de connexion directe (envoi)
    sleep(Duration::from_secs(1)).await;
    socket.send_to("IS_HOLE_PUNCHED".as_bytes(), listen_peer_addr).await.unwrap();
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
		    let message = String::from_utf8_lossy(&buf[..size]).trim().to_string();
	    	println!("Message received: '{}'", message);

		    // Recherche de l'adresse dans le message du dial
		    if let Some(addr_sender) = message.split('[').nth(1).and_then(|s| s.split(']').next()) {
		        if let Ok(addr_sender) = addr_sender.parse::<SocketAddr>() {
		        	if addr_sender == listen_peer_addr {
		            	println!("[SUCCEED] We can receive messages from {}", listen_peer_addr);
		            	break;
		            }
				    // Affichage du message reçu, si ce n'est pas celui attendu
				    println!("[{}] {}", addr_sender, message);
		        }
		    }
		}
	}).await;

	if timeout_result.is_err() {
		println!("[FAIL] We can not receive messages from {} (timeout)", listen_peer_addr);
	}
}