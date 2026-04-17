use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration};
use std::net::SocketAddr;

use crate::p2p::types::*;
use crate::p2p::network::*;
use crate::p2p::dispatch::*;
use crate::p2p::utils::*;


pub async fn delete_disconnected_peers(peers: &PeersMap) {
    let mut peers_map = peers.lock().await;
    peers_map.retain(|addr, peer_info| {
        let active = now_secs() - peer_info.last_seen < 120;
        if !active { println!("[INFO] Connect with {} lost (timeout)", addr); }
        active
    });
}

pub async fn relay_message(peers: &PeersMap, sender_addr: SocketAddr, mut msg: Message, socket: &UdpSocket, logs: &MessagesMap) {
    let peers_map = peers.lock().await;

    for (other_addr, _) in peers_map.iter() {
        if other_addr != &sender_addr {
        	msg.last_hop = *other_addr;
            if let Err(e) = socket.send_msg(msg.clone(), *other_addr, logs).await {
                eprintln!("Failed to send to {}: {}", other_addr, e);
            } else {
                println!("    Relayed to {}", other_addr);
            }
        }
    }
}

pub async fn connect_to_a_relay(socket: &UdpSocket, public_addr: SocketAddr, peer_id: &str, hub_relay_addr: SocketAddr, hub_rx: &mut MsgReceiver, ack_waiter: &AckWaiter, logs: &MessagesMap) -> Option<(SocketAddr, String)> {
    let (relay_addr, relay_id) = loop {
        println!("\nAsking the hub relay an available relay...");
        let msg = Message {
            headers: Headers {
                msg_id:   new_msg_id(),
                src_addr: public_addr,
                src_id:   peer_id.to_string(),
                dst_addr: hub_relay_addr,
                dst_id:   "hub".to_string(),
                time:     now_secs(),
            },
            payload: Payload::NeedRelay,
            last_hop: public_addr,
        };
        let _ = socket.send_msg(msg, hub_relay_addr, &logs).await;

        match hub_rx.recv().await {
        	Some((msg, _)) => match &msg.payload {
		        Payload::PeerInfo { peer_addr, peer_id: rid } => {
		            println!("Received relay address {} ({})", peer_addr, rid);
		            sleep(Duration::from_millis(2000)).await;  // Attendre le hole punching chez le relais
		            break (*peer_addr, rid.clone());
		        }
		        Payload::NoRelayAvailable => {
		            println!("[WARN] No relays available, retrying in 10s...");
		            sleep(Duration::from_secs(10)).await;
		        }
		        _ => { eprintln!("[ERROR] Unexpected message"); return None; }
		    },
		    None => { eprintln!("[ERROR] hub channel closed"); return None; }
        }
    };

    let msg = Message {
        headers: Headers {
            msg_id: new_msg_id(),
            src_addr: public_addr,
            src_id:   peer_id.to_string(),
            dst_addr: relay_addr,
            dst_id:   relay_id.clone(),
            time:     now_secs(),
        },
        payload: Payload::Register,
        last_hop: public_addr,
    };
    if !socket.send_and_wait_ack(msg, relay_addr, ack_waiter, &logs).await {
        println!("[ERROR] No ack from relay, aborting");
        return None;
    }

    Some((relay_addr, relay_id))
}