use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::net::UdpSocket;
use std::sync::Arc;
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::p2p::network::*;
use crate::p2p::types::*;


pub type MsgItem     = (Message, SocketAddr);
pub type MsgSender   = mpsc::Sender<MsgItem>;
pub type MsgReceiver = mpsc::Receiver<MsgItem>;

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
            std::prelude::v1::None => eprintln!("[WARN] Ack inattendu pour msg_id={}", reply_to),
        }
    }
}
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
        NodeDispatcher { ack_waiter: ack_waiter.clone(), hub_tx, general_tx },  // Envoie (redirigé vers dispatcher)
        NodeInbox      { ack_waiter,                     hub_rx, general_rx },  // Réception des messages
    )
}

impl NodeDispatcher {
    pub async fn run(self, socket: Arc<UdpSocket>, logs: MessagesMap) {
        loop {
            let Some((msg, addr)) = recv_msg(&socket).await else { continue };
            logs.lock().await.push(msg.clone());
            match &msg.payload {
                Payload::Ack { reply_to } => {
                    self.ack_waiter.resolve(*reply_to).await;
                }
                Payload::PeerInfo { .. } | Payload::NoRelayAvailable => {
                    let _ = self.hub_tx.send((msg, addr)).await;
                }
                _ => {
                    let _ = self.general_tx.send((msg, addr)).await;
                }
            }
        }
    }
}
