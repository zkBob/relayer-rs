use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustodyServiceSettings {
    pub db_path: String,
    pub relayer_url: String,
    pub sync_interval_sec: u64, 
    pub admin_token: String,
}