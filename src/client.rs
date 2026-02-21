use tokio::net::{TcpStream, TcpListener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, Duration, timeout};
use std::net::SocketAddr;
use tokio::process::Command;
use crate::{Opts, Mode};

pub async fn main_client(opts: Opts) {
    // Connexion au relai
    let ip_relay = opts.relay_ip.expect("--relay-ip est requis");
    let port_relay = opts.relay_port.expect("--relay-port est requis");
    let socket_relay = format!("{}:{}", ip_relay, port_relay);
    
    let mut relay_stream = TcpStream::connect(&socket_relay).await
        .expect("[ERROR] Can't connect to relay");
    
    println!("\nConnected to relay {}", socket_relay);

    // Mise à l'écart des noeuds derrière un NAT symétrique
    if !peer_hole_punchable().await {
        println!("[WARNING] This pair can't become a relay (Symmetric NAT, Hole Punching impossible)");
        return;
    }

    // Récupère l'adresse locale de cette connexion (pour connaître notre port)
    let local_addr = relay_stream.local_addr().unwrap();
    println!("Our local address: {}", local_addr);
    
    match opts.mode {
        Mode::Listen => {
            listen_mode(&mut relay_stream, local_addr).await;
        }
        Mode::Dial => {
            let remote_peer_ip = opts.remote_peer_ip.expect("--remote-peer-ip requis");
            dial_mode(&mut relay_stream, local_addr, &remote_peer_ip).await;
        }
        _ => unreachable!()
    }
}

// ==================== MODE LISTEN ====================
async fn listen_mode(relay_stream: &mut TcpStream, local_addr: SocketAddr) {
    println!("\nLISTEN MODE: Waiting for dial peer...");
    
    // Étape 1 : Annoncer au relai qu'on est prêt à recevoir
    let msg = format!("LISTEN_READY\n");
    relay_stream.write_all(msg.as_bytes()).await.unwrap();
    println!("Sent 'LISTEN_READY' to relay");
    
    // Étape 2 : Recevoir l'adresse du peer Dial via le relai
    let mut buf = [0u8; 512];
    let n = relay_stream.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    
    let dial_peer_addr: SocketAddr = response.trim()
        .strip_prefix("DIAL_PEER:")
        .expect("Invalid response format")
        .parse()
        .expect("Invalid peer address");
    
    println!("Received dial peer address: {}", dial_peer_addr);
    
    // Étape 3 : TEST 1 - Connexion directe AVANT hole punching
    println!("\n🧪 TEST 1: Direct connection BEFORE hole punching...");
    if test_direct_connection(&dial_peer_addr).await {
        println!("✅ Direct connection works WITHOUT hole punching!");
        start_p2p_communication(dial_peer_addr).await;
        return;
    }
    println!("❌ Direct connection failed (expected with NAT)");
    
    // Étape 4 : Hole Punching - Écoute + envoi simultané
    println!("\n🔨 Starting HOLE PUNCHING...");
    
    // Bind sur le même port local qu'on utilise avec le relai
    let listener = TcpListener::bind(local_addr).await
        .expect("Failed to bind listener");
    println!("👂 Listening on {}", local_addr);
    
    // Envoi de paquets de "punch" pour ouvrir le NAT
    tokio::spawn(async move {
        for i in 0..5 {
            if let Ok(mut stream) = TcpStream::connect(dial_peer_addr).await {
                let _ = stream.write_all(b"PUNCH\n").await;
                println!("  👊 Punch {} sent to {}", i+1, dial_peer_addr);
            }
            sleep(Duration::from_millis(200)).await;
        }
    });
    
    // Attente de connexion du peer Dial
    sleep(Duration::from_secs(2)).await;
    
    // Étape 5 : TEST 2 - Connexion directe APRÈS hole punching
    println!("\n🧪 TEST 2: Direct connection AFTER hole punching...");
    if test_direct_connection(&dial_peer_addr).await {
        println!("✅ Hole punching SUCCESS! Direct connection established!");
        start_p2p_communication(dial_peer_addr).await;
    } else {
        println!("❌ Hole punching failed. NAT type might be too restrictive.");
    }
}

// ==================== MODE DIAL ====================
async fn dial_mode(relay_stream: &mut TcpStream, local_addr: SocketAddr, remote_peer_ip: &str) {
    println!("\nDIAL MODE: Initiating connection to {}...", remote_peer_ip);
    
    // Étape 1 : Demander au relai de nous connecter au peer Listen
    let msg = format!("DIAL_REQUEST:{}\n", remote_peer_ip);
    relay_stream.write_all(msg.as_bytes()).await.unwrap();
    println!("Sent 'DIAL_REQUEST:{}' to relay", remote_peer_ip);
    
    // Étape 2 : Recevoir l'adresse du peer Listen via le relai
    let mut buf = [0u8; 512];
    let n = relay_stream.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    
    let listen_peer_addr: SocketAddr = response.trim()
        .strip_prefix("LISTEN_PEER:")
        .expect("Invalid response format")
        .parse()
        .expect("Invalid peer address");
    
    println!("📥 Received listen peer address: {}", listen_peer_addr);
    
    // Étape 3 : TEST 1 - Connexion directe AVANT hole punching
    println!("\n🧪 TEST 1: Direct connection BEFORE hole punching...");
    if test_direct_connection(&listen_peer_addr).await {
        println!("✅ Direct connection works WITHOUT hole punching!");
        start_p2p_communication(listen_peer_addr).await;
        return;
    }
    println!("❌ Direct connection failed (expected with NAT)");
    
    // Étape 4 : Hole Punching - Envoi simultané
    println!("\n🔨 Starting HOLE PUNCHING...");
    
    sleep(Duration::from_millis(500)).await;  // Petit délai pour sync
    
    // Envoi de paquets de "punch"
    for i in 0..5 {
        if let Ok(mut stream) = TcpStream::connect(listen_peer_addr).await {
            let _ = stream.write_all(b"PUNCH\n").await;
            println!("  👊 Punch {} sent to {}", i+1, listen_peer_addr);
        }
        sleep(Duration::from_millis(200)).await;
    }
    
    sleep(Duration::from_secs(1)).await;
    
    // Étape 5 : TEST 2 - Connexion directe APRÈS hole punching
    println!("\n🧪 TEST 2: Direct connection AFTER hole punching...");
    if test_direct_connection(&listen_peer_addr).await {
        println!("✅ Hole punching SUCCESS! Direct connection established!");
        start_p2p_communication(listen_peer_addr).await;
    } else {
        println!("❌ Hole punching failed. NAT type might be too restrictive.");
    }
}

// ==================== TESTS & UTILS ====================
async fn test_direct_connection(peer_addr: &SocketAddr) -> bool {
    match timeout(Duration::from_secs(3), TcpStream::connect(peer_addr)).await {
        Ok(Ok(mut stream)) => {
            // Test d'envoi/réception
            if stream.write_all(b"PING\n").await.is_ok() {
                let mut buf = [0u8; 16];
                if let Ok(n) = timeout(Duration::from_secs(1), stream.read(&mut buf)).await {
                    if n.is_ok() {
                        return true;
                    }
                }
            }
            false
        }
        _ => false
    }
}

async fn start_p2p_communication(peer_addr: SocketAddr) {
    println!("\n🎉 P2P CONNECTION ESTABLISHED with {}", peer_addr);
    println!("Type messages to send (Ctrl+C to quit):\n");
    
    let mut stream = TcpStream::connect(peer_addr).await
        .expect("Failed to reconnect for P2P");
    
    let (mut reader, mut writer) = stream.into_split();
    
    // Thread d'écoute
    tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 { break; }
            print!("\n[PEER] {}\n> ", String::from_utf8_lossy(&buf[..n]).trim());
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
        }
    });
    
    // Envoi de messages
    let stdin = std::io::stdin();
    loop {
        print!("> ");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
        
        let mut input = String::new();
        stdin.read_line(&mut input).unwrap();
        
        let msg = format!("{}\n", input.trim());
        writer.write_all(msg.as_bytes()).await.unwrap();
    }
}

async fn peer_hole_punchable() -> bool {
	// Fait une requête à un serveur STUN  (en CLI : >>> stunclient stun.l.google.com 3478)
    let output = Command::new("stunclient")  
        .arg("stun.l.google.com")
        .arg("3478")
        .output()
        .await
        .expect("Failed to run stunclient");
    let result = String::from_utf8_lossy(&output.stdout);

    // Interpète les résultats de la requête
    if let Some((local_port, mapped_port)) = extract_ports(&result) {
    	return local_port == mapped_port;
    } else {
    	println!("[ERROR] We can't verify if this peer is behind a Symmetric NAT or not.");
    	return true;  // Si l'on est pas sûr que le pair est derrière du NAT symétrique, on essaye quand meme le hole punching
    }
}


fn extract_ports(stun_output: &str) -> Option<(u16, u16)> {
    let mut local_port = None;
    let mut mapped_port = None;
    
    // Cherche les ports dans les résultats de la requête
    for line in stun_output.lines() {
        if line.contains("Local address:") {
            local_port = line.split(':').last().and_then(|s| s.trim().parse().ok());
        } else if line.contains("Mapped address:") {
            mapped_port = line.split(':').last().and_then(|s| s.trim().parse().ok());
        }
    }
    
    Some((local_port?, mapped_port?))
}