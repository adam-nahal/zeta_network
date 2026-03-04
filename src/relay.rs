use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;
use std::net::{SocketAddr, IpAddr};
use std::time::{SystemTime, UNIX_EPOCH};

type PeersMap = Arc<Mutex<HashMap<SocketAddr, u64>>>; // un noeud = [Addr, date dernière connection en sec]

pub async fn main_relay() {
    // Le relay démarre l'écoute
    let port_relay = 12345;
    let addr_relay = format!("0.0.0.0:{}", port_relay);
    let socket_relay = UdpSocket::bind(&addr_relay).await.expect("Failed to bind");
    let ip_relay = get_public_ip().await.expect("Public IP of the relay not obtained.");
    let ip_relay: IpAddr = ip_relay.parse().expect("Invalid relay IP");
    println!("Listening on {}:{}...", ip_relay, port_relay);

    // Crée la liste de tous les clients qui ont contacté ce relai
    let peers_list: PeersMap = Arc::new(Mutex::new(HashMap::new()));

    let mut buf = vec![0; 1024];
    loop {
        match socket_relay.recv_from(&mut buf).await {
            Ok((size, peer_addr)) => {
                // Affichage du message
                let message = String::from_utf8_lossy(&buf[..size]).trim().to_string();
                println!("[{}] {:?}", peer_addr, message);

                // Ajout du client dans le repertoire
                let connected_peers_clone = Arc::clone(&peers_list);
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();
                connected_peers_clone.lock().await.insert(peer_addr, now);

                relay_message(&connected_peers_clone, peer_addr, &message, &socket_relay).await;
            }
            Err(e) => eprintln!("[ERROR]: a message contain an error ({})", e),
        }
    }
}

async fn relay_message(peers: &PeersMap, sender_addr: SocketAddr, message: &str, socket: &UdpSocket) {
    let mut peers_map = peers.lock().await;

    for (other_addr, _) in peers_map.iter_mut() {
        if other_addr != &sender_addr {
            let formatted = format!("[{}] {}\n", sender_addr, message);
            if let Err(e) = socket.send_to(formatted.as_bytes(), *other_addr).await {
                eprintln!("Failed to send to {}: {}", other_addr, e);
            } else {
                println!("	Relayed to {}", other_addr);
            }
        }
    }
}

async fn get_public_ip() -> Result<String, reqwest::Error> {
    let resp = reqwest::get("https://api.ipify.org").await?;
    let ip = resp.text().await?;
    Ok(ip)
}