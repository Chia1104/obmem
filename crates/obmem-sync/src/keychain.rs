//! What a device needs to reach its backup, kept in the OS keychain (macOS Keychain, Windows
//! Credential Manager, Secret Service elsewhere): the bucket's access key and the unlocked vault
//! key. One entry per bucket location, so the keychain asks for access once.

use crate::key::VaultKey;
use crate::remote::{Credentials, location};
use age::secrecy::ExposeSecret;
use anyhow::{Context, Result, anyhow};
use obmem_core::config::SyncConfig;
use serde::{Deserialize, Serialize};

const SERVICE: &str = "obmem-sync";

#[derive(Serialize, Deserialize)]
struct Stored {
    access_key_id: String,
    secret_access_key: String,
    vault_key: String,
}

fn entry(cfg: &SyncConfig) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &location(cfg)).context("opening the OS keychain")
}

pub fn save(cfg: &SyncConfig, creds: &Credentials, key: &VaultKey) -> Result<()> {
    let stored = Stored {
        access_key_id: creds.access_key_id.clone(),
        secret_access_key: creds.secret_access_key.clone(),
        vault_key: key.to_secret().expose_secret().to_string(),
    };
    entry(cfg)?.set_password(&serde_json::to_string(&stored)?).context("saving to the OS keychain")
}

/// Whether this device has what it needs for the configured bucket.
pub fn is_set_up(cfg: &SyncConfig) -> Result<bool> {
    match entry(cfg)?.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(anyhow::Error::from(e).context("reading the OS keychain")),
    }
}

pub fn load(cfg: &SyncConfig) -> Result<(Credentials, VaultKey)> {
    let json = match entry(cfg)?.get_password() {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => {
            return Err(anyhow!("this device is not set up for {}: run `obmem sync setup`", location(cfg)));
        }
        Err(e) => return Err(anyhow::Error::from(e).context("reading the OS keychain")),
    };
    let s: Stored = serde_json::from_str(&json).context("the keychain entry for obmem-sync is damaged; run `obmem sync setup`")?;
    let key = VaultKey::parse(&s.vault_key)?;
    Ok((Credentials { access_key_id: s.access_key_id, secret_access_key: s.secret_access_key }, key))
}
