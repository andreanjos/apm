use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub user_id: i64,
    pub email: String,
}

impl SessionRecord {
    pub fn is_expired(&self) -> bool {
        self.expires_at <= Utc::now() + chrono::Duration::seconds(30)
    }
}
