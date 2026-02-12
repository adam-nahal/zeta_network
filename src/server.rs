use std::io::Read;
use std::net::TcpListener;

fn main() {
    let listener = TcpListener::bind("0.0.0.0:12345").unwrap();
    println!("Écoute sur le port 12345...");
    
    let (mut stream, _) = listener.accept().unwrap();
    let mut buffer = [0; 512];
    
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => println!("{}", String::from_utf8_lossy(&buffer[..n])),
            Err(_) => break,
        }
    }
}