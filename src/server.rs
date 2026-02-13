use tokio::net::TcpListener;
use tokio::io::AsyncReadExt;

#[derive(Clone, Debug)]
struct Client {
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
    
    // Crée la liste de tous les clients connectés à ce relai
    let mut connected_clients: Vec<Client> = Vec::new();

    loop {
        let (mut socket_client, addr_client) = listener.accept().await.unwrap();  // En arrière plan avec .await()

        // Ajout du nouveau client dans le repertoire
        let client = Client {
	        ip: addr_client.ip().to_string(),
	        port: addr_client.port(),
	        is_ipv4: addr_client.is_ipv4(),
	        is_ipv6: addr_client.is_ipv6()
	    };
	    connected_clients.push(client);
	    println!("New client connect as {:?}", connected_clients);

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