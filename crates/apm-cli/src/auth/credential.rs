use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};

use crate::auth::{api_keys::StoredApiKey, session::SessionRecord};

const SERVICE: &str = "apm";
const SESSION_ENTRY: &str = "session";
const API_KEY_INDEX_ENTRY: &str = "api-key-index";

#[derive(Debug, Clone)]
pub struct CredentialStore {
    backend: Backend,
}

#[derive(Debug, Clone)]
enum Backend {
    Keychain,
    TestFile(PathBuf),
}

#[derive(Debug, Clone)]
pub enum ResolvedCredential {
    EnvApiKey(String),
    StoredApiKey(StoredApiKey),
    Session(SessionRecord),
}

impl CredentialStore {
    pub fn from_env() -> Self {
        let backend = match std::env::var("APM_TEST_CREDENTIAL_STORE_DIR") {
            Ok(path) if !path.trim().is_empty() => Backend::TestFile(PathBuf::from(path)),
            _ => Backend::Keychain,
        };
        Self { backend }
    }

    pub fn save_session(&self, session: &SessionRecord) -> Result<()> {
        self.write_entry(SESSION_ENTRY, &serde_json::to_string(session)?)
    }

    pub fn load_session(&self) -> Result<Option<SessionRecord>> {
        self.read_entry(SESSION_ENTRY)?
            .map(|raw| serde_json::from_str(&raw).context("failed to parse stored session"))
            .transpose()
    }

    pub fn clear_session(&self) -> Result<()> {
        self.delete_entry(SESSION_ENTRY)
    }

    pub fn save_api_key(&self, api_key: &StoredApiKey) -> Result<()> {
        let entry_name = api_key_entry(&api_key.name);
        self.write_entry(&entry_name, &serde_json::to_string(api_key)?)?;
        let mut names = self.api_key_names()?;
        if !names.iter().any(|existing| existing == &api_key.name) {
            names.push(api_key.name.clone());
            names.sort();
            self.write_entry(API_KEY_INDEX_ENTRY, &serde_json::to_string(&names)?)?;
        }
        Ok(())
    }

    pub fn list_api_keys(&self) -> Result<Vec<StoredApiKey>> {
        let mut keys: Vec<StoredApiKey> = Vec::new();
        for name in self.api_key_names()? {
            if let Some(raw) = self.read_entry(&api_key_entry(&name))? {
                keys.push(
                    serde_json::from_str(&raw)
                        .with_context(|| format!("failed to parse stored API key {name}"))?,
                );
            }
        }
        keys.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(keys)
    }

    pub fn remove_api_key(&self, name: &str) -> Result<()> {
        self.delete_entry(&api_key_entry(name))?;
        let mut names = self.api_key_names()?;
        names.retain(|existing| existing != name);
        self.write_entry(API_KEY_INDEX_ENTRY, &serde_json::to_string(&names)?)?;
        Ok(())
    }

    pub fn clear_api_keys(&self) -> Result<()> {
        for name in self.api_key_names()? {
            self.delete_entry(&api_key_entry(&name))?;
        }
        self.delete_entry(API_KEY_INDEX_ENTRY)
    }

    pub fn resolve_credential(&self) -> Result<Option<ResolvedCredential>> {
        if let Ok(api_key) = std::env::var("APM_API_KEY") {
            if !api_key.trim().is_empty() {
                return Ok(Some(ResolvedCredential::EnvApiKey(api_key)));
            }
        }

        if let Some(api_key) = self.list_api_keys()?.into_iter().next() {
            return Ok(Some(ResolvedCredential::StoredApiKey(api_key)));
        }

        if let Some(session) = self.load_session()? {
            return Ok(Some(ResolvedCredential::Session(session)));
        }

        Ok(None)
    }

    fn api_key_names(&self) -> Result<Vec<String>> {
        match self.read_entry(API_KEY_INDEX_ENTRY)? {
            Some(raw) => serde_json::from_str(&raw).context("failed to parse API key index"),
            None => Ok(Vec::new()),
        }
    }

    fn write_entry(&self, name: &str, value: &str) -> Result<()> {
        match &self.backend {
            Backend::Keychain => {
                maybe_force_keychain_error(name)?;
                let entry = keyring::Entry::new(SERVICE, name).map_err(|error| {
                    anyhow!("failed to open macOS Keychain entry {name}: {error}")
                })?;
                entry
                    .set_password(value)
                    .map_err(|error| anyhow!("failed to write {name} to macOS Keychain: {error}"))
            }
            Backend::TestFile(dir) => {
                fs::create_dir_all(dir).with_context(|| {
                    format!(
                        "failed to create test credential directory {}",
                        dir.display()
                    )
                })?;
                fs::write(file_path(dir, name), value)
                    .with_context(|| format!("failed to write test credential entry {name}"))
            }
        }
    }

    fn read_entry(&self, name: &str) -> Result<Option<String>> {
        match &self.backend {
            Backend::Keychain => {
                maybe_force_keychain_error(name)?;
                let entry = keyring::Entry::new(SERVICE, name).map_err(|error| {
                    anyhow!("failed to open macOS Keychain entry {name}: {error}")
                })?;
                match entry.get_password() {
                    Ok(value) => Ok(Some(value)),
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(error) => Err(anyhow!(
                        "failed to read {name} from macOS Keychain: {error}"
                    )),
                }
            }
            Backend::TestFile(dir) => {
                let path = file_path(dir, name);
                if !path.exists() {
                    return Ok(None);
                }
                Ok(Some(fs::read_to_string(&path).with_context(|| {
                    format!("failed to read test credential entry {}", path.display())
                })?))
            }
        }
    }

    fn delete_entry(&self, name: &str) -> Result<()> {
        match &self.backend {
            Backend::Keychain => {
                maybe_force_keychain_error(name)?;
                let entry = keyring::Entry::new(SERVICE, name).map_err(|error| {
                    anyhow!("failed to open macOS Keychain entry {name}: {error}")
                })?;
                match entry.delete_password() {
                    Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                    Err(error) => Err(anyhow!(
                        "failed to delete {name} from macOS Keychain: {error}"
                    )),
                }
            }
            Backend::TestFile(dir) => {
                let path = file_path(dir, name);
                if path.exists() {
                    fs::remove_file(&path).with_context(|| {
                        format!("failed to delete test credential entry {}", path.display())
                    })?;
                }
                Ok(())
            }
        }
    }
}

fn api_key_entry(name: &str) -> String {
    format!("api-key:{name}")
}

fn file_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.json", name.replace(':', "_")))
}

fn maybe_force_keychain_error(name: &str) -> Result<()> {
    if std::env::var("APM_TEST_FORCE_KEYCHAIN_ERROR").as_deref() == Ok("1") {
        return Err(anyhow!(
            "failed to access macOS Keychain entry {name}: forced test error"
        ));
    }
    Ok(())
}
