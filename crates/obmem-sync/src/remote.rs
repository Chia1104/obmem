//! The bucket, behind a blocking interface: obmem's front ends are synchronous, so the async
//! object store runs on a runtime of its own.

use anyhow::{Context, Result, anyhow};
use object_store::aws::AmazonS3Builder;
use object_store::path::Path;
use object_store::prefix::PrefixStore;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload};
use obmem_core::config::SyncConfig;
use std::sync::Arc;

/// S3 access key pair, from the keychain.
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}

pub struct Remote {
    store: Arc<dyn ObjectStore>,
    rt: tokio::runtime::Runtime,
}

pub const DEFAULT_PREFIX: &str = "obmem";

impl Remote {
    pub fn s3(cfg: &SyncConfig, creds: &Credentials) -> Result<Self> {
        let bucket = cfg.bucket.as_deref().ok_or_else(|| anyhow!("[sync] has no `bucket`"))?;
        let mut b = AmazonS3Builder::new()
            .with_bucket_name(bucket)
            .with_region(cfg.region.as_deref().unwrap_or("auto"))
            .with_access_key_id(&creds.access_key_id)
            .with_secret_access_key(&creds.secret_access_key);
        if let Some(endpoint) = &cfg.endpoint {
            b = b.with_endpoint(endpoint);
        }
        let s3 = b.build().context("configuring the S3 client")?;
        Self::with_store(Arc::new(PrefixStore::new(s3, prefix(cfg))))
    }

    pub(crate) fn with_store(store: Arc<dyn ObjectStore>) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        Ok(Self { store, rt })
    }

    /// The object's bytes, or `None` when there is none.
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.rt.block_on(async {
            match self.store.get(&Path::from(key)).await {
                Ok(r) => Ok(Some(r.bytes().await?.to_vec())),
                Err(object_store::Error::NotFound { .. }) => Ok(None),
                Err(e) => Err::<_, object_store::Error>(e),
            }
        })
        .with_context(|| format!("reading {key} from the bucket"))
    }

    /// Write only if nothing is there yet. `false`: the object already existed and was left alone.
    pub fn create(&self, key: &str, bytes: Vec<u8>) -> Result<bool> {
        self.rt
            .block_on(async {
                let opts = PutOptions { mode: PutMode::Create, ..Default::default() };
                match self.store.put_opts(&Path::from(key), PutPayload::from(bytes), opts).await {
                    Ok(_) => Ok(true),
                    Err(object_store::Error::AlreadyExists { .. }) => Ok(false),
                    Err(e) => Err(anyhow::Error::from(e)),
                }
            })
            .with_context(|| format!("writing {key} to the bucket"))
    }

    /// The keys under `dir` (a folder name without the trailing slash).
    pub fn list(&self, dir: &str) -> Result<Vec<String>> {
        use futures::TryStreamExt;
        self.rt
            .block_on(async {
                let items: Vec<_> = self.store.list(Some(&Path::from(dir))).try_collect().await?;
                Ok::<_, object_store::Error>(items.into_iter().map(|m| m.location.to_string()).collect())
            })
            .with_context(|| format!("listing {dir}/ in the bucket"))
    }
}

/// `<endpoint>/<bucket>/<prefix>`: a different bucket or prefix is a different backup.
pub fn location(cfg: &SyncConfig) -> String {
    format!(
        "{}/{}/{}",
        cfg.endpoint.as_deref().unwrap_or("aws").trim_end_matches('/'),
        cfg.bucket.as_deref().unwrap_or(""),
        prefix(cfg)
    )
}

pub fn prefix(cfg: &SyncConfig) -> &str {
    cfg.prefix.as_deref().map(|p| p.trim_matches('/')).filter(|p| !p.is_empty()).unwrap_or(DEFAULT_PREFIX)
}

#[cfg(test)]
pub(crate) fn in_memory() -> Remote {
    Remote::with_store(Arc::new(object_store::memory::InMemory::new())).unwrap()
}
