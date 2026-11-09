# obmem-sync

Two-way sync and backup of the personal (global) vault through an S3-compatible bucket, so that macOS and Windows machines share one vault. The project vault is not included, because git already carries it. This crate holds the network code and the async runtime, which obmem-core must not have. Its API is blocking: `Remote` runs `object_store` on a current-thread tokio runtime of its own.

## Bucket layout (`lib.rs`)

Under `[sync] prefix` (default `obmem`):
- `key.age`: the vault key (an age X25519 identity), encrypted with the user's passphrase (age scrypt). There is no recovery key: a forgotten passphrase means the bucket cannot be read. Changing the passphrase only rewraps this file.
- `snapshots/<seq:012>.age`: a `Manifest` (JSON, encrypted to the vault key) mapping each portable file name to its blob id and size, plus the device and time. Every sync that changes something writes one, so every snapshot is a restore point.
- `blobs/<id[..2]>/<id>.age`: file content, encrypted. `id` is an HMAC-SHA256 of the plaintext, using a MAC key derived from the identity. Blobs are written once and never change.

## Sync (`sync.rs`)

- Each device keeps a *base*, the snapshot it last synced with, in `~/.config/obmem/sync/<hash>.json`, never in the vault. A base made under another key, or newer than the bucket's latest snapshot, is ignored, which makes the sync a pure merge.
- `merge` compares base, local and latest snapshot per name. If one side changed, it wins. If both edited, the local version stays and the remote one becomes `<stem>.sync-conflict-<device>-<time>.<ext>` (core's `audit::SYNC_CONFLICT_MARK`, reported by the `sync-conflict` rule). An edit beats a deletion.
- Names this pass does not sync are carried along from the bucket untouched, never downloaded and never deleted: what `files::included` leaves out (community plugins when off), files the scan skipped (symlinks, case collisions), and bucket names that fail `check_portable`. The saved base covers only the names this device synced, plus its old entries for the rest; otherwise a name it only carried would count as deleted here once it starts syncing it.
- Order matters: upload blobs, then create the snapshot, then change local files, then save the base. If the snapshot number is taken, or a file changed between hashing and upload, nothing local has changed yet and the pass starts over.
- Before overwriting or deleting a local file, the sync checks it still has the content it hashed. If not, the file is left alone, and its base entry keeps the old value so the next sync sees both sides' changes.
- A sync that would delete more than half of the vault (and more than 10 files) stops unless `allow_mass_delete` is set. One sync per state file runs at a time: an OS file lock (`File::try_lock`) on a `.lock` file next to it, which the OS releases when the process ends.

## Running a sync (`run.rs`)

- `run` is what front ends call. It loads the credentials from the keychain, syncs the personal vault, and writes the outcome to `<state>.status.json`, which `last_status` reads for `obmem status` and the desktop app. `connect` stores a device's credentials and unlocks the key, or creates it.
- Agent hooks start `obmem sync --background` (the CLI's `spawn_background_sync`) at session start and when a turn ends, if `[sync]` names a bucket and `auto` is not false. Such a sync waits up to `BACKGROUND_WAIT` for a running one, and starts no sooner than `MIN_INTERVAL` (a minute) after the last start, which the `.lock` file's time records. At most one sync waits (a file lock on `.queued`); any other returns `Queued`, because the waiting sync will see its changes. So a burst of agent turns makes one sync, and the last change always gets one. A queued return is not recorded as a status.
- `connect` sets `[sync] device` to `config::new_device_name` (host name plus four random hex digits) when it is unset: two machines with one host name would otherwise write the same session log every day.

## Invariants

- Every write to the bucket is create-only (`PutMode::Create`). Nothing in the bucket is overwritten, so no device can destroy an earlier snapshot. Removing old snapshots and unreferenced blobs (retention) is the only way objects should ever disappear.
- The manifest records its own `seq`. `load_snapshot` rejects a manifest stored under a different number.
- Names in a manifest are vault-relative, `/`-separated and NFC. `files::check_portable` rejects what Windows cannot hold: reserved characters and device names, trailing dots and spaces, `..`. A scan skips such files and reports them. Restore checks every name, and names that collide by case, before it writes anything, because the names come from the bucket. A sync refuses a result with case collisions.
- Downloaded content is checked against its HMAC and written through `obmem_core::store::atomic_write_bytes`. Restore writes only into a missing or empty folder.
- `files::scan` never follows symlinks and leaves out dot-entries, except the settings part of `.obsidian/` (top-level `*.json` other than `workspace*`, `themes/`, `snippets/`; `plugins/` and `community-plugins.json` only with `[sync] community_plugins = true`).
- Secrets live in the OS keychain (`keychain.rs`, the `keyring` crate): one entry per `<endpoint>/<bucket>/<prefix>`, holding the access key pair and the unlocked vault key. `[sync]` in the user's config holds only locations. It is read from the user's config only, never from a repo's `.obmem.toml`.

## Tests

Tests run against `object_store::memory::InMemory` (`remote::in_memory`), with no network and no keychain. `sync::tests::device` gives each simulated machine its own vault and state file on one bucket. The S3 client and the keychain are thin wrappers; check them by hand with `obmem sync setup` / `obmem sync` / `obmem sync restore` against a real bucket.
