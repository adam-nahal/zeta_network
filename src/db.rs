use std::collections::HashMap;
use rusqlite::{params, Connection, Result};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::net::SocketAddr;

use crate::lib_p2p::*;

// Gestionnaire de base de données (thread-safe)
#[derive(Clone)]
pub struct DbManager {
    conn: Arc<Mutex<Connection>>,
}

impl DbManager {
    // Initialise la base (crée les tables si absentes)
    pub async fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS peers (
                addr TEXT PRIMARY KEY,
                id TEXT NOT NULL,
                last_seen INTEGER NOT NULL,
                is_relay INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS logs (
                msg_id INTEGER PRIMARY KEY,
                time INTEGER NOT NULL,
                src_addr TEXT NOT NULL,
                src_id TEXT NOT NULL,
                dst_addr TEXT NOT NULL,
                dst_id TEXT NOT NULL,
                msg_type TEXT NOT NULL,
                payload TEXT
            )",
            [],
        )?;

    	println!("Database created as '{}'", db_path);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

	pub async fn get_peers_from_db(&self) -> Result<PeersMap> {
	    let conn = self.conn.lock().await;
	    let mut stmt = conn.prepare("SELECT addr, id, last_seen, is_relay FROM peers")?;
	    let mut rows = stmt.query([])?;
	    let peers = Arc::new(Mutex::new(HashMap::new()));
	    while let Some(row) = rows.next()? {
	        let addr_str: String = row.get(0)?;
	        let addr = addr_str.parse::<SocketAddr>()
	            .map_err(|_| rusqlite::Error::InvalidColumnType(0, "SocketAddr".to_string(), rusqlite::types::Type::Text))?;
	        peers.lock().await.insert(addr, PeerInfo {
	            addr,
	            id: row.get(1)?,
	            last_seen: row.get(2)?,
	            is_relay: row.get(3)?,
	        });
	    }
	    Ok(peers)
	}

	pub async fn get_logs_from_db(&self) -> Result<MessagesMap> {
	    let conn = self.conn.lock().await;
	    let mut stmt = conn.prepare(
	        "SELECT msg_id, time, src_addr, src_id, dst_addr, dst_id, msg_type, payload FROM logs"
	    )?;
	    let mut rows = stmt.query([])?;
	    let logs = Arc::new(Mutex::new(Vec::new()));
	    while let Some(row) = rows.next()? {
	        let src_str: String = row.get(2)?;
	        let dst_str: String = row.get(4)?;
	        let msg_type: String = row.get(6)?;
	        let payload_str: Option<String> = row.get(7)?;

	        let payload = match (msg_type.as_str(), payload_str.as_deref()) {
	            ("Classic", Some(txt))         => Payload::Classic { txt: txt.to_string() },
	            ("AskForAddr", Some(peer_id))  => Payload::AskForAddr { peer_id: peer_id.to_string() },
	            ("Ack", Some(s))               => Payload::Ack { reply_to: s.parse().unwrap_or(0) },
	            ("PeerInfo", Some(s))          => {
	                let mut parts = s.splitn(2, '|');
	                let peer_addr = parts.next().unwrap_or("0.0.0.0:0").parse().unwrap();
	                let peer_id   = parts.next().unwrap_or("").to_string();
	                Payload::PeerInfo { peer_addr, peer_id }
	            }
	            ("RelayHasNewClient", Some(s)) => {
	                let mut parts = s.splitn(2, '|');
	                let peer_addr = parts.next().unwrap_or("0.0.0.0:0").parse().unwrap();
	                let peer_id   = parts.next().unwrap_or("").to_string();
	                Payload::RelayHasNewClient { peer_addr, peer_id }
	            }
	            ("Register", _)          => Payload::Register,
	            ("Connect", _)           => Payload::Connect,
	            ("BeNewRelay", _)        => Payload::BeNewRelay,
	            ("NeedRelay", _)         => Payload::NeedRelay,
	            ("NoRelayAvailable", _)  => Payload::NoRelayAvailable,
	            ("PunchTheHole", _)      => Payload::PunchTheHole,
	            _                        => continue,
	        };

	        logs.lock().await.push(Message {
	            headers: Headers {
	                msg_id:   row.get(0)?,
	                time:     row.get(1)?,
	                src_addr: src_str.parse().map_err(|_| rusqlite::Error::InvalidColumnType(2, "SocketAddr".into(), rusqlite::types::Type::Text))?,
	                src_id:   row.get(3)?,
	                dst_addr: dst_str.parse().map_err(|_| rusqlite::Error::InvalidColumnType(4, "SocketAddr".into(), rusqlite::types::Type::Text))?,
	                dst_id:   row.get(5)?,
	            },
	            payload,
	        });
	    }
	    Ok(logs)
	}

	pub async fn refresh_peers(&self, peer_map: PeersMap) -> Result<()> {
	    let peers_guard = peer_map.lock().await;
	    let mut conn = self.conn.lock().await;
	    let tx = conn.transaction()?;  // Permet l'atomicité du refresh (soit tout est maj, soit rien)

	    // Ajoute ou met à jour les noeuds de la base de données
	    for (addr, peer_info) in peers_guard.iter() {
	        tx.execute(
	            "INSERT INTO peers (addr, id, last_seen, is_relay)
	             VALUES (?1, ?2, ?3, ?4)
	             ON CONFLICT(addr) DO UPDATE SET
	                 id = excluded.id,
	                 last_seen = excluded.last_seen,
	                 is_relay = excluded.is_relay",
	            params![addr.to_string(), peer_info.id, peer_info.last_seen, peer_info.is_relay],
	        )?;
	    }

	    // Supprime les noeuds déconnectés ou anciens
	    let placeholders = peers_guard
	        .keys()
	        .enumerate()
	        .map(|(i, _)| format!("?{}", i + 1))
	        .collect::<Vec<_>>()
	        .join(", ");
	    let addrs: Vec<String> = peers_guard.keys().map(|a| a.to_string()).collect();
	    let query = format!("DELETE FROM peers WHERE addr NOT IN ({})", placeholders);
	    tx.execute(&query, rusqlite::params_from_iter(addrs.iter()))?;

	    tx.commit()?;
	    Ok(())
	}

    pub async fn refresh_logs(&self, logs: MessagesMap) -> Result<()> {
	    let logs2 = logs.lock().await;
        let mut conn = self.conn.lock().await;
	    let tx = conn.transaction()?;

        for log in logs2.iter() {
	        let (msg_type, payload) = match &log.payload {
	            Payload::Classic { txt } => ("Classic", Some(txt.clone())),
			    Payload::Ack { reply_to } => ("Ack", Some(reply_to.to_string())),
			    Payload::PeerInfo { peer_addr, peer_id } => ("PeerInfo", Some(format!("{}|{}", peer_addr, peer_id))),
			    Payload::RelayHasNewClient { peer_addr, peer_id } => ("RelayHasNewClient", Some(format!("{}|{}", peer_addr, peer_id))),
			    Payload::AskForAddr { peer_id } => ("AskForAddr", Some(peer_id.clone())),
			    Payload::Register => ("Register", None),
			    Payload::Connect => ("Connect", None),
			    Payload::BeNewRelay => ("BeNewRelay", None),
			    Payload::NeedRelay => ("NeedRelay", None),
			    Payload::NoRelayAvailable => ("NoRelayAvailable", None),
			    Payload::PunchTheHole => ("PunchTheHole", None),
	        };
	        tx.execute(
	            "INSERT OR REPLACE INTO logs (msg_id, time, src_addr, src_id, dst_addr, dst_id, msg_type, payload)
	             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
	            params![
	                log.headers.msg_id,
	                log.headers.time,
	                log.headers.src_addr.to_string(),
	                log.headers.src_id,
	                log.headers.dst_addr.to_string(),
	                log.headers.dst_id,
	                msg_type,
	                payload,
	            ],
	        )?;
	    }

	    tx.commit()?;
    	Ok(())
    }
}
