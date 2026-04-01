use std::fmt;
use tokio::net::UdpSocket;
use std::net::{SocketAddr, ToSocketAddrs};
use clap::{Parser, ValueEnum};
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use anyhow::Result;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use std::sync::Arc;
use std::collections::HashMap;

pub type PeersMap = Arc<Mutex<HashMap<SocketAddr, (String, u64)>>>; // un noeud = [Addr, (pseudo, derniere connection en secs)]
pub const MAX_PACKET_SIZE: usize = 4096;

#[derive(Debug, Parser, Clone)]
pub struct Opts {
    // Si ce noeud est celui qui initie la connection, celui qui la recoit, voir le relai
    #[arg(long, value_enum)]
    pub mode: Mode,

    #[arg(long)]
    pub peer_id: String
}

#[derive(Clone, Debug, PartialEq, ValueEnum)]
pub enum Mode {
    Client,
    HubRelay
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Register {  // Client → Relay : "Je m'enregistre, voici mon adresse et mon id"
        src_addr: SocketAddr,
        src_id: String,
        dst_addr: SocketAddr,
        dst_id: String,
        time: u64,
    },

    Connect {  // Dial → Relay : "Mets-moi en contact avec ce peer_id"
        src_addr: SocketAddr,
        src_id: String,
        dst_addr: SocketAddr,   // l'id du Listen recherché
        dst_id: String,
        time: u64,
    },

    AskForAddr {  // Relay → Client : "Voici l'adresse+id du peer que tu cherches"
        src_addr: SocketAddr,
        src_id: String,
        peer_id: String,
        time: u64,
    },

    PeerInfo {  // Relay → Client : "Voici l'adresse+id du peer que tu cherches"
        peer_addr: SocketAddr,
        peer_id: String,
    },

    Classic {  // Peer → Peer : message direct (hole punching, hello, etc.)
        src_addr: SocketAddr,
        src_id: String,
        dst_addr: SocketAddr,
        dst_id: String,
        txt: String,
        time: u64,
    },

    BeNewRelay {  // new Relay → Serveur stockant les adresses des relais : "Je me déclare relay"
        src_addr: SocketAddr,
        src_id: String,
        time: u64,
    },

    NeedRelay {
        src_addr: SocketAddr,
        src_id: String,
        time: u64,
    },

    RelayHasNewClient {
        src_addr: SocketAddr,
        src_id: String,
        peer_addr: SocketAddr,
        peer_id: String,
        time: u64,
    },

    NoRelayAvailable {
        src_addr: SocketAddr,
        src_id: String,
        dst_addr: SocketAddr,
        dst_id: String,
        time: u64,
    },

    PunchTheHole {
        src_addr: SocketAddr,
        src_id: String,
        dst_addr: SocketAddr,
        dst_id: String,
        time: u64,
    },

    Ack {
        src_addr: SocketAddr,
        src_id: String,
        time: u64,
    },
}

#[async_trait::async_trait]
pub trait UdpSocketExt {
    async fn send_msg(&self, msg: &Message, next_hop: SocketAddr) -> Result<usize>;
}

#[async_trait::async_trait]
impl UdpSocketExt for UdpSocket {
    async fn send_msg(&self, msg: &Message, next_hop: SocketAddr) -> Result<usize> {
        let encoded = bincode::serialize(&msg)?;
        Ok(self.send_to(&encoded, next_hop).await?)
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Message::Register { src_addr, src_id, dst_addr, dst_id, time } => {
                write!(f, "[Register] {} ({}) → {} ({}) ({})", src_addr, src_id, dst_addr, dst_id, fmt_time(*time))
            }
            Message::Connect { src_addr, src_id, dst_id, dst_addr, time } => {
                write!(f, "[Connect] {} ({}) → {} ({}) ({})", src_addr, src_id, dst_addr, dst_id, fmt_time(*time))
            }
            Message::AskForAddr { src_addr, src_id, peer_id, time } => {
                write!(f, "[AskForAddr] {} ({}) asks for {}'s addr ({})", *src_addr, src_id, peer_id, fmt_time(*time))
            }
            Message::PeerInfo { peer_addr, peer_id } => {
                write!(f, "[PeerInfo] {} ({})", peer_addr, peer_id)
            }
            Message::Classic { src_addr, src_id, dst_addr, dst_id, txt, time } => {
                write!(f, "[{} ({}) → {} ({})] \"{}\" ({})", src_addr, src_id, dst_addr, dst_id, txt, fmt_time(*time))
            }
            Message::BeNewRelay { src_addr, src_id, time } => {
                write!(f, "[BeNewRelay] {} ({}) ({})", src_addr, src_id, fmt_time(*time))
            }
            Message::NeedRelay { src_addr, src_id, time } => {
                write!(f, "[NeedRelay] {} ({}) ({})", src_addr, src_id, fmt_time(*time))
            }
            Message::Ack { src_addr, src_id, time } => {
                write!(f, "[Ack] {} ({}) ({})", src_addr, src_id, fmt_time(*time))
            }
            Message::RelayHasNewClient { peer_addr, peer_id, time, .. } => {
                write!(f, "[RelayHasNewClient] {} ({}) wants to connect to you, relay ({})", peer_addr, peer_id, fmt_time(*time))
            }
            Message::NoRelayAvailable { src_addr, src_id, dst_addr, dst_id, time } => {
                write!(f, "[NoRelayAvailable] [{} ({}) → {} ({})] ({})", src_addr, src_id, dst_addr, dst_id, fmt_time(*time))
            }
            Message::PunchTheHole { src_addr, src_id, dst_addr, dst_id, time } => {
                write!(f, "[PunchTheHole] [{} ({}) → {} ({})] ({})", src_addr, src_id, dst_addr, dst_id, fmt_time(*time))
            }
        }
    }
}

pub async fn get_public_ip(socket: &UdpSocket) -> Result<SocketAddr> {
    let stun_addr = "stun.l.google.com:19302"
        .to_socket_addrs()?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| anyhow::anyhow!("Cannot resolve STUN server"))?;

    let client = stunclient::StunClient::new(stun_addr);
    let public_addr = client.query_external_address_async(socket).await?;
    Ok(public_addr)
}

pub async fn recv_msg(socket: &UdpSocket) -> Option<(Message, SocketAddr)> {
    let mut buf = [0; MAX_PACKET_SIZE];
    let (size, sender_addr) = socket.recv_from(&mut buf).await.expect("Nothing received");
    if size == 0 || size >= MAX_PACKET_SIZE {
        println!("The message's size is incorrect({})", size);
        return None;
    }    
    match bincode::deserialize(&buf[..size]) {
        Ok(msg) => Some((msg, sender_addr)),
        Err(e) => {
            eprintln!("[WARN] Deserialization failed from {}: {}", sender_addr, e);
            None
        }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

pub async fn delete_disconnected_peers(peers: &PeersMap) {
    let mut peers_map = peers.lock().await;
    peers_map.retain(|addr, (_, last_seen)| {
        let active = now_secs() - *last_seen < 120;
        if !active { println!("[INFO] Peer {} disconnected (timeout)", addr); }
        active
    });
}

pub async fn relay_message(peers: &PeersMap, sender_addr: SocketAddr, msg: Message, socket: &UdpSocket) {
    let mut peers_map = peers.lock().await;

    for (other_addr, _) in peers_map.iter_mut() {
        if other_addr != &sender_addr {
            if let Err(e) = socket.send_msg(&msg, *other_addr).await {
                eprintln!("Failed to send to {}: {}", other_addr, e);
            } else {
                println!("    Relayed to {}", other_addr);
            }
        }
    }
}

pub async fn wait_for_ack(socket: &UdpSocket) -> bool {
    loop {
        let Some((msg, _)) = recv_msg(socket).await else { return false };
        match &msg {
            Message::Ack { src_addr, src_id, .. } => {
                println!("Ack from {} ({})", src_addr, src_id);
                return true;
            }
            _ => println!("Unexpected message: '{}'", msg),
        }
    }
}

pub async fn connect_to_a_relay(socket: &UdpSocket, public_addr: SocketAddr, peer_id: &str, hub_relay_addr: SocketAddr) -> Option<(SocketAddr, String)> {
    // Demande un relais jusqu'à en obtenir un
    let (relay_addr, relay_id) = loop {
        println!("Asking the hub relay an available relay...");
        let msg = Message::NeedRelay {
            src_addr: public_addr,
            src_id: peer_id.to_string(),
            time: now_secs(),
        };
        let _ = socket.send_msg(&msg, hub_relay_addr).await;
        println!("->({})", msg);

        let Some((msg, _)) = recv_msg(socket).await else { continue };
        println!("<-({})", msg);
        match &msg {
            Message::PeerInfo { peer_addr, peer_id, .. } => {
                println!("Received relay address {} ({})", peer_addr, peer_id);
                break (*peer_addr, peer_id.clone());
            }
            Message::NoRelayAvailable { .. } => {
                println!("[WARN] No relays available, retrying in 5s...");
                sleep(Duration::from_secs(15)).await;
            }
            _ => println!("Unexpected message: '{}'", msg),
        }
    };

    // Enregistrement auprès du relais
    let msg = Message::Register {
        src_addr: public_addr,
        src_id: peer_id.to_string(),
        dst_addr: relay_addr,
        dst_id: relay_id.clone(),
        time: now_secs(),
    };
    let _ = socket.send_msg(&msg, relay_addr).await;
    println!("->({})", msg);

    if !wait_for_ack(socket).await {
        println!("[ERROR] No ack from relay, aborting");
        return None;
    }

    Some((relay_addr, relay_id))
}
 

fn fmt_time(time: u64) -> String {
    DateTime::<Utc>::from_timestamp(time as i64, 0)
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| format!("t={}", time))
}
