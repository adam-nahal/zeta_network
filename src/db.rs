use std::collections::HashMap;
use ed25519_dalek::VerifyingKey;
use rusqlite::{params, Connection, Result};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::net::SocketAddr;

use crate::p2p::*;

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
            	id TEXT PRIMARY KEY,
                addr TEXT NOT NULL,
                username TEXT NOT NULL,
                last_seen INTEGER NOT NULL,
                is_relay INTEGER NOT NULL,
                verifying_key BLOB NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS logs (
                msg_id INTEGER NOT NULL,
                time INTEGER NOT NULL,
                src_addr TEXT NOT NULL,
                src_id TEXT NOT NULL,
                dst_addr TEXT NOT NULL,
                dst_id TEXT NOT NULL,
                msg_type TEXT NOT NULL,
                payload TEXT,
                last_hop TEXT NOT NULL,
                signature BLOB NOT NULL,
                UNIQUE(msg_id, time, src_id)
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
	    let mut stmt = conn.prepare("SELECT id, addr, username, last_seen, is_relay, verifying_key FROM peers")?;
	    let mut rows = stmt.query([])?;
	    let peers = Arc::new(Mutex::new(HashMap::new()));
	    while let Some(row) = rows.next()? {
	        let id: Vec<u8> = row.get(0)?;
	        let addr: SocketAddr = row.get::<_, String>(1)?.parse()
	        	.expect("[ERROR] Badly formated addr in db");
	        let verifying_key: [u8; 32] = row.get::<_, Vec<u8>>(5)?.try_into()
	        	.expect("[ERROR] Badly formated verifying_key in db");
	        let verifying_key: VerifyingKey = VerifyingKey::from_bytes(&verifying_key)
	        	.expect("[ERROR] Badly formated verifying_key in db");

	        peers.lock().await.insert(id.clone(), PeerInfo {
	        	id,
	            addr,
	            username: row.get(2)?,
	            last_seen: row.get(3)?,
	            is_relay: row.get(4)?,
	            verifying_key,
	        });
	    }
	    Ok(peers)
	}

	pub async fn get_logs_from_db(&self) -> Result<MessagesMap> {
	    let conn = self.conn.lock().await;
	    let mut stmt = conn.prepare(
	        "SELECT msg_id, time, src_addr, src_id, dst_addr, dst_id, msg_type, payload, last_hop, signature FROM logs"
	    )?;
	    let mut rows = stmt.query([])?;
	    let logs = Arc::new(Mutex::new(Vec::new()));
	    while let Some(row) = rows.next()? {
	        let msg_type: String = row.get(6)?;
	        let payload_str: Option<String> = row.get(7)?;

	        let payload = match (msg_type.as_str(), payload_str.as_deref()) {
	            ("Classic", Some(txt))         => Payload::Classic { txt: txt.to_string() },
	            ("AskForAddr", Some(username))  => Payload::AskForAddr { username: username.to_string() },
	            ("Ack", Some(msg_id))               => Payload::Ack { reply_to: msg_id.parse().unwrap_or(0) },
	            ("PeerInfo", Some(peer_info))          => {
	                let mut parts = peer_info.splitn(6, '|');
	                Payload::PeerInfo { peer_info: PeerInfo {
	                	id: parts.next().expect("Wrong id in db").as_bytes().to_vec(),
	                	addr: parts.next().expect("Wrong addr in db").parse().expect("Wrong addr in db"), 
	                	username: parts.next().expect("Wrong addr in db").to_string(),
	                	last_seen: parts.next().expect("Wrong last_seen in db").parse().expect("Wrong last_seen in db"),
	                	is_relay: parts.next().expect("Wrong bool in db").parse().expect("Wrong bool in db"),
	                	verifying_key: VerifyingKey::from_bytes(
							&hex::decode(parts.next().expect("Missing verifying_key"))
								.expect("Invalid hex").try_into().expect("Not 32 bytes")
						).expect("Invalid verifying_key in db"),
	                }}
	            }
	            ("RelayHasNewClient", Some(s)) => {
	                let mut parts = s.splitn(2, '|');
	                let peer_addr = parts.next().unwrap_or("0.0.0.0:0").parse().unwrap();
	                let peer_id   = parts.next().unwrap_or("").to_string();
	                Payload::RelayHasNewClient { peer_addr, peer_id }
	            }
	            ("BeNewRelay", Some(hex_str)) => {
	            	let bytes = hex::decode(hex_str)
	                    .map_err(|_| rusqlite::Error::InvalidColumnType(7, "Hex string".into(), rusqlite::types::Type::Text))?;
	                let key_bytes: [u8; 32] = bytes.try_into()
	                    .map_err(|_| rusqlite::Error::InvalidColumnType(7, "32-byte array".into(), rusqlite::types::Type::Text))?;
	                let verifying_key = VerifyingKey::from_bytes(&key_bytes)
	                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e)))?;
	            	Payload::BeNewRelay { verifying_key }
	        	}
	            ("Register", Some(hex_str)) => {
	            	let bytes = hex::decode(hex_str)
	                    .map_err(|_| rusqlite::Error::InvalidColumnType(7, "Hex string".into(), rusqlite::types::Type::Text))?;
	                let key_bytes: [u8; 32] = bytes.try_into()
	                    .map_err(|_| rusqlite::Error::InvalidColumnType(7, "32-byte array".into(), rusqlite::types::Type::Text))?;
	                let verifying_key = VerifyingKey::from_bytes(&key_bytes)
	                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e)))?;
	                Payload::Register { verifying_key }
	            }
	            ("Connect", _)           => Payload::Connect,
	            ("NeedRelay", _)         => Payload::NeedRelay,
	            ("NoRelayAvailable", _)  => Payload::NoRelayAvailable,
	            ("PunchTheHole", _)      => Payload::PunchTheHole,
	            _                        => continue,
	        };

	        logs.lock().await.push(Message {
	            headers: Headers {
	                msg_id:   row.get(0)?,
	                time:     row.get(1)?,
	                src_addr: row.get::<_, String>(2)?.parse().map_err(|_| rusqlite::Error::InvalidColumnType(2, "SocketAddr".into(), rusqlite::types::Type::Text))?,
	                src_username:   row.get(3)?,
	                dst_addr: row.get::<_, String>(4)?.parse().map_err(|_| rusqlite::Error::InvalidColumnType(4, "SocketAddr".into(), rusqlite::types::Type::Text))?,
	                dst_username:   row.get(5)?,
	                signature: row.get(9)?,
	            },
	            payload,
	            last_hop: row.get::<_, String>(8)?.parse().map_err(|_| rusqlite::Error::InvalidColumnType(4, "SocketAddr".into(), rusqlite::types::Type::Text))?,
	        });
	    }
	    Ok(logs)
	}

	pub async fn refresh_peers_in_db(&self, peers: &PeersMap) -> Result<()> {
	    let peers = peers.lock().await.clone();
	    let conn = Arc::clone(&self.conn);
	    
		tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
		    // Ajoute ou met à jour les noeuds de la base de données
	        let mut conn = conn.blocking_lock();
		    let tx = conn.transaction()?;  // Permet l'atomicité du refresh (soit tout est maj, soit rien)
		    for (id, peer_info) in peers.iter() {
		        tx.execute(
		            "INSERT INTO peers (id, addr, username, last_seen, is_relay, verifying_key)
		             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
		             ON CONFLICT(id) DO UPDATE SET
		                 id = excluded.id,
		                 last_seen = excluded.last_seen,
		                 is_relay = excluded.is_relay",
		            params![id, peer_info.addr.to_string(), peer_info.username, peer_info.last_seen, peer_info.is_relay, peer_info.verifying_key.as_bytes().to_vec()],
		        )?;
		    }

		    // Supprime les noeuds déconnectés ou anciens
		    let placeholders = peers
		        .keys()
		        .enumerate()
		        .map(|(i, _)| format!("?{}", i + 1))
		        .collect::<Vec<_>>()
		        .join(", ");
		    let addrs: Vec<String> = peers.values().map(|peer_info| peer_info.addr.to_string()).collect();
		    let query = format!("DELETE FROM peers WHERE addr NOT IN ({})", placeholders);
		    tx.execute(&query, rusqlite::params_from_iter(addrs.iter()))?;

		    tx.commit()?;
		    Ok(())
		}).await.map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?
	}

    pub async fn refresh_logs_in_db(&self, logs: &MessagesMap) -> Result<()> {
	    let logs = logs.lock().await.clone();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
        	let mut conn = conn.blocking_lock();
        	let tx = conn.transaction()?;
	        for log in logs.iter() {
		        let (msg_type, payload) = match &log.payload {
		            Payload::Classic { txt } => ("Classic", Some(txt.clone())),
				    Payload::Ack { reply_to } => ("Ack", Some(reply_to.to_string())),
				    Payload::PeerInfo { peer_info } => ("PeerInfo", Some(format!(
						"{}|{}|{}|{}|{}|{}", 
						hex::encode(peer_info.id.clone()),
						peer_info.addr, 
						peer_info.username, 
						peer_info.last_seen, 
						peer_info.is_relay, 
						hex::encode(peer_info.verifying_key.as_bytes()),
					))),
				    Payload::RelayHasNewClient { peer_addr, peer_id } => ("RelayHasNewClient", Some(format!("{}|{}", peer_addr, peer_id))),
				    Payload::AskForAddr { username } => ("AskForAddr", Some(username.clone())),
				    Payload::Register { verifying_key } => ("Register", Some(hex::encode(verifying_key.to_bytes()))),
				    Payload::Connect => ("Connect", None),
				    Payload::BeNewRelay { verifying_key } => ("BeNewRelay", Some(hex::encode(verifying_key.to_bytes()))),
				    Payload::NeedRelay => ("NeedRelay", None),
				    Payload::NoRelayAvailable => ("NoRelayAvailable", None),
				    Payload::PunchTheHole => ("PunchTheHole", None),
		        };
		        tx.execute(
		            "INSERT OR IGNORE INTO logs (msg_id, time, src_addr, src_id, dst_addr, dst_id, msg_type, payload, last_hop, signature)
		             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
		            params![
		                log.headers.msg_id,
		                log.headers.time,
		                log.headers.src_addr.to_string(),
		                log.headers.src_username,
		                log.headers.dst_addr.to_string(),
		                log.headers.dst_username,
		                msg_type.to_string(),
		                payload,
		                log.last_hop.to_string(),
		                log.headers.signature,
		            ],
		        )?;
		    }
		    tx.commit()?;
		    Ok(())
		}).await.map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?
    }

    pub async fn get_max_msg_id(&self, public_addr: SocketAddr) -> Result<u64> {
	    let conn = self.conn.lock().await;
	    let mut stmt = conn.prepare(
	        "SELECT COALESCE(MAX(msg_id), 0) FROM logs WHERE src_addr = ?1"
	    )?;
	    let max_id = stmt.query_row(params![public_addr.to_string()], |row| row.get(0))?;
	    Ok(max_id)
    }
}
