use std::collections::HashMap;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use serde::Deserialize;

const ORDER_DIR: &str = "agent-status-order";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HERDR_PLUGIN_EVENT_JSON is not set")]
    MissingEvent,
    #[error("HERDR_PLUGIN_STATE_DIR is not set")]
    MissingStateDir,
    #[error("invalid Herdr plugin event JSON: {0}")]
    InvalidEvent(#[from] serde_json::Error),
    #[error("invalid pane id in Herdr plugin event: {0}")]
    InvalidPaneId(String),
    #[error("failed to access agent status order state {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Deserialize)]
struct EventEnvelope {
    data: StatusEvent,
}

#[derive(Deserialize)]
struct StatusEvent {
    pane_id: String,
}

pub fn record_status_event() -> Result<(), Error> {
    let raw = std::env::var("HERDR_PLUGIN_EVENT_JSON").map_err(|_| Error::MissingEvent)?;
    let event: EventEnvelope = serde_json::from_str(&raw)?;
    if !valid_pane_id(&event.data.pane_id) {
        return Err(Error::InvalidPaneId(event.data.pane_id));
    }
    let directory = order_dir()?;
    std::fs::create_dir_all(&directory).map_err(|source| Error::Io {
        path: directory.clone(),
        source,
    })?;
    let path = directory.join(event.data.pane_id);
    std::fs::write(&path, []).map_err(|source| Error::Io { path, source })
}

pub fn load_status_order() -> Result<HashMap<String, u128>, Error> {
    let directory = order_dir()?;
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HashMap::new());
        }
        Err(source) => {
            return Err(Error::Io {
                path: directory,
                source,
            });
        }
    };
    let mut order = HashMap::new();
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            path: directory.clone(),
            source,
        })?;
        let Some(pane_id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !valid_pane_id(&pane_id) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|source| Error::Io {
                path: entry.path(),
                source,
            })?;
        let sequence = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        order.insert(pane_id, sequence);
    }
    Ok(order)
}

fn order_dir() -> Result<PathBuf, Error> {
    std::env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .map(|path| path.join(ORDER_DIR))
        .ok_or(Error::MissingStateDir)
}

fn valid_pane_id(pane_id: &str) -> bool {
    !pane_id.is_empty()
        && pane_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_id_validation_rejects_path_components() {
        assert!(valid_pane_id("w7:p2"));
        assert!(!valid_pane_id("../config.toml"));
    }

    #[test]
    fn event_payload_uses_the_changed_pane() {
        let event: EventEnvelope = serde_json::from_str(
            r#"{"event":"pane_agent_status_changed","data":{"type":"pane_agent_status_changed","pane_id":"w7:p2","agent_status":"idle"}}"#,
        )
        .unwrap();
        assert_eq!(event.data.pane_id, "w7:p2");
    }
}
