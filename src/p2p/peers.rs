use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration};
use std::net::SocketAddr;

use crate::p2p::types::*;
use crate::p2p::network::*;
use crate::p2p::dispatch::*;
use crate::p2p::utils::*;

use super::AuthKeys;


pub async fn delete_disconnected_peers(peers: &PeersMap) {
    let mut peers = peers.lock().await;
    peers.retain(|_, peer_info| {
        let active = now_secs() - peer_info.last_seen < 120;
        if !active { println!("[INFO] Connect with {} lost (timeout)", peer_info.addr); }
        active
    });
}

pub async fn relay_message(peers: &PeersMap, relayer_addr: SocketAddr, mut msg: Message, socket: &UdpSocket, logs: &MessagesMap) {
    let peers = peers.lock().await;

    msg.last_hop = relayer_addr;
    for (_, peer_info) in peers.iter() {
        if peer_info.addr != msg.headers.src_addr {
            if let Err(e) = socket.send_msg(msg.clone(), peer_info.addr, logs).await {
                eprintln!("Failed to send to {}: {}", peer_info.addr, e);
            } else {
                println!("    Relayed to {}", peer_info.addr);
            }
        }
    }
}

pub async fn connect_to_a_relay(socket: &UdpSocket, public_addr: SocketAddr, peer_id: &str, hub_relay_addr: SocketAddr, hub_rx: &mut MsgReceiver, ack_waiter: &AckWaiter, logs: &MessagesMap, auth_keys: &AuthKeys) -> Option<(SocketAddr, String)> {
    let (relay_addr, relay_id) = loop {
        println!("\nAsking the hub relay an available relay...");
        let mut msg = Message {
            headers: Headers {
                msg_id: new_msg_id(),
                src_addr: public_addr,
                src_id: peer_id.to_string(),
                dst_addr: hub_relay_addr,
                dst_id: "hub".to_string(),
                time: now_secs(),
                signature: vec![],
            },
            payload: Payload::NeedRelay,
            last_hop: public_addr,
        };
        let _ = msg.sign(&auth_keys.signing_key);
        let _ = socket.send_msg(msg, hub_relay_addr, &logs).await;

        match hub_rx.recv().await {
        	Some((msg, _)) => match &msg.payload {
		        Payload::PeerInfo { peer_info } => {
		            println!("Received relay address {} ({})", peer_info.addr, peer_info.username);
		            sleep(Duration::from_millis(2000)).await;  // Attendre le hole punching chez le relais
		            break (peer_info.addr, peer_info.username.clone());
		        }
		        Payload::NoRelayAvailable => {
		            println!("[WARN] No relays available, retrying in 10s...");
		            sleep(Duration::from_secs(10)).await;
		        }
		        _ => { eprintln!("[ERROR] Unexpected message"); return None; }
		    },
		    std::prelude::v1::None => { eprintln!("[ERROR] hub channel closed"); return None; }
        }
    };

    let mut msg = Message {
        headers: Headers {
            msg_id: new_msg_id(),
            src_addr: public_addr,
            src_id: peer_id.to_string(),
            dst_addr: relay_addr,
            dst_id: relay_id.clone(),
            time: now_secs(),
            signature: vec![],
        },
        payload: Payload::Register { verifying_key: auth_keys.verifying_key },
        last_hop: public_addr,
    };
    let _ = msg.sign(&auth_keys.signing_key);
    if !socket.send_and_wait_ack(msg, relay_addr, ack_waiter, &logs).await {
        println!("[ERROR] No ack from relay, aborting");
        return None;
    }

    Some((relay_addr, relay_id))
}
