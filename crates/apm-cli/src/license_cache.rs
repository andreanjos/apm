use anyhow::{anyhow, Context, Result};
use apm_core::{
    config::Config,
    license::{verify_signed_license, SignedLicense},
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone)]
pub struct CachedLicense {
    pub plugin_slug: String,
    pub order_id: i64,
    pub status: String,
    pub license: SignedLicense,
    pub public_key_hex: String,
    pub last_synced_at: DateTime<Utc>,
}

pub struct LicenseCache {
    connection: Connection,
}

impl LicenseCache {
    pub fn open(config: &Config) -> Result<Self> {
        let path = config.license_cache_db_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let connection = Connection::open(&path)
            .with_context(|| format!("failed to open license cache {}", path.display()))?;
        let cache = Self { connection };
        cache.initialize()?;
        Ok(cache)
    }

    pub fn upsert_license(
        &self,
        status: &str,
        public_key_hex: &str,
        license: &SignedLicense,
    ) -> Result<()> {
        self.connection.execute(
            r#"
            INSERT INTO licenses (
                plugin_slug,
                order_id,
                status,
                license_json,
                public_key_hex,
                last_synced_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(plugin_slug) DO UPDATE SET
                order_id = excluded.order_id,
                status = excluded.status,
                license_json = excluded.license_json,
                public_key_hex = excluded.public_key_hex,
                last_synced_at = excluded.last_synced_at
            "#,
            params![
                license.payload.plugin_slug,
                license.payload.order_id,
                status,
                serde_json::to_string(license)?,
                public_key_hex,
                Utc::now(),
            ],
        )?;
        Ok(())
    }

    pub fn load_license(&self, plugin_slug: &str) -> Result<Option<CachedLicense>> {
        self.connection
            .query_row(
                r#"
                SELECT plugin_slug, order_id, status, license_json, public_key_hex, last_synced_at
                FROM licenses
                WHERE plugin_slug = ?1
                "#,
                params![plugin_slug],
                |row| {
                    let license_json: String = row.get(3)?;
                    Ok(CachedLicense {
                        plugin_slug: row.get(0)?,
                        order_id: row.get(1)?,
                        status: row.get(2)?,
                        license: serde_json::from_str(&license_json).map_err(to_sql_error)?,
                        public_key_hex: row.get(4)?,
                        last_synced_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_licenses(&self) -> Result<Vec<CachedLicense>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT plugin_slug, order_id, status, license_json, public_key_hex, last_synced_at
            FROM licenses
            ORDER BY plugin_slug ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            let license_json: String = row.get(3)?;
            Ok(CachedLicense {
                plugin_slug: row.get(0)?,
                order_id: row.get(1)?,
                status: row.get(2)?,
                license: serde_json::from_str(&license_json).map_err(to_sql_error)?,
                public_key_hex: row.get(4)?,
                last_synced_at: row.get(5)?,
            })
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn verify_active_license(&self, plugin_slug: &str) -> Result<CachedLicense> {
        let cached = self
            .load_license(plugin_slug)?
            .ok_or_else(|| anyhow!("no cached license exists for '{plugin_slug}'"))?;

        if cached.status != "active" {
            anyhow::bail!(
                "cached license for '{}' is not active (status: {})",
                plugin_slug,
                cached.status
            );
        }

        verify_signed_license(&cached.public_key_hex, &cached.license)?;
        Ok(cached)
    }

    fn initialize(&self) -> Result<()> {
        self.connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS licenses (
                plugin_slug TEXT PRIMARY KEY,
                order_id INTEGER NOT NULL,
                status TEXT NOT NULL,
                license_json TEXT NOT NULL,
                public_key_hex TEXT NOT NULL,
                last_synced_at TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }
}

fn to_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}
