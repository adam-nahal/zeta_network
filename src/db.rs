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
                content TEXT
            )",
            [],
        )?;

    	println!("Database created as '{}'", db_path);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // Insère ou met à jour un noeud
    pub async fn upsert_peer(&self, addr: SocketAddr, id: &str, last_seen: u64) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO peers (addr, id, last_seen, is_relay) VALUES (?1, ?2, ?3, 1)",
            params![addr.to_string(), id, last_seen],
        )?;
        Ok(())
    }

    // Supprime un noeud (quand il est inactif)
    pub async fn delete_one_peer(&self, addr: SocketAddr) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM peers WHERE addr = ?1", params![addr.to_string()])?;
        Ok(())
    }

	pub async fn get_all_peers(&self) -> Result<Vec<PeerInfo>> {
	    let conn = self.conn.lock().await;
	    let mut stmt = conn.prepare("SELECT addr, id, last_seen, is_relay FROM peers")?;
	    let mut rows = stmt.query([])?;
	    let mut peers = Vec::new();
	    while let Some(row) = rows.next()? {
	        let addr_str: String = row.get(0)?;
	        let addr = addr_str.parse::<SocketAddr>()
	            .map_err(|_| rusqlite::Error::InvalidColumnType(0, "SocketAddr".to_string(), rusqlite::types::Type::Text))?;
	        peers.push(PeerInfo {
	            addr,
	            id: row.get(1)?,
	            last_seen: row.get(2)?,
	            is_relay: row.get(3)?,
	        });
	    }
	    Ok(peers)
	}

	pub async fn get_all_logs(&self) -> Result<Vec<PeerInfo>> {
	    let conn = self.conn.lock().await;
	    let mut stmt = conn.prepare("SELECT addr, id, last_seen, is_relay FROM peers")?;
	    let mut rows = stmt.query([])?;
	    let mut peers = Vec::new();
	    while let Some(row) = rows.next()? {
	        let addr_str: String = row.get(0)?;
	        let addr = addr_str.parse::<SocketAddr>()
	            .map_err(|_| rusqlite::Error::InvalidColumnType(0, "SocketAddr".to_string(), rusqlite::types::Type::Text))?;
	        peers.push(PeerInfo {
	            addr,
	            id: row.get(1)?,
	            last_seen: row.get(2)?,
	            is_relay: row.get(3)?,
	        });
	    }
	    Ok(peers)
	}

pub async fn refresh_peers(&self, peer_map: Arc<Mutex<HashMap<SocketAddr, PeerInfo>>>) -> Result<()> {
    let peers_guard = peer_map.lock().await;
    let mut conn = self.conn.lock().await;
    let tx = conn.transaction()?;

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

    // Journaliser un message
    pub async fn refresh_logs(&self, msg: &crate::lib_p2p::Message, src_addr: SocketAddr, dst_addr: SocketAddr, timestamp: u64) -> Result<()> {
        let conn = self.conn.lock().await;
        let (msg_type, content) = match msg {
            crate::lib_p2p::Message::Classic { txt, .. } => ("Classic", Some(txt.clone())),
            _ => (std::any::type_name_of_val(msg), None),
        };
        // Extraire les IDs depuis le header selon le type de message (simplifié)
        let (src_id, dst_id) = match msg {
            crate::lib_p2p::Message::Register { header } | crate::lib_p2p::Message::Connect { header } | crate::lib_p2p::Message::BeNewRelay { header } => 
                (header.src_id.clone(), header.dst_id.clone()),
            _ => ("?".to_string(), "?".to_string()),
        };
        conn.execute(
            "INSERT INTO logs (timestamp, src_addr, src_id, dst_addr, dst_id, msg_type, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                timestamp,
                src_addr.to_string(),
                src_id,
                dst_addr.to_string(),
                dst_id,
                msg_type,
                content,
            ],
        )?;
        Ok(())
    }
}
