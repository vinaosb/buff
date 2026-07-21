//! Progress persistence to `~/.bufflings/progress.toml`.
//!
//! Tracks which exercises the user has completed and when they were last
//! verified. The file is a simple TOML table: exercise name → done + timestamp.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::exercise::ExerciseManifest;

/// A single progress entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressEntry {
    /// Whether the exercise has been marked as done.
    pub done: bool,
    /// ISO 8601 timestamp of last verification (or empty string).
    #[serde(default)]
    pub last_verified: String,
}

/// The full progress store. Maps exercise name → progress entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProgressStore {
    /// Exercise name → entry.
    #[serde(flatten)]
    pub entries: BTreeMap<String, ProgressEntry>,
}

/// The directory where progress is stored: `~/.bufflings/`.
const PROGRESS_DIR_NAME: &str = ".bufflings";

/// The progress file name.
const PROGRESS_FILE_NAME: &str = "progress.toml";

impl ProgressStore {
    /// Load progress from `~/.bufflings/progress.toml`.
    ///
    /// Returns an empty store if the file does not exist or cannot be
    /// read (silent degradation).
    pub fn load() -> anyhow::Result<Self> {
        let path = progress_file_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Ok(Self::default()),
        };
        let store: ProgressStore = toml::from_str(&content).unwrap_or_default();
        Ok(store)
    }

    /// Persist progress to `~/.bufflings/progress.toml`.
    ///
    /// Creates the directory if it does not exist. Errors are silently
    /// ignored (progress is best-effort).
    pub fn save(&self) -> anyhow::Result<()> {
        let path = progress_file_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Mark an exercise as done with the current timestamp.
    pub fn mark_done(&mut self, name: &str) {
        let now = chrono_now_iso();
        self.entries.insert(
            name.to_string(),
            ProgressEntry {
                done: true,
                last_verified: now,
            },
        );
    }

    /// Check whether an exercise is marked as done.
    pub fn is_done(&self, name: &str) -> bool {
        self.entries.get(name).map(|e| e.done).unwrap_or(false)
    }

    /// Count how many exercises in the manifest are marked done.
    pub fn count_done(&self, manifest: &ExerciseManifest) -> usize {
        manifest
            .all_entries()
            .iter()
            .filter(|e| self.is_done(&e.name))
            .count()
    }
}

/// Resolve the progress file path: `~/.bufflings/progress.toml`.
fn progress_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(PROGRESS_DIR_NAME)
        .join(PROGRESS_FILE_NAME)
}

/// Get the current time as an ISO 8601 string.
///
/// Uses `std::time::SystemTime` to avoid depending on `chrono` at
/// runtime. The format is `YYYY-MM-DDTHH:MM:SSZ` (UTC).
fn chrono_now_iso() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Simple UTC calculation (no timezone dependency)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01 → year/month/day (simplified, good enough)
    let (year, month, day) = days_to_date(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to a (year, month, day) tuple.
/// Simplified algorithm for the common case.
fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    let days = days_since_epoch + 719468;
    let year_val = ((days * 400 + 3) / 146097) * 100;
    let day_of_year = ((days * 400 + 3) % 146097) / 4;
    let (month_val, day_of_month) = day_of_year_to_month_day(day_of_year);
    (year_val, month_val, day_of_month)
}

/// Convert day-of-year (0-based) to (month, day-of-month).
fn day_of_year_to_month_day(doy: u64) -> (u64, u64) {
    let leap = (59..91).contains(&doy) || doy >= 306;
    let march_doy = if leap {
        if doy >= 306 {
            doy - 306
        } else {
            doy + 60
        }
    } else if doy >= 60 {
        doy + 1
    } else {
        doy
    };

    // Days in each month starting from March (simplified)
    let days_per_month: &[u64] = &[31, 30, 31, 30, 31, 31, 30, 31, 30, 31, 30, 31];
    let mut remaining = march_doy;
    let mut month_idx = 0u64;
    for (i, &d) in days_per_month.iter().enumerate() {
        if remaining < d {
            month_idx = i as u64;
            break;
        }
        remaining -= d;
        month_idx = (i + 1) as u64;
        if i == days_per_month.len() - 1 {
            remaining = 0;
        }
    }

    // Map March=0 back to January=1
    let month = (month_idx + 2) % 12 + 1;
    let day = remaining + 1;
    (month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exercise::ExerciseEntry;

    #[test]
    fn progress_store_round_trip() {
        let mut store = ProgressStore::default();
        store.mark_done("hello1");
        let toml_str = toml::to_string(&store).expect("serialize");
        let back: ProgressStore = toml::from_str(&toml_str).expect("deserialize");
        assert!(back.is_done("hello1"));
        assert!(!back.is_done("other"));
    }

    #[test]
    fn progress_store_empty_serializes_cleanly() {
        let store = ProgressStore::default();
        let toml_str = toml::to_string(&store).expect("serialize");
        let back: ProgressStore = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(store, back);
    }

    #[test]
    fn mark_done_overwrites_previous() {
        let mut store = ProgressStore::default();
        store.mark_done("ex1");
        let first_ts = store.entries.get("ex1").unwrap().last_verified.clone();
        store.mark_done("ex1");
        let second_ts = store.entries.get("ex1").unwrap().last_verified.clone();
        assert_eq!(first_ts, second_ts);
    }

    #[test]
    fn count_done_with_manifest() {
        let manifest = ExerciseManifest {
            topics: vec![crate::exercise::TopicGroup {
                name: "basics".to_string(),
                exercises: vec![
                    ExerciseEntry {
                        name: "a".to_string(),
                        path: PathBuf::from("a.buff"),
                        hint: None,
                    },
                    ExerciseEntry {
                        name: "b".to_string(),
                        path: PathBuf::from("b.buff"),
                        hint: None,
                    },
                ],
            }],
        };
        let mut store = ProgressStore::default();
        assert_eq!(store.count_done(&manifest), 0);
        store.mark_done("a");
        assert_eq!(store.count_done(&manifest), 1);
        store.mark_done("b");
        assert_eq!(store.count_done(&manifest), 2);
    }

    #[test]
    fn chrono_now_iso_produces_non_empty_string() {
        let s = chrono_now_iso();
        assert!(!s.is_empty());
        assert!(s.contains('T'));
        assert!(s.ends_with('Z'));
    }
}
