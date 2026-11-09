//! The vault key: one age X25519 identity. The remote keeps it in `key.age`, encrypted with the
//! passphrase the user chooses; each device keeps it in the OS keychain after the first unlock.
//! Changing the passphrase only rewraps `key.age`, because every blob is encrypted to the key.

use age::secrecy::{ExposeSecret, SecretString};
use anyhow::{Result, anyhow};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

pub struct VaultKey {
    identity: age::x25519::Identity,
    /// Derived from the identity, so the key stays one string that `age -d -i` also accepts.
    mac_key: [u8; 32],
}

impl VaultKey {
    pub fn generate() -> Self {
        Self::from_identity(age::x25519::Identity::generate())
    }

    fn from_identity(identity: age::x25519::Identity) -> Self {
        let mut h = Sha256::new();
        h.update(b"obmem-sync blob id v1\0");
        h.update(identity.to_string().expose_secret().as_bytes());
        Self {
            mac_key: h.finalize().into(),
            identity,
        }
    }

    /// `AGE-SECRET-KEY-1…`: what the keychain stores.
    pub fn to_secret(&self) -> SecretString {
        self.identity.to_string()
    }

    pub fn parse(s: &str) -> Result<Self> {
        let identity = s
            .trim()
            .parse::<age::x25519::Identity>()
            .map_err(|e| anyhow!("not a vault key: {e}"))?;
        Ok(Self::from_identity(identity))
    }

    /// `key.age`: the key encrypted with the passphrase (age scrypt).
    pub fn wrap(&self, passphrase: &SecretString) -> Result<Vec<u8>> {
        let recipient = age::scrypt::Recipient::new(passphrase.clone());
        Ok(age::encrypt(
            &recipient,
            self.identity.to_string().expose_secret().as_bytes(),
        )?)
    }

    pub fn unwrap(wrapped: &[u8], passphrase: &SecretString) -> Result<Self> {
        let identity = age::scrypt::Identity::new(passphrase.clone());
        let plain = age::decrypt(&identity, wrapped).map_err(|e| match e {
            age::DecryptError::DecryptionFailed | age::DecryptError::KeyDecryptionFailed => {
                anyhow!("wrong passphrase for this backup")
            }
            e => anyhow!("reading key.age: {e}"),
        })?;
        Self::parse(
            std::str::from_utf8(&plain)
                .map_err(|_| anyhow!("key.age does not hold a vault key"))?,
        )
    }

    /// A blob's name on the remote: HMAC of its content, so equal files are stored once and the
    /// name tells nobody without the key what is inside.
    pub fn blob_id(&self, content: &[u8]) -> String {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.mac_key)
            .expect("HMAC takes any key length");
        mac.update(content);
        hex::encode(mac.finalize().into_bytes())
    }

    pub fn encrypt(&self, plain: &[u8]) -> Result<Vec<u8>> {
        Ok(age::encrypt(&self.identity.to_public(), plain)?)
    }

    pub fn decrypt(&self, cipher: &[u8]) -> Result<Vec<u8>> {
        Ok(age::decrypt(&self.identity, cipher)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_round_trips_and_rejects_a_wrong_passphrase() {
        let key = VaultKey::generate();
        let wrapped = key
            .wrap(&SecretString::from("correct horse".to_string()))
            .unwrap();
        let back =
            VaultKey::unwrap(&wrapped, &SecretString::from("correct horse".to_string())).unwrap();
        assert_eq!(
            back.to_secret().expose_secret(),
            key.to_secret().expose_secret()
        );
        assert_eq!(back.blob_id(b"x"), key.blob_id(b"x"));
        let err = VaultKey::unwrap(&wrapped, &SecretString::from("wrong".to_string()))
            .err()
            .unwrap();
        assert!(err.to_string().contains("wrong passphrase"), "{err}");
    }

    #[test]
    fn blob_ids_depend_on_the_key() {
        let (a, b) = (VaultKey::generate(), VaultKey::generate());
        assert_eq!(a.blob_id(b"same"), a.blob_id(b"same"));
        assert_ne!(a.blob_id(b"same"), b.blob_id(b"same"));
        assert_ne!(a.blob_id(b"one"), a.blob_id(b"two"));
    }

    #[test]
    fn encrypt_round_trips_and_needs_the_key() {
        let key = VaultKey::generate();
        let cipher = key.encrypt(b"note body").unwrap();
        assert!(!cipher.windows(9).any(|w| w == b"note body"));
        assert_eq!(key.decrypt(&cipher).unwrap(), b"note body");
        assert!(VaultKey::generate().decrypt(&cipher).is_err());
    }
}
