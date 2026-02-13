use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;
use std::net::{SocketAddr, IpAddr};


type PeersMap = Arc<Mutex<HashMap<SocketAddr, tokio::net::TcpStream>>>;  // Raccourci

#[derive(Clone)]
struct Peer {
	address: SocketAddr
}


#[tokio::main]
async fn main() {
    // Le relay démarre l'écoute 
    let port_relay = 12345u16;
    let socket_relay = SocketAddr::new(IpAddr::from([0, 0, 0, 0]), port_relay);
    
    let listener = TcpListener::bind(&socket_relay).await.unwrap();
    println!("Listening on  {}...", socket_relay);
    
    // Crée la liste de tous les clients connectés à ce relai
    let connected_peers: PeersMap = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (socket_peer, addr_peer) = listener.accept().await.unwrap();  // En arrière plan avec .await()

        // Ajout du nouveau client dans le repertoire
        let peer = Peer {
	        address: addr_peer
	    };

		let connected_peers_clone = Arc::clone(&connected_peers);
		connected_peers_clone.lock().await.insert(addr_peer, socket_peer);

	    println!("New client connected as {}", addr_peer);

	    // Écoute des messages recus puis relayage 
		tokio::spawn(async move {
		    let mut buffer = [0; 512];
		    let peers_ref = Arc::clone(&connected_peers_clone);
		    
		    loop {
		        // ✅ Récupérer le socket du peer pour lire
		        let mut peers_map = peers_ref.lock().await;
		        let socket = match peers_map.get_mut(&peer.address) {
		            Some(s) => s,
		            None => break,
		        };
		        
		        match socket.read(&mut buffer).await {
		            Ok(0) => {
		                // Peer déconnecté
		                drop(peers_map);
		                peers_ref.lock().await.remove(&peer.address);
		                println!("❌ Peer déconnecté: {}", peer.address);
		                break;
		            }
		            Ok(n) => {
		                // ✅ Message reçu
		                let message = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
		                println!("📨 Message de {}: {}", peer.address, message);
		                
		                // ✅ RELAYER aux autres peers
		                drop(peers_map);
		                relay_message(&peers_ref, peer.address, &message).await;
		            }
		            Err(e) => {
		                println!("⚠️  Erreur: {}", e);
		                drop(peers_map);
		                peers_ref.lock().await.remove(&peer.address);
		                break;
		            }
		        }
		    }
		});
    }
}

async fn relay_message(
    peers: &PeersMap,
    sender_addr: SocketAddr,
    message: &str,
) {
    let mut peers_map = peers.lock().await;
    
    for (other_addr, other_socket) in peers_map.iter_mut() {
        if other_addr != &sender_addr {  // Ne pas envoyer au peer qui a initier le message
            let formatted = format!("[{}] {}\n", sender_addr, message);
            match other_socket.write_all(formatted.as_bytes()).await {
                Ok(_) => println!("  → Relayé vers {}", other_addr),
                Err(e) => println!("  ⚠️  Erreur: {}", e),
            }
        }
    }
}