use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use ed25519_dalek::VerifyingKey;

use crate::p2p::utils::*;


pub type PeerId = String;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PeerInfo {
	pub id: PeerId,  // hash256 de la verifying_key (unique)
    pub addr: SocketAddr,
    pub username: String,
    pub last_seen: u64,
    pub is_relay: bool,
    pub verifying_key: VerifyingKey,
}

pub type PeersMap = Arc<Mutex<HashMap<PeerId, PeerInfo>>>;
pub type MessagesMap = Arc<Mutex<Vec<Message>>>;
pub const MAX_PACKET_SIZE: usize = 65536;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Headers {
    pub msg_id:   u64,
    pub src_addr: SocketAddr,
    pub src_username:   String,
    pub dst_addr: SocketAddr,
    pub dst_username:   String,
    pub time:     u64,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload {
    Register {  verifying_key: VerifyingKey },  // Client → Relay : "Je m'enregistre, voici mon adresse et mon id"
    Connect,  // Dial → Relay : "Mets-moi en contact avec ce peer_id"
    BeNewRelay { verifying_key: VerifyingKey },  // new Relay → Serveur stockant les adresses des relais : "Je me déclare relay"
    NeedRelay,
    NoRelayAvailable,
    PunchTheHole,
    Classic { txt: String },  // Peer → Peer : message direct (hole punching, hello, etc.)
    AskForAddr { username: String },  // Relay → Client : "Voici l'adresse+id du peer que tu cherches"
    PeerInfo { peer_info: PeerInfo },  // Relay → Client : "Voici l'adresse+id du peer que tu cherches"
    RelayHasNewClient { peer_addr: SocketAddr, peer_id: String },
    Ack { reply_to: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub headers: Headers,
    pub payload: Payload,
    pub last_hop: SocketAddr,
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.payload {
            Payload::Register { .. } => {
                write!(f, "[Register] [{}]", self.headers)
            }
            Payload::Connect => {
                write!(f, "[Connect] [{}]", self.headers)
            }
            Payload::AskForAddr { username } => {
                write!(f, "[AskForAddr] [{}] He asks for {}'s addr", self.headers, username)
            }
            Payload::PeerInfo { peer_info } => {
                write!(f, "[PeerInfo] [{}] {} ({})", self.headers, peer_info.addr, peer_info.username)
            }
            Payload::Classic { txt } => {
                write!(f, "[Classic] [{}] \"{}\"", self.headers, txt)
            }
            Payload::BeNewRelay { .. } => {
                write!(f, "[BeNewRelay] [{}]", self.headers)
            }
            Payload::NeedRelay => {
                write!(f, "[NeedRelay] [{}]", self.headers)
            }
            Payload::Ack { reply_to } => {
                write!(f, "[Ack] [{}] reply_to=#{}", self.headers, reply_to)
            }
            Payload::RelayHasNewClient { peer_addr, peer_id } => {
                write!(f, "[RelayHasNewClient] [{}] {} ({}) wants to connect to you as relay", self.headers, peer_addr, peer_id)
            }
            Payload::NoRelayAvailable => {
                write!(f, "[NoRelayAvailable] [{}]", self.headers)
            }
            Payload::PunchTheHole => {
                write!(f, "[PunchTheHole] [{}]", self.headers)
            }
        }
    }
}

impl fmt::Display for Headers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) -> {} ({}) (#{} - {})",
            self.src_addr, self.src_username,
            self.dst_addr, self.dst_username,
            self.msg_id,
            fmt_time(self.time)
        )
    }
}
