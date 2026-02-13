use std::io::{self, Write};
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() {
	// Connexion au relai
    let ip_relay = "65.75.200.180";
    let port_relay = 12345;
    let socket_relay = format!("{}:{}", ip_relay, port_relay);
    let stream = TcpStream::connect(&socket_relay).await
        .expect("[ERROR] Can't connect to relay");
    
    println!("\nConnected to relay {}. Enter messages (Ctrl+C to quit):", socket_relay);
    
    // Séparation du flux de données pour l'envoie et la réception
    let (mut reader, mut writer) = stream.into_split();
    
    // Construction du thread d'écoute (tourne en parallele)
    tokio::spawn(async move {
        let mut buf = [0; 1024];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 { break; }
            println!("\n[RECEIVED] {}", String::from_utf8_lossy(&buf[..n]));
        }
    });
    

    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        
        let mut input = String::new();
        stdin.read_line(&mut input).unwrap();
        
        writer.write_all(input.as_bytes()).await.unwrap();
    }
}