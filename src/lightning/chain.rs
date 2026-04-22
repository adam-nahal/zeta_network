//FeeEstimator + BroadcasterInterface via Esplora (API)
//

use std::sync::Arc;
use tokio::sync::RwLock;

use bitcoin::{Network, Transaction};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};

