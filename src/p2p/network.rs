use async_trait::async_trait;
use std::net::ToSocketAddrs;
use tokio::net::UdpSocket;
use std::net::SocketAddr;
use anyhow::Result;
use tokio::time::{timeout, Duration};

use crate::p2p::types::*;
use crate::p2p::dispatch::AckWaiter;

#[async_trait]
pub trait UdpSocketExt {
    async fn send_msg(&self, msg: Message, next_hop: SocketAddr, logs: &MessagesMap) -> Result<usize>;
    async fn send_and_wait_ack(&self, msg: Message, dest: SocketAddr, ack_waiter: &AckWaiter, logs: &MessagesMap) -> bool;
}

#[async_trait::async_trait]
impl UdpSocketExt for UdpSocket {
    async fn send_msg(&self, msg: Message, next_hop: SocketAddr, logs: &MessagesMap) -> Result<usize> {
        let encoded = bincode::serialize(&msg)?;
        let size = self.send_to(&encoded, next_hop).await?;
		println!("->{}", msg);
		logs.lock().await.push(msg);
		Ok(size)
    }

    // Enregistre l'attente, envoie le message, attend le signal
	async fn send_and_wait_ack(&self, msg: Message, dest: SocketAddr, ack_waiter: &AckWaiter, logs: &MessagesMap) -> bool {
	    let rx = ack_waiter.register(msg.headers.msg_id).await;   // register AVANT send
	    let _ = self.send_msg(msg, dest, logs).await;
	    match timeout(Duration::from_secs(10), rx).await {
	        Ok(Ok(())) => true,
	        _ => { eprintln!("[TIMEOUT] No acks received"); false }
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
    let (size, sender_addr) = match socket.recv_from(&mut buf).await {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[WARN] recv_from failed: {}", e);
            return None;
        }
    };

    if size == 0 || size > MAX_PACKET_SIZE {
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
