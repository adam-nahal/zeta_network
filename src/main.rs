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

    // Adresse du relai qui va permettre de connecter les noeuds (au moins au début)
    #[arg(long, required_if_eq_any([("mode", "dial"), ("mode", "listen")]), help("Peers in dial/listen mode require '--relay-addr 1.2.3.4:5678'"))]
    relay_addr: Option<String>,

    // L'adresse du noeud auquel le dial veut se connecter
    #[arg(long, required_if_eq("mode", "dial"), help("Peers in dial mode require '--remote-peer-addr 1.2.3.4:5678'"))]
    remote_peer_addr: Option<String>,
}

#[derive(Clone, Debug, PartialEq, ValueEnum)]
enum Mode {
    Dial,
    Listen,
    Relay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
	pub src: SocketAddr,
	pub dst: SocketAddr,
	pub txt: String,
	pub time: u64,
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
    async fn send_txt(&self, src: SocketAddr, dst: SocketAddr, txt: &str, next_hop: SocketAddr) -> Result<usize>;
}

#[async_trait::async_trait]
impl UdpSocketExt for UdpSocket {
    async fn send_msg(&self, msg: &Message, next_hop: SocketAddr) -> Result<usize> {
        let encoded = bincode::serialize(&msg)?;
        Ok(self.send_to(&encoded, next_hop).await?)
    }

    async fn send_txt(&self, src: SocketAddr, dst: SocketAddr, txt: &str, next_hop: SocketAddr) -> Result<usize> {
        let msg = Message {
            src,
            dst,
            txt: txt.to_string(),
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
    	self.send_msg(&msg, next_hop).await
    }
}

impl fmt::Display for Message {  // Pour pouvoir faire print("{}", msg) avec un affichage formatté
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dt) = DateTime::<Utc>::from_timestamp(self.time as i64, 0) {
            write!(f, "[{}→{}] \"{}\" ({})", self.src, self.dst, self.txt, dt.format("%H:%M:%S"))
        } else { // On affiche le timestamp brut s'il y a un problème de conversion
            write!(f, "[{}→{}] \"{}\" (t={})", self.src, self.dst, self.txt, self.time)
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