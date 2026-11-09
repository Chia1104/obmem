# obmem-sync

Encrypted backup of the personal (global) vault to an S3-compatible bucket, so that a backup made on one device restores on another, macOS or Windows. The project vault is not included, because git already carries it. This crate holds the network code and the async runtime, which obmem-core must not have. Its API is blocking: `Remote` runs `object_store` on a current-thread tokio runtime of its own.

## Bucket layout (`lib.rs`)

Under `[sync] prefix` (default `obmem`):
- `key.age`: the vault key (an age X25519 identity), encrypted with the user's passphrase (age scrypt). There is no recovery key: a forgotten passphrase means the backup cannot be read. Changing the passphrase only rewraps this file.
- `snapshots/<seq:012>.age`: a `Manifest` (JSON, encrypted to the vault key) mapping each portable file name to its blob id and size, plus the device and time.
- `blobs/<id[..2]>/<id>.age`: file content, encrypted. `id` is an HMAC-SHA256 of the plaintext, using a MAC key derived from the identity. Blobs are written once and never change, so a backup uploads only content the latest snapshot lacks.

## Invariants

- Every write to the bucket is create-only (`PutMode::Create`). A snapshot takes the next sequence number, and if another device took it first, the backup retries on top of that snapshot. Nothing in the bucket is ever overwritten, so no device can destroy an earlier restore point.
- The manifest records its own `seq`. `load_snapshot` rejects a manifest stored under a different number.
- Names in a manifest are vault-relative, `/`-separated and NFC. `files::check_portable` rejects what Windows cannot hold: reserved characters and device names, trailing dots and spaces, `..`. Backup skips such files and reports them. Restore checks every name, and names that collide by case, before it writes anything, because the names come from the bucket.
- Restore writes only into a missing or empty folder, through `obmem_core::store::atomic_write_bytes`, and checks each blob's HMAC against the manifest.
- `files::scan` never follows symlinks and leaves out dot-entries, except the settings part of `.obsidian/` (top-level `*.json` other than `workspace*`, `themes/`, `snippets/`; `plugins/` and `community-plugins.json` only with `[sync] community_plugins = true`).
- Secrets live in the OS keychain (`keychain.rs`, the `keyring` crate): one entry per `<endpoint>/<bucket>/<prefix>`, holding the access key pair and the unlocked vault key. `[sync]` in the user's config holds only locations. It is read from the user's config only, never from a repo's `.obmem.toml`.

## Tests

Tests run against `object_store::memory::InMemory` (`remote::in_memory`), with no network and no keychain. The S3 client and the keychain are thin wrappers; check them by hand with `obmem sync setup` / `backup` / `restore` against a real bucket.
