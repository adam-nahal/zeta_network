use tokio::net::{TcpStream, TcpListener, TcpSocket};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, Duration, timeout};
use std::net::SocketAddr;
use std::io::{self, Write};
use crate::{Opts, Mode};

use crate::nat_detector::nat_detector;


pub async fn main_client(opts: Opts) {
	// Description de ce noeud
	println!("Looking for NAT type and public IP address...");
	let (nat_type, public_addr) = nat_detector().await
		.expect("[ERROR] NAT type not detected");
    println!("   -> {:?}\n   -> {}", nat_type, public_addr);

    // Connexion au relai
    let ip_relay = opts.relay_ip.expect("--relay-ip est requis");
    let port_relay = opts.relay_port.expect("--relay-port est requis");
    let socket_relay = format!("{}:{}", ip_relay, port_relay);
    
    let mut relay_stream = TcpStream::connect(&socket_relay).await
        .expect(&format!("[ERROR] Can't connect to relay {}", socket_relay));
    let local_addr = relay_stream.local_addr().unwrap();
    println!("\nConnected to relay {}", socket_relay);

    println!("\n\n## Let's create direct connection with other peers ##\nOur local address: {}", local_addr);
    match opts.mode {
        Mode::Listen => {
            listen_mode(relay_stream, local_addr).await;
        }
        Mode::Dial => {
            let remote_peer_ip = opts.remote_peer_ip.expect("--remote-peer-ip requis");
            let remote_peer_port = opts.remote_peer_port.expect("--remote-peer-port requis");
            dial_mode(relay_stream, local_addr, &remote_peer_ip, &remote_peer_port).await;
        }
        _ => unreachable!()
    }
}

// ==================== MODE LISTEN ====================
async fn listen_mode(mut relay_stream: TcpStream, local_addr: SocketAddr) {
	// Étape 1 : Écouter jusqu'à recevoir l'adresse du peer Dial via le relai
    println!("Waiting for the dial's address (LISTEN MODE)...");
    let dial_peer_addr: SocketAddr = loop {
    	// Récupération des message reçu
    	let mut buf = [0; 1024];
    	let n = relay_stream.read(&mut buf).await.unwrap();
	    if n == 0 {
	    	println!("This pair is disconnected from the relay"); 
	    	return; 
	    }
	    let message = String::from_utf8_lossy(&buf[..n]).trim().to_string();

	    // Recherche de l'adresse dans le message du dial
	    if let Some(addr_str) = message.split('[').nth(1).and_then(|s| s.split(']').next()) {
	        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
	            println!("Received dial peer address: {}", addr);
	            break addr;
	        }
	    }

	    // Affichage du message reçu, si ce n'est pas celui attendu
	    println!("[RECEIVED] '{}'", message);
	};

    // Étape 2 : Annoncer au relai qu'on est prêt à recevoir
    let msg = format!("LISTEN_READY:{}\n", dial_peer_addr);
    relay_stream.write_all(msg.as_bytes()).await.unwrap();
    println!("Sent 'LISTEN_READY:{}' to relay", dial_peer_addr);
    
    // // Étape 3 : Test de connexion directe (avant hole punching)
    // let _ = relay_stream.shutdown().await;
    // let listener = TcpListener::bind(local_addr).await.unwrap();
    // println!("Listening...");

    // let (_, new_peer_address) = listener.accept().await.unwrap();
	// println!("New peer connected as {}", new_peer_address);

	let _ = relay_stream.shutdown().await;
    drop(relay_stream); // fermeture effective

    // Créer un socket d'écoute avec réutilisation d'adresse
    let socket = TcpSocket::new_v4().expect("Failed to create socket");
    socket.set_reuseaddr(true).expect("Failed to set SO_REUSEADDR");
    socket.bind(local_addr).expect("Failed to bind to local address");
    let listener = socket.listen(1024).expect("Failed to listen");

    println!("Listening on {} for direct connection from dial", local_addr);

    // Étape 4 : Accepter la connexion directe du pair Dial
    match listener.accept().await {
        Ok((direct_stream, peer_addr)) => {
            println!("✅ Direct connection established with {}", peer_addr);
            // Utiliser direct_stream pour la communication directe
        }
        Err(e) => {
            println!("Failed to accept direct connection: {}", e);
            return;
        }
    }
	


	
	// Étape 4 : Hole Punching - connect() simultané
	println!("🔨 Starting HOLE PUNCHING...");

}

// ==================== MODE DIAL ====================
async fn dial_mode(mut relay_stream: TcpStream, local_addr: SocketAddr, remote_peer_ip: &str, remote_peer_port: &str) {

	// Étape 0 : séparer le flux de données du canal vers le relay
	let (mut relay_read, mut relay_write) = relay_stream.split();
    
    // Étape 1 : Demander au relai de nous connecter au peer Listen
    println!("\nInitiating connection to {}:{} (DIAL MODE)...", remote_peer_ip, remote_peer_port);
    let msg = format!("DIAL_REQUEST:{}:{}\n", remote_peer_ip, remote_peer_port);
    relay_write.write_all(msg.as_bytes()).await.unwrap();
    println!("Sent 'DIAL_REQUEST:{}:{}' to relay", remote_peer_ip, remote_peer_port);
    
    // Étape 2 : Recevoir l'adresse du peer Listen via le relai
    let listen_peer_addr: SocketAddr = loop { 
    	// Récupération des message reçu
    	let mut buf = [0; 1024];
    	let n = relay_read.read(&mut buf).await.unwrap();
	    if n == 0 {
	    	println!("This pair is disconnected from the relay"); 
	    	return; 
	    }
	    let message = String::from_utf8_lossy(&buf[..n]).trim().to_string();

	    // Recherche de l'adresse dans le message du dial
	    if let Some(addr_str) = message.split('[').nth(1).and_then(|s| s.split(']').next()) {
	        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
	            println!("Received listen peer address: {}", addr);
	            break addr;
	        }
	    }

	    // Affichage du message reçu, si ce n'est pas celui attendu
	    println!("[RECEIVED] '{}'", message);
	};

    // Étape 3 : Test de connexion directe (avant hole punching)
    sleep(Duration::from_secs(3)).await;
    match timeout(Duration::from_secs(5), TcpStream::connect(listen_peer_addr)).await {
	    Ok(Ok(stream)) => {
	        println!("✓ Direct connection to {}", listen_peer_addr);
	        return;
	    }
	    _ => println!("Direct connection failed")
	}

	// Étape 4 : Hole Punching - connect() simultané
	println!("🔨 Starting HOLE PUNCHING...");

}