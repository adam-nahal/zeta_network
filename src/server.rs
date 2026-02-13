use tokio::net::TcpListener;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() {
    // Le relay démarre l'écoute 
    let listen_port_relay = 12345;
    let listen_socket_relay = format!("0.0.0.0:{}", listen_port_relay);
    
    let listener = TcpListener::bind(&listen_socket_relay).await.unwrap();
    println!("Listening on port {}...", listen_port_relay);
    
    loop {
        let (mut socket_client, addr_client) = listener.accept().await.unwrap();  // En arrière plan avec .await()
                // TOUTES les infos du client
        println!("\n========== CLIENT CONNECTÉ ==========");
        println!("IP: {}", addr_client.ip());
        println!("Port: {}", addr_client.port());
        println!("Adresse: {}", addr_client);
        println!("IPv4: {}", addr_client.is_ipv4());
        println!("IPv6: {}", addr_client.is_ipv6());
        
        println!("\n===== SOCKET =====");
        println!("Local addr: {:?}", socket_client.local_addr());
        println!("Peer addr: {:?}", socket_client.peer_addr());
        println!("TTL: {:?}", socket_client.ttl());
        println!("Nodelay: {:?}", socket_client.nodelay());
        println!("==================================\n");

        tokio::spawn(async move {
            let mut buffer = [0; 512];
            
            loop {
                match socket_client.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(n) => println!("{}", String::from_utf8_lossy(&buffer[..n])),
                    Err(_) => break,
                }
            }
        });
    }
}