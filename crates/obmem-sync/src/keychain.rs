//! What a device needs to reach its backup: the bucket's access key and the unlocked vault key.
//! They are kept in the OS keychain (macOS Keychain, Windows Credential Manager, Secret Service
//! elsewhere), one entry per bucket location, so the keychain asks for access once.
//!
//! A machine without a keychain, such as a server or a cloud sandbox, takes them from the
//! environment instead ([`ENV_VARS`]). `obmem sync key` prints the vault key for that.

use crate::key::VaultKey;
use crate::remote::{Credentials, location};
use age::secrecy::ExposeSecret;
use anyhow::{Context, Result, anyhow, bail};
use obmem_core::config::SyncConfig;
use serde::{Deserialize, Serialize};

const SERVICE: &str = "obmem-sync";

/// The access key id, the secret access key and the vault key, in this order. When any of them
/// is set, all three must be, and the keychain is not asked.
pub const ENV_VARS: [&str; 3] = [
    "OBMEM_SYNC_ACCESS_KEY_ID",
    "OBMEM_SYNC_SECRET_ACCESS_KEY",
    "OBMEM_SYNC_VAULT_KEY",
];

#[derive(Serialize, Deserialize)]
struct Stored {
    access_key_id: String,
    secret_access_key: String,
    vault_key: String,
}

fn entry(cfg: &SyncConfig) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &location(cfg)).with_context(without_a_keychain)
}

fn without_a_keychain() -> String {
    format!(
        "opening the OS keychain (on a machine without one, set {} instead)",
        ENV_VARS.join(", ")
    )
}

/// The credentials in the environment, read through `var`. `None`: none of [`ENV_VARS`] is set.
fn from_env(var: impl Fn(&str) -> Option<String>) -> Result<Option<(Credentials, VaultKey)>> {
    let values = ENV_VARS.map(|name| var(name).filter(|v| !v.trim().is_empty()));
    let missing: Vec<&str> = ENV_VARS
        .iter()
        .zip(&values)
        .filter(|(_, v)| v.is_none())
        .map(|(name, _)| *name)
        .collect();
    let [
        Some(access_key_id),
        Some(secret_access_key),
        Some(vault_key),
    ] = values
    else {
        if missing.len() == ENV_VARS.len() {
            return Ok(None);
        }
        bail!(
            "{} must be set too: the sync credentials come from the environment as a whole",
            missing.join(" and ")
        );
    };
    let key = VaultKey::parse(&vault_key).context("reading OBMEM_SYNC_VAULT_KEY")?;
    Ok(Some((
        Credentials {
            access_key_id: access_key_id.trim().to_string(),
            secret_access_key: secret_access_key.trim().to_string(),
        },
        key,
    )))
}

fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn save(cfg: &SyncConfig, creds: &Credentials, key: &VaultKey) -> Result<()> {
    let stored = Stored {
        access_key_id: creds.access_key_id.clone(),
        secret_access_key: creds.secret_access_key.clone(),
        vault_key: key.to_secret().expose_secret().to_string(),
    };
    entry(cfg)?
        .set_password(&serde_json::to_string(&stored)?)
        .with_context(without_a_keychain)
}

/// Whether this device has what it needs for the configured bucket.
pub fn is_set_up(cfg: &SyncConfig) -> Result<bool> {
    if from_env(process_env)?.is_some() {
        return Ok(true);
    }
    match entry(cfg)?.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(anyhow::Error::from(e).context("reading the OS keychain")),
    }
}

pub fn load(cfg: &SyncConfig) -> Result<(Credentials, VaultKey)> {
    if let Some(found) = from_env(process_env)? {
        return Ok(found);
    }
    let json = match entry(cfg)?.get_password() {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => {
            return Err(anyhow!(
                "this device is not set up for {}: run `obmem sync setup`",
                location(cfg)
            ));
        }
        Err(e) => return Err(anyhow::Error::from(e).context(without_a_keychain())),
    };
    let s: Stored = serde_json::from_str(&json)
        .context("the keychain entry for obmem-sync is damaged; run `obmem sync setup`")?;
    let key = VaultKey::parse(&s.vault_key)?;
    Ok((
        Credentials {
            access_key_id: s.access_key_id,
            secret_access_key: s.secret_access_key,
        },
        key,
    ))
}

/// This device's vault key as text, for `OBMEM_SYNC_VAULT_KEY` on a machine without a keychain.
/// Whoever holds it and can read the bucket can read the whole backup.
pub fn vault_key_text(cfg: &SyncConfig) -> Result<String> {
    let (_, key) = load(cfg)?;
    Ok(key.to_secret().expose_secret().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_from_the_environment_come_as_a_whole() {
        let key = VaultKey::generate();
        let key_text = key.to_secret().expose_secret().to_string();
        let env = |set: &[&str]| {
            let set: Vec<String> = set.iter().map(|s| s.to_string()).collect();
            let key_text = key_text.clone();
            move |name: &str| {
                set.iter().any(|s| s == name).then(|| match name {
                    "OBMEM_SYNC_VAULT_KEY" => format!("{key_text}\n"),
                    _ => format!("value-of-{name}"),
                })
            }
        };
        assert!(from_env(env(&[])).unwrap().is_none());
        // an empty variable is an unset one
        assert!(from_env(|_| Some("  ".into())).unwrap().is_none());

        let (creds, found) = from_env(env(&ENV_VARS)).unwrap().expect("all three set");
        assert_eq!(creds.access_key_id, "value-of-OBMEM_SYNC_ACCESS_KEY_ID");
        assert_eq!(
            creds.secret_access_key,
            "value-of-OBMEM_SYNC_SECRET_ACCESS_KEY"
        );
        assert_eq!(found.blob_id(b"x"), key.blob_id(b"x"));

        let err = from_env(env(&["OBMEM_SYNC_VAULT_KEY"])).err().unwrap();
        let err = err.to_string();
        assert!(
            err.contains("OBMEM_SYNC_ACCESS_KEY_ID and OBMEM_SYNC_SECRET_ACCESS_KEY"),
            "{err}"
        );
        // a value that is no vault key is named, and never printed
        let err = from_env(|name| Some(format!("value-of-{name}")))
            .err()
            .unwrap();
        let err = format!("{err:#}");
        assert!(err.contains("reading OBMEM_SYNC_VAULT_KEY"), "{err}");
        assert!(!err.contains("value-of-OBMEM_SYNC_VAULT_KEY"), "{err}");
    }
}
