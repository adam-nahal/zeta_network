use std::fmt;
use tokio::net::UdpSocket;
use std::net::{SocketAddr, ToSocketAddrs};
use clap::{Parser, ValueEnum};
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use anyhow::Result;
mod relay;
mod client;
mod nat_detector;

#[derive(Debug, Parser, Clone)]
struct Opts {
    // Si ce noeud est celui qui initie la connection, celui qui la recoit, voir le relai
    #[arg(long, value_enum)]
    mode: Mode,

    #[arg(long)]
    peer_id: String,

    // Adresse du relai qui va permettre de connecter les noeuds (au moins au début)
    #[arg(long, required_if_eq_any([("mode", "dial"), ("mode", "listen")]), help("Peers in dial/listen mode require '--relay-addr 1.2.3.4:5678'"))]
    relay_addr: Option<String>,

    // L'adresse du noeud auquel le dial veut se connecter
    #[arg(long, required_if_eq("mode", "dial"), help("Peers in dial mode require '--listen-peer-d pseudo'"))]
    listen_peer_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, ValueEnum)]
enum Mode {
    Dial,
    Listen,
    Relay,
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
}


#[tokio::main]
async fn main() {
	// Récupération du type de noeud (dial/listen/relay)
    let opts = Opts::parse();

    match opts.mode {
        Mode::Relay => relay::main_relay().await,
        Mode::Listen | Mode::Dial => client::main_client(opts.clone()).await,
    }
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

// impl fmt::Display for Message {  // Pour pouvoir faire print("{}", msg) avec un affichage formatté
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         if let Some(dt) = DateTime::<Utc>::from_timestamp(self.time as i64, 0) {
//             write!(f, "[{} → {}] \"{}\" ({})", self.src, self.dst, self.txt, dt.format("%H:%M:%S"))
//         } else { // On affiche le timestamp brut s'il y a un problème de conversion
//             write!(f, "[{} → {}] \"{}\" (t={})", self.src, self.dst, self.txt, self.time)
//         }
//     }
// }
impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Message::Register { src_addr, src_id, dst_addr, dst_id, time } => {
                let time_str = DateTime::<Utc>::from_timestamp(*time as i64, 0)
                    .map(|dt| dt.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| format!("t={}", time));
                write!(f, "[Register] {} ({}) → {} ({}) ({})", src_addr, src_id, dst_addr, dst_id, time_str)
            }
            Message::Connect { src_addr, src_id, dst_id, dst_addr, time } => {
                let time_str = DateTime::<Utc>::from_timestamp(*time as i64, 0)
                    .map(|dt| dt.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| format!("t={}", time));
                write!(f, "[Connect] {} ({}) → ? {} ({}) ({})", src_addr, src_id, dst_addr, dst_id, time_str)
            }
            Message::AskForAddr { src_addr, src_id, peer_id, time } => {
                let time_str = DateTime::<Utc>::from_timestamp(*time as i64, 0)
                    .map(|dt| dt.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| format!("t={}", time));
                write!(f, "[AskInfo] {} ({}) asks for {}'s addr ({})", *src_addr, src_id, peer_id, time_str)
            }
            Message::PeerInfo { peer_addr, peer_id } => {
                write!(f, "[PeerInfo] {} ({})", peer_addr, peer_id)
            }
            Message::Classic { src_addr, src_id, dst_addr, dst_id, txt, time } => {
                let time_str = DateTime::<Utc>::from_timestamp(*time as i64, 0)
                    .map(|dt| dt.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| format!("t={}", time));
                write!(f, "[{} ({}) → {} ({})] \"{}\" ({})", src_addr, src_id, dst_addr, dst_id, txt, time_str)
            }
        }
    }
}

async fn get_public_ip(socket: &UdpSocket) -> Result<SocketAddr> {
    let stun_addr = "stun.l.google.com:19302"
        .to_socket_addrs()?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| anyhow::anyhow!("Cannot resolve STUN server"))?;

    let client = stunclient::StunClient::new(stun_addr);
    let public_addr = client.query_external_address_async(socket).await?;
    Ok(public_addr)
}