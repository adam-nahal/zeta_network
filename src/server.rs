use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;
use std::net::{SocketAddr, IpAddr};
type PeersMap = Arc<Mutex<HashMap<SocketAddr, tokio::net::TcpStream>>>;  // Raccourci


#[tokio::main]
async fn main() {
    // Le relay démarre l'écoute 
    let port_relay = 12345;
    let socket_relay = SocketAddr::new(IpAddr::from([0, 0, 0, 0]), port_relay);
    
    let listener = TcpListener::bind(&socket_relay).await.unwrap();
    println!("Listening on port {}...", port_relay);
    
    // Crée la liste de tous les clients connectés à ce relai
    let connected_peers: PeersMap = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (new_peer_socket, new_peer_address) = listener.accept().await.unwrap();  // En arrière plan avec .await()
	    println!("New peer connected as {}", new_peer_address);

        // Ajout du nouveau client dans le repertoire
		let connected_peers_clone = Arc::clone(&connected_peers);
		connected_peers_clone.lock().await.insert(new_peer_address, new_peer_socket);

	    // Écoute des messages recus depuis ce nouveau peer connecté, et broadcast 
	    tokio::spawn(handle_peer_connection(connected_peers_clone, new_peer_address));
    }
}

async fn handle_peer_connection(peers_ref: PeersMap, current_peer_address: SocketAddr) {
    let mut buffer = [0; 512];  // Pour stocker les message que ce nouveau peer envoie au relai
    
    loop {
        let mut peers_map = peers_ref.lock().await;
        let socket = match peers_map.get_mut(&current_peer_address) {
            Some(s) => s,
            None => break,
        };
        
        match socket.read(&mut buffer).await {
            Ok(0) => {  // Ce peer est déconnecté du relai
                drop(peers_map);
                peers_ref.lock().await.remove(&current_peer_address);  // Suppression de la liste des peers connectés
                println!("❌ Peer disconnected: {}", current_peer_address);
                break;
            }
            Ok(n) => {  // Ce peer a envoyé un message au relai
                let message = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
                println!("📨 Message from {}: {}", current_peer_address, message);
                
                drop(peers_map);
                relay_message(&peers_ref, current_peer_address, &message).await;
            }
            Err(e) => {
                println!("⚠️  Error: {}", e);
                drop(peers_map);
                peers_ref.lock().await.remove(&current_peer_address);
                break;
            }
        }
    }
}

async fn relay_message(peers: &PeersMap, sender_addr: SocketAddr, message: &str) {
    let mut peers_map = peers.lock().await;
    
    for (other_addr, other_socket) in peers_map.iter_mut() {
        if other_addr != &sender_addr {  // Ne pas envoyer au peer qui a initié le message
            let formatted = format!("[{}] {}\n", sender_addr, message);
            match other_socket.write_all(formatted.as_bytes()).await {
                Ok(_) => println!("  → Message sent to {}", other_addr),
                Err(e) => println!("  ⚠️  Error: {}", e),
            }
        }
    }
}