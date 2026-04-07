use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredApiKey {
    pub name: String,
    pub key: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
}
