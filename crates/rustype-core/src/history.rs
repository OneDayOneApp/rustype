use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub at_unix: u64,
    pub mode: String,
    pub app: String,
    pub duration_ms: u64,
    pub text: String,
}

impl HistoryEntry {
    pub fn new(mode: &str, app: &str, duration_ms: u64, text: &str) -> Self {
        Self {
            at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            mode: mode.to_string(),
            app: app.to_string(),
            duration_ms,
            text: text.to_string(),
        }
    }
}

pub fn append(path: &Path, entry: &HistoryEntry) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(entry)?)?;
    Ok(())
}

pub fn read_recent(path: &Path, limit: usize) -> std::io::Result<Vec<HistoryEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path)?;
    let mut entries = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
            entries.push(entry);
        }
    }
    let keep = entries.len().saturating_sub(limit);
    Ok(entries.split_off(keep))
}

pub fn last_text(path: &Path) -> Option<String> {
    read_recent(path, 1)
        .ok()
        .and_then(|v| v.into_iter().next())
        .map(|e| e.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_recent() {
        let dir = std::env::temp_dir().join(format!("rustype-hist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.jsonl");
        let _ = std::fs::remove_file(&path);
        for i in 0..3 {
            append(
                &path,
                &HistoryEntry::new("dictation", "Notes", 1000, &format!("line {i}")),
            )
            .unwrap();
        }
        let recent = read_recent(&path, 2).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1].text, "line 2");
        assert_eq!(last_text(&path).as_deref(), Some("line 2"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
