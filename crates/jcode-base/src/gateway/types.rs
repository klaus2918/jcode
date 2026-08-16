//! Device pairing types (merged from the removed `jcode-gateway-types` crate,
//! feature-simplification #16 / S-1).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub paired_at: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingCode {
    pub code: String,
    pub created_at: String,
    pub expires_at: String,
}
