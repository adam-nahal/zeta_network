use std::io::{self, Write};
use std::net::TcpStream;

fn main() {
    // Connexion au relai
    let ip_relay = "65.75.200.180";
    let port_relay = 12345;
    let socket_relay = format!("{}:{}", ip_relay, port_relay);

    let mut stream = TcpStream::connect(&socket_relay)
        .expect("[ERROR] Impossible de se connecter au relay");
    
    println!("\nConnecté au relai ({}). Tapez vos messages (Ctrl+C pour quitter):", socket_relay);
    
    // Boucle d'envoi de mesage au relai
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        
        let mut input = String::new();
        stdin.read_line(&mut input).unwrap();
        
        // Envoyer
        stream.write_all(input.as_bytes()).unwrap();
    }
}