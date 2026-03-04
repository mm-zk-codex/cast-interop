use crate::types::RelaySummary;
use anyhow::{Context, Result};
use std::io;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A relay event record persisted to the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayRecord {
    /// Unique record ID (UUID v4).
    pub id: String,
    /// Bundle hash (0x-prefixed hex).
    pub bundle_hash: String,
    /// Source transaction hash (0x-prefixed hex).
    pub source_tx_hash: String,
    /// Destination execution transaction hash (None for verify-only or dry-run).
    pub handler_tx_hash: Option<String>,
    /// Source chain ID (decimal string).
    pub source_chain_id: String,
    /// Destination chain ID (decimal string).
    pub destination_chain_id: String,
    /// L1 batch number from the proof.
    pub l1_batch_number: u64,
    /// L2 message index within the batch.
    pub l2_message_index: u64,
    /// Relay mode: "verify" or "execute".
    pub mode: String,
    /// ISO 8601 UTC timestamp when this relay was recorded.
    pub relayed_at: String,
}

impl RelayRecord {
    pub fn from_summary(summary: &RelaySummary, mode: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            bundle_hash: summary.bundle_hash.clone(),
            source_tx_hash: summary.source_tx_hash.clone(),
            handler_tx_hash: summary.handler_tx_hash.clone(),
            source_chain_id: summary.source_chain_id.clone(),
            destination_chain_id: summary.destination_chain_id.clone(),
            l1_batch_number: summary.l1_batch_number,
            l2_message_index: summary.l2_message_index,
            mode: mode.to_string(),
            relayed_at: Utc::now().to_rfc3339(),
        }
    }
}

/// In-memory relay store backed by a JSON file on disk.
///
/// Thread-safe via `Arc<RwLock<RelayStore>>`. The JSON file is written on every
/// mutation so data survives process restarts. Delete or truncate the file to clear.
#[derive(Debug)]
pub struct RelayStore {
    path: PathBuf,
    records: Vec<RelayRecord>,
}

impl RelayStore {
    /// Load from `path`, creating an empty store if the file does not exist.
    pub fn load(path: PathBuf) -> Result<Self> {
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let records: Vec<RelayRecord> = serde_json::from_str(&raw)
                    .with_context(|| format!("failed to parse relay store {}", path.display()))?;
                Ok(Self { path, records })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Ok(Self { path, records: Vec::new() })
            }
            Err(e) => {
                Err(e).with_context(|| format!("failed to read relay store {}", path.display()))
            }
        }
    }

    /// Append a record and flush to disk.
    pub fn push(&mut self, record: RelayRecord) -> Result<()> {
        self.records.push(record);
        self.flush()
    }

    /// Remove all records and flush to disk.
    pub fn clear(&mut self) -> Result<()> {
        self.records.clear();
        self.flush()
    }

    /// All records, newest first.
    pub fn all(&self) -> Vec<&RelayRecord> {
        self.records.iter().rev().collect()
    }

    /// All records for a specific source chain / destination chain pair.
    pub fn by_chain_pair(
        &self,
        src: &str,
        dst: &str,
    ) -> Vec<&RelayRecord> {
        self.records
            .iter()
            .rev()
            .filter(|r| r.source_chain_id == src && r.destination_chain_id == dst)
            .collect()
    }

    /// Find by bundle hash (case-insensitive hex comparison).
    pub fn by_bundle_hash(&self, hash: &str) -> Option<&RelayRecord> {
        let needle = hash.to_lowercase();
        self.records.iter().find(|r| r.bundle_hash.to_lowercase() == needle)
    }

    /// Find by source transaction hash (case-insensitive).
    pub fn by_tx_hash(&self, hash: &str) -> Option<&RelayRecord> {
        let needle = hash.to_lowercase();
        self.records.iter().find(|r| r.source_tx_hash.to_lowercase() == needle)
    }

    fn flush(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.records)?;
        std::fs::write(&self.path, json)
            .with_context(|| format!("failed to write relay store {}", self.path.display()))?;
        Ok(())
    }
}

/// Shared handle used by both the relay command and the HTTP server.
pub type SharedStore = Arc<RwLock<RelayStore>>;

/// Open (or create) the relay store at `path` and wrap it in a shared handle.
pub fn open_store(path: PathBuf) -> Result<SharedStore> {
    let store = RelayStore::load(path)?;
    Ok(Arc::new(RwLock::new(store)))
}

/// Default store file path: `~/.config/cast-interop/relays.json`.
pub fn default_store_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cast-interop")
        .join("relays.json")
}

/// Save a relay summary to the store at the given path (used by the relay command).
pub async fn record_relay(path: &Path, summary: &RelaySummary, mode: &str) -> Result<()> {
    let mut store = RelayStore::load(path.to_path_buf())?;
    let record = RelayRecord::from_summary(summary, mode);
    store.push(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RelaySummary;
    use tempfile::NamedTempFile;

    fn fake_summary(bundle_hash: &str, src_tx: &str) -> RelaySummary {
        RelaySummary {
            source_chain_id: "324".to_string(),
            destination_chain_id: "300".to_string(),
            l1_batch_number: 1,
            l2_message_index: 0,
            bundle_hash: bundle_hash.to_string(),
            source_tx_hash: src_tx.to_string(),
            handler_tx_hash: Some("0xhandler".to_string()),
        }
    }

    #[test]
    fn empty_store_starts_empty() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::remove_file(tmp.path()).unwrap(); // file must not exist
        let store = RelayStore::load(tmp.path().to_path_buf()).unwrap();
        assert!(store.all().is_empty());
    }

    #[test]
    fn push_and_retrieve_by_bundle_hash() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::remove_file(tmp.path()).unwrap();
        let mut store = RelayStore::load(tmp.path().to_path_buf()).unwrap();
        let summary = fake_summary("0xabc", "0xtx1");
        let record = RelayRecord::from_summary(&summary, "execute");
        store.push(record).unwrap();

        let found = store.by_bundle_hash("0xabc");
        assert!(found.is_some());
        assert_eq!(found.unwrap().source_tx_hash, "0xtx1");
    }

    #[test]
    fn push_and_retrieve_by_tx_hash() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::remove_file(tmp.path()).unwrap();
        let mut store = RelayStore::load(tmp.path().to_path_buf()).unwrap();
        let summary = fake_summary("0xbundle", "0xsourcetx");
        store.push(RelayRecord::from_summary(&summary, "verify")).unwrap();

        let found = store.by_tx_hash("0xsourcetx");
        assert!(found.is_some());
        assert_eq!(found.unwrap().bundle_hash, "0xbundle");
    }

    #[test]
    fn bundle_hash_lookup_is_case_insensitive() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::remove_file(tmp.path()).unwrap();
        let mut store = RelayStore::load(tmp.path().to_path_buf()).unwrap();
        let summary = fake_summary("0xABCDEF", "0xtx");
        store.push(RelayRecord::from_summary(&summary, "execute")).unwrap();
        assert!(store.by_bundle_hash("0xabcdef").is_some());
        assert!(store.by_bundle_hash("0xABCDEF").is_some());
    }

    #[test]
    fn clear_removes_all_records() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::remove_file(tmp.path()).unwrap();
        let mut store = RelayStore::load(tmp.path().to_path_buf()).unwrap();
        store.push(RelayRecord::from_summary(&fake_summary("0x1", "0xa"), "execute")).unwrap();
        store.push(RelayRecord::from_summary(&fake_summary("0x2", "0xb"), "execute")).unwrap();
        assert_eq!(store.all().len(), 2);
        store.clear().unwrap();
        assert!(store.all().is_empty());
    }

    #[test]
    fn all_returns_newest_first() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::remove_file(tmp.path()).unwrap();
        let mut store = RelayStore::load(tmp.path().to_path_buf()).unwrap();
        store.push(RelayRecord::from_summary(&fake_summary("0x1", "0xa"), "execute")).unwrap();
        store.push(RelayRecord::from_summary(&fake_summary("0x2", "0xb"), "execute")).unwrap();
        let all = store.all();
        assert_eq!(all[0].bundle_hash, "0x2"); // newest first
        assert_eq!(all[1].bundle_hash, "0x1");
    }

    #[test]
    fn store_persists_across_reload() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::remove_file(tmp.path()).unwrap();
        {
            let mut store = RelayStore::load(tmp.path().to_path_buf()).unwrap();
            store.push(RelayRecord::from_summary(&fake_summary("0xpersist", "0xtx"), "execute")).unwrap();
        }
        // Reload from file
        let store2 = RelayStore::load(tmp.path().to_path_buf()).unwrap();
        assert_eq!(store2.all().len(), 1);
        assert_eq!(store2.all()[0].bundle_hash, "0xpersist");
    }

    #[test]
    fn by_chain_pair_filters_correctly() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::remove_file(tmp.path()).unwrap();
        let mut store = RelayStore::load(tmp.path().to_path_buf()).unwrap();
        store.push(RelayRecord::from_summary(&fake_summary("0x1", "0xa"), "execute")).unwrap();
        // Record with different chains
        let mut other = RelayRecord::from_summary(&fake_summary("0x2", "0xb"), "execute");
        other.source_chain_id = "1".to_string();
        other.destination_chain_id = "10".to_string();
        store.push(other).unwrap();

        let pair = store.by_chain_pair("324", "300");
        assert_eq!(pair.len(), 1);
        assert_eq!(pair[0].bundle_hash, "0x1");
    }

    #[test]
    fn relay_record_mode_is_stored() {
        let summary = fake_summary("0xh", "0xt");
        let r1 = RelayRecord::from_summary(&summary, "verify");
        let r2 = RelayRecord::from_summary(&summary, "execute");
        assert_eq!(r1.mode, "verify");
        assert_eq!(r2.mode, "execute");
    }
}
