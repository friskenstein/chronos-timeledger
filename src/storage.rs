use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;

use crate::domain::{Ledger, LedgerHeader};

const EVENTS_MARKER: &str = "\n=== EVENTS ===\n";

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    TomlDecode(toml::de::Error),
    TomlEncode(toml::ser::Error),
    JsonDecode(serde_json::Error),
    JsonEncode(serde_json::Error),
}

impl Display for StorageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(err) => write!(f, "io error: {err}"),
            StorageError::TomlDecode(err) => write!(f, "failed to parse TOML header: {err}"),
            StorageError::TomlEncode(err) => write!(f, "failed to encode TOML header: {err}"),
            StorageError::JsonDecode(err) => write!(f, "failed to parse JSONL event: {err}"),
            StorageError::JsonEncode(err) => write!(f, "failed to encode JSONL event: {err}"),
        }
    }
}

impl std::error::Error for StorageError {}

pub fn load_ledger(path: &Path) -> Result<Ledger, StorageError> {
    let raw = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Ledger::new()),
        Err(err) => return Err(StorageError::Io(err)),
    };

    if raw.trim().is_empty() {
        return Ok(Ledger::new());
    }

    let (header_blob, events_blob) = if let Some((header, events)) = raw.split_once(EVENTS_MARKER) {
        (header, events)
    } else {
        (raw.as_str(), "")
    };

    let header: LedgerHeader = toml::from_str(header_blob).map_err(StorageError::TomlDecode)?;
    let mut events = Vec::new();
    for line in events_blob.lines() {
        if line.trim().is_empty() {
            continue;
        }
        events.push(serde_json::from_str(line).map_err(StorageError::JsonDecode)?);
    }

    Ok(Ledger { header, events })
}

pub fn save_ledger(path: &Path, ledger: &Ledger) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(StorageError::Io)?;
        }
    }

    let header = toml::to_string_pretty(&ledger.header).map_err(StorageError::TomlEncode)?;
    let mut file = fs::File::create(path).map_err(StorageError::Io)?;
    file.write_all(header.as_bytes())
        .map_err(StorageError::Io)?;
    file.write_all(EVENTS_MARKER.as_bytes())
        .map_err(StorageError::Io)?;

    for event in &ledger.events {
        let line = serde_json::to_string(event).map_err(StorageError::JsonEncode)?;
        file.write_all(line.as_bytes()).map_err(StorageError::Io)?;
        file.write_all(b"\n").map_err(StorageError::Io)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use std::fs;
    use std::path::PathBuf;

    use crate::domain::Ledger;

    use super::{load_ledger, save_ledger};

    #[test]
    fn round_trips_toml_and_jsonl() {
        let mut ledger = Ledger::new();
        let project_id = ledger.add_project("Personal".to_string(), Some("blue".to_string()));
        let task_id = ledger
            .add_task(project_id, None, "Write spec".to_string())
            .expect("task should be created");
        ledger
            .start_task(
                &task_id,
                Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(),
                Some("deep work".to_string()),
            )
            .expect("start should work");
        ledger
            .stop_task(
                &task_id,
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap(),
                None,
            )
            .expect("stop should work");

        let path = temp_file("chronos_storage_roundtrip.ledger");
        save_ledger(&path, &ledger).expect("save should succeed");
        let loaded = load_ledger(&path).expect("load should succeed");
        assert_eq!(loaded.header.projects.len(), 1);
        assert_eq!(loaded.header.tasks.len(), 1);
        assert_eq!(loaded.events.len(), 2);
        let _ = fs::remove_file(path);
    }

    fn temp_file(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("{}_{}", name, std::process::id()));
        path
    }
}
