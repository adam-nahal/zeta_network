use std::sync::atomic::{AtomicU64, Ordering};
use std::net::SocketAddr;
use crate::db::DbManager;


static MSG_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

pub fn new_msg_id() -> u64 {
    MSG_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub async fn init_msg_id(db: &DbManager, public_addr: SocketAddr) -> u64 {
    let max_id = db.get_max_msg_id(public_addr).await.unwrap_or(0);
    let init_val = if max_id == 0 { 1 } else { max_id + 1 };
    MSG_COUNTER.store(init_val, Ordering::Relaxed);
    init_val
}

pub fn fmt_time(time: u64) -> String {
    let seconds = time % 60;
    let minutes = (time / 60) % 60;
    let hours = (time / 3600) % 24;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
