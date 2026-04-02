use std::fmt;
use tokio::net::UdpSocket;
use std::net::{SocketAddr, ToSocketAddrs};
use clap::{Parser, Subcommand};
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use anyhow::Result;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::{sleep, timeout, Duration};
use std::sync::Arc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};


pub type PeersMap = Arc<Mutex<HashMap<SocketAddr, (String, u64)>>>; // un noeud = [Addr, (pseudo, derniere connection en secs)]
pub const MAX_PACKET_SIZE: usize = 4096;

#[derive(Debug, Parser)]
pub struct Opts {
    #[command(subcommand)]
    pub mode: Mode,
}

#[derive(Debug, Subcommand)]
pub enum Mode {
    Client {
        #[arg(long)]
        peer_id: String,
    },
    HubRelay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Register {  // Client → Relay : "Je m'enregistre, voici mon adresse et mon id"
    	header: MessageHeader,
    },

    Connect {  // Dial → Relay : "Mets-moi en contact avec ce peer_id"
    	header: MessageHeader,
    },

    AskForAddr {  // Relay → Client : "Voici l'adresse+id du peer que tu cherches"
    	header: MessageHeader,
        peer_id: String,
    },

    PeerInfo {  // Relay → Client : "Voici l'adresse+id du peer que tu cherches"
    	header: MessageHeader,
        peer_addr: SocketAddr,
        peer_id: String,
    },

    Classic {  // Peer → Peer : message direct (hole punching, hello, etc.)
    	header: MessageHeader,
        txt: String,
    },

    BeNewRelay {  // new Relay → Serveur stockant les adresses des relais : "Je me déclare relay"
    	header: MessageHeader,
    },

    NeedRelay {
		header: MessageHeader,
    },

    RelayHasNewClient {
    	header: MessageHeader,
        peer_addr: SocketAddr,
        peer_id: String,
    },

    NoRelayAvailable {
    	header: MessageHeader,
    },

    PunchTheHole {
    	header: MessageHeader,
    },

    Ack {
    	header: MessageHeader,
    	reply_to: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageHeader {
	pub msg_id: u64,
    pub src_addr: SocketAddr,
    pub src_id: String,
    pub dst_addr: SocketAddr,
    pub dst_id: String,
    pub time: u64,
}

#[async_trait::async_trait]
pub trait UdpSocketExt {
    async fn send_msg(&self, msg: &Message, next_hop: SocketAddr) -> Result<usize>;
}

#[async_trait::async_trait]
impl UdpSocketExt for UdpSocket {
    async fn send_msg(&self, msg: &Message, next_hop: SocketAddr) -> Result<usize> {
        let encoded = bincode::serialize(&msg)?;
        let size = self.send_to(&encoded, next_hop).await?;
		println!("->({})", msg);
		Ok(size)
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Message::Register { header } => {
                write!(f, "[Register] [{}]", *header)
            }
            Message::Connect { header } => {
                write!(f, "[Connect] [{}]", *header)
            }
            Message::AskForAddr { header, peer_id } => {
                write!(f, "[AskForAddr] [{}] He asks for {}'s addr", header, peer_id)
            }
            Message::PeerInfo { header, peer_addr, peer_id } => {
                write!(f, "[PeerInfo] [{}] {} ({})", header, peer_addr, peer_id)
            }
            Message::Classic { header, txt } => {
                write!(f, "[Classic] [{}] \"{}\"", header, txt)
            }
            Message::BeNewRelay { header } => {
                write!(f, "[BeNewRelay] [{}]", header)
            }
            Message::NeedRelay { header } => {
                write!(f, "[NeedRelay] [{}]", header)
            }
            Message::Ack { header, reply_to } => {
                write!(f, "[Ack] [{}] reply_to=#{}", header, reply_to)
            }
            Message::RelayHasNewClient { header, peer_addr, peer_id } => {
                write!(f, "[RelayHasNewClient] [{}] {} ({}) wants to connect to you as relay", header, peer_addr, peer_id)
            }
            Message::NoRelayAvailable { header } => {
                write!(f, "[NoRelayAvailable] [{}]", header)
            }
            Message::PunchTheHole { header } => {
                write!(f, "[PunchTheHole] [{}]", header)
            }
        }
    }
}

impl fmt::Display for MessageHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) -> {} ({}) (#{} - {})",
            self.src_addr, self.src_id,
            self.dst_addr, self.dst_id,
            self.msg_id,
            fmt_time(self.time)
        )
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
        Ok(msg) => {
	    	println!("<-{}", msg);
	    	Some((msg, sender_addr))
	    }
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

// Enregistre l'attente, envoie le message, attend le signal
pub async fn send_and_wait_ack(socket: &UdpSocket, msg: &Message, dest: SocketAddr, ack_waiter: &AckWaiter, msg_id: u64) -> bool {
    let rx = ack_waiter.register(msg_id).await;   // register AVANT send
    let _ = socket.send_msg(msg, dest).await;
    match timeout(Duration::from_secs(30), rx).await {
        Ok(Ok(())) => true,
        _ => { eprintln!("[ERROR] Timeout waiting for ack"); false }
    }
}

pub async fn connect_to_a_relay(socket: &UdpSocket, public_addr: SocketAddr, peer_id: &str, hub_relay_addr: SocketAddr, hub_rx: &mut MsgReceiver, ack_waiter: &AckWaiter) -> Option<(SocketAddr, String)> {
    let (relay_addr, relay_id) = loop {
        println!("Asking the hub relay an available relay...");
        let msg = Message::NeedRelay {
            header: MessageHeader {
                msg_id:   new_msg_id(),
                src_addr: public_addr,
                src_id:   peer_id.to_string(),
                dst_addr: hub_relay_addr,
                dst_id:   "hubrelay".to_string(),
                time:     now_secs(),
            }
        };
        let _ = socket.send_msg(&msg, hub_relay_addr).await;

        match hub_rx.recv().await {
            Some((Message::PeerInfo { peer_addr, peer_id: rid, .. }, _)) => {
                println!("Received relay address {} ({})", peer_addr, rid);
                break (peer_addr, rid);
            }
            Some((Message::NoRelayAvailable { .. }, _)) => {
                println!("[WARN] No relays available, retrying in 10s...");
                sleep(Duration::from_secs(10)).await;
            }
            _ => { eprintln!("[ERROR] hub channel closed"); return None; }
        }
    };

    let msg_id = new_msg_id();
    let msg = Message::Register {
        header: MessageHeader {
            msg_id,
            src_addr: public_addr,
            src_id:   peer_id.to_string(),
            dst_addr: relay_addr,
            dst_id:   relay_id.clone(),
            time:     now_secs(),
        }
    };
    if !send_and_wait_ack(&socket, &msg, relay_addr, ack_waiter, msg_id).await {
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

static MSG_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn new_msg_id() -> u64 {
    MSG_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct AckWaiter {
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<()>>>>,
}

impl AckWaiter {
    pub fn new() -> Self {
        Self { pending: Arc::new(Mutex::new(HashMap::new())) }
    }

    // Appeler AVANT send_msg
    pub async fn register(&self, msg_id: u64) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(msg_id, tx);
        rx
    }

    // Appelé par le dispatcher
    pub async fn resolve(&self, reply_to: u64) {
        match self.pending.lock().await.remove(&reply_to) {
            Some(tx) => { let _ = tx.send(()); }
            None => eprintln!("[WARN] Ack inattendu pour msg_id={}", reply_to),
        }
    }
}

pub type MsgItem     = (Message, SocketAddr);
pub type MsgSender   = mpsc::Sender<MsgItem>;
pub type MsgReceiver = mpsc::Receiver<MsgItem>;

pub struct NodeInbox {
    pub ack_waiter: AckWaiter,
    pub hub_rx:     MsgReceiver,
    pub general_rx: MsgReceiver,
}

pub struct NodeDispatcher {
    ack_waiter: AckWaiter,
    hub_tx:     MsgSender,
    general_tx: MsgSender,
}

pub fn create_node_channels() -> (NodeDispatcher, NodeInbox) {
    let ack_waiter               = AckWaiter::new();
    let (hub_tx,     hub_rx)     = mpsc::channel(32);
    let (general_tx, general_rx) = mpsc::channel(256);
    (
        NodeDispatcher { ack_waiter: ack_waiter.clone(), hub_tx, general_tx },
        NodeInbox      { ack_waiter,                     hub_rx, general_rx },
    )
}

impl NodeDispatcher {
    pub async fn run(self, socket: Arc<UdpSocket>) {
        loop {
            let Some((msg, addr)) = recv_msg(&socket).await else { continue };
            match &msg {
                Message::Ack { reply_to, .. } => {
                    self.ack_waiter.resolve(*reply_to).await;
                }
                Message::PeerInfo { .. } | Message::NoRelayAvailable { .. } => {
                    let _ = self.hub_tx.send((msg, addr)).await;
                }
                _ => {
                    let _ = self.general_tx.send((msg, addr)).await;
                }
            }
        }
    }
}