//! Retention: which snapshots stay in the bucket, and removing what no kept snapshot needs. This
//! is the only code that deletes objects.
//!
//! Kept: the latest snapshot; every snapshot of the last 7 days; the newest of each day for 30
//! days; the newest of each week for a year. A snapshot's age is its object's time in the bucket,
//! so choosing needs no decryption.
//!
//! A blob goes when no kept snapshot references it and it is older than [`BLOB_GRACE`]. The grace
//! covers a sync running elsewhere: it uploads (and so renews) every blob its new snapshot needs
//! beyond those of the latest snapshot, which this prune keeps.

use crate::key::VaultKey;
use crate::remote::Remote;
use crate::{BLOBS, SNAPSHOTS, load_snapshot, seq_of};
use anyhow::Result;
use chrono::{DateTime, Datelike, TimeDelta, Utc};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// How old an unreferenced blob must be before it is deleted.
pub const BLOB_GRACE: TimeDelta = TimeDelta::hours(24);

#[derive(Debug, Default)]
pub struct PruneReport {
    /// Snapshots that stay.
    pub kept: usize,
    /// Snapshots removed (or, in a dry run, that would be).
    pub deleted_snapshots: Vec<u64>,
    pub deleted_blobs: usize,
    pub freed_bytes: u64,
}

/// Apply the retention rule as of `now`. With `dry_run`, only report.
pub fn prune(
    remote: &Remote,
    key: &VaultKey,
    now: DateTime<Utc>,
    dry_run: bool,
) -> Result<PruneReport> {
    let snapshots: Vec<(u64, DateTime<Utc>, u64, String)> = remote
        .list_meta(SNAPSHOTS)?
        .into_iter()
        .filter_map(|o| {
            let seq = seq_of(&o.key)?;
            Some((seq, o.modified, o.size, o.key))
        })
        .collect();
    if snapshots.is_empty() {
        return Ok(PruneReport::default());
    }
    let keep = retained(
        &snapshots
            .iter()
            .map(|(seq, at, ..)| (*seq, *at))
            .collect::<Vec<_>>(),
        now,
    );

    // Read every kept snapshot before deleting anything: one that cannot be read stops the prune.
    let mut referenced = HashSet::new();
    for &seq in &keep {
        referenced.extend(
            load_snapshot(remote, key, seq)?
                .files
                .into_values()
                .map(|e| e.blob),
        );
    }

    let mut report = PruneReport {
        kept: keep.len(),
        ..Default::default()
    };
    for (seq, _, size, object) in snapshots.iter().filter(|(seq, ..)| !keep.contains(seq)) {
        if !dry_run {
            remote.delete(object)?;
        }
        report.deleted_snapshots.push(*seq);
        report.freed_bytes += size;
    }
    report.deleted_snapshots.sort_unstable();

    for o in remote.list_meta(BLOBS)? {
        // Only names this crate writes: `blobs/<2 hex>/<64 hex>.age`.
        let Some(id) = o
            .key
            .rsplit('/')
            .next()
            .and_then(|f| f.strip_suffix(".age"))
        else {
            continue;
        };
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if referenced.contains(id) || now - o.modified < BLOB_GRACE {
            continue;
        }
        if !dry_run {
            remote.delete(&o.key)?;
        }
        report.deleted_blobs += 1;
        report.freed_bytes += o.size;
    }
    Ok(report)
}

/// The snapshot numbers the rule keeps, from `(seq, time)` pairs.
fn retained(snapshots: &[(u64, DateTime<Utc>)], now: DateTime<Utc>) -> BTreeSet<u64> {
    let mut keep: BTreeSet<u64> = snapshots
        .iter()
        .map(|(seq, _)| *seq)
        .max()
        .into_iter()
        .collect();
    // Newest snapshot per day (age up to 30 days) or per ISO week (up to a year).
    let mut newest: BTreeMap<(u8, i32, u32), u64> = BTreeMap::new();
    for &(seq, at) in snapshots {
        let age = now - at;
        let bucket = if age <= TimeDelta::days(7) {
            keep.insert(seq);
            continue;
        } else if age <= TimeDelta::days(30) {
            (0, at.year(), at.ordinal())
        } else if age <= TimeDelta::days(365) {
            let week = at.iso_week();
            (1, week.year(), week.week())
        } else {
            continue;
        };
        let slot = newest.entry(bucket).or_insert(seq);
        *slot = (*slot).max(seq);
    }
    keep.extend(newest.into_values());
    keep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::{device, put};
    use crate::{blob_key, remote, restore, snapshot_seqs, tempdir};

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn the_retention_rule() {
        let now = at("2026-11-09T12:00:00Z");
        let snaps = [
            (1, at("2025-01-01T00:00:00Z")), // over a year old
            (2, at("2026-01-05T10:00:00Z")), // week 2 of 2026: older of two
            (3, at("2026-01-06T10:00:00Z")), // week 2 of 2026: newer
            (4, at("2026-10-15T08:00:00Z")), // 25 days: older of the day
            (5, at("2026-10-15T20:00:00Z")), // 25 days: newer of the day
            (6, at("2026-10-16T08:00:00Z")), // 24 days: alone on its day
            (7, at("2026-11-05T08:00:00Z")), // within 7 days: all kept
            (8, at("2026-11-05T09:00:00Z")),
        ];
        assert_eq!(
            retained(&snaps, now).into_iter().collect::<Vec<_>>(),
            [3, 5, 6, 7, 8]
        );
        // The latest is kept however old.
        assert_eq!(
            retained(&[(1, at("2020-01-01T00:00:00Z"))], now)
                .into_iter()
                .collect::<Vec<_>>(),
            [1]
        );
    }

    #[test]
    fn prune_drops_old_snapshots_and_their_blobs_and_the_latest_still_restores() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "one");
        put(&mac.vault, "Global/b.md", "stays");
        mac.sync().unwrap();
        put(&mac.vault, "Global/a.md", "two");
        mac.sync().unwrap();
        put(&mac.vault, "Global/a.md", "three");
        mac.sync().unwrap();
        assert_eq!(remote.list("blobs").unwrap().len(), 4);

        // Today nothing is old enough.
        let r = prune(&remote, &mac.key, Utc::now(), false).unwrap();
        assert_eq!(
            (r.kept, r.deleted_snapshots.len(), r.deleted_blobs),
            (3, 0, 0)
        );

        // A dry run a year and more later reports, and changes nothing.
        let later = Utc::now() + TimeDelta::days(400);
        let r = prune(&remote, &mac.key, later, true).unwrap();
        assert_eq!(
            (r.kept, r.deleted_snapshots.clone(), r.deleted_blobs),
            (1, vec![1, 2], 2)
        );
        assert_eq!(snapshot_seqs(&remote).unwrap(), [1, 2, 3]);

        let r = prune(&remote, &mac.key, later, false).unwrap();
        assert_eq!(
            (r.deleted_snapshots.clone(), r.deleted_blobs),
            (vec![1, 2], 2)
        );
        assert_eq!(snapshot_seqs(&remote).unwrap(), [3]);
        assert_eq!(remote.list("blobs").unwrap().len(), 2);
        let target = tempdir().join("restored");
        restore(&remote, &mac.key, None, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("Global/a.md")).unwrap(),
            "three"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("Global/b.md")).unwrap(),
            "stays"
        );
    }

    #[test]
    fn fresh_unreferenced_blobs_survive_and_foreign_objects_are_left_alone() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "one");
        mac.sync().unwrap();
        // A blob a sync elsewhere just uploaded for a snapshot it has not written yet.
        let id = mac.key.blob_id(b"pending");
        remote
            .put(&blob_key(&id), mac.key.encrypt(b"pending").unwrap())
            .unwrap();
        remote.put("blobs/zz/not-ours.txt", b"x".to_vec()).unwrap();

        let r = prune(&remote, &mac.key, Utc::now() + TimeDelta::hours(1), false).unwrap();
        assert_eq!(r.deleted_blobs, 0);
        let r = prune(&remote, &mac.key, Utc::now() + TimeDelta::hours(25), false).unwrap();
        assert_eq!(r.deleted_blobs, 1);
        assert!(remote.get(&blob_key(&id)).unwrap().is_none());
        assert!(remote.get("blobs/zz/not-ours.txt").unwrap().is_some());
    }

    #[test]
    fn a_sync_after_a_prune_brings_back_a_collected_blob_it_needs() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "one");
        mac.sync().unwrap();
        put(&mac.vault, "Global/a.md", "two");
        mac.sync().unwrap();
        prune(&remote, &mac.key, Utc::now() + TimeDelta::days(400), false).unwrap();
        // "one" was only in the deleted snapshot 1; writing it again must upload it again.
        put(&mac.vault, "Global/a.md", "one");
        mac.sync().unwrap();
        let target = tempdir().join("restored");
        restore(&remote, &mac.key, None, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("Global/a.md")).unwrap(),
            "one"
        );
    }
}
