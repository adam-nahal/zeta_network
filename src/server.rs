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
        let (mut socket_client, _) = listener.accept().await.unwrap();  // En arrière plan avec .await()
        
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