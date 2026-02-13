use tokio::net::TcpListener;
use tokio::io::AsyncReadExt;

struct client {
	ip: String,
	port: u16,
	is_ipv4: bool,
	is_ipv6: bool
}

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
        println!("IPv4: {}", addr_client.is_ipv4());
        println!("IPv6: {}", addr_client.is_ipv6());

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