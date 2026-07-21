//! Exercise discovery and manifest loading.
//!
//! Reads `bufflings.toml` from the exercises directory and provides
//! structured access to the exercise list, hints, and file paths.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A single exercise entry in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExerciseEntry {
    /// The exercise name (used as the key in the CLI, e.g. "variables1").
    pub name: String,
    /// Path to the `.buff` exercise file, relative to the exercises root.
    pub path: PathBuf,
    /// Optional hint text shown by `bufflings hint <name>`.
    #[serde(default)]
    pub hint: Option<String>,
}

/// A single topic group containing exercises.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TopicGroup {
    /// The topic name (e.g. "basics", "functions").
    pub name: String,
    /// Exercises in this topic.
    pub exercises: Vec<ExerciseEntry>,
}

/// The full exercise manifest. Deserialized from `bufflings.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct ExerciseManifest {
    /// Topic groups, each containing exercises.
    pub topics: Vec<TopicGroup>,
}

impl ExerciseManifest {
    /// Find an exercise by name across all topics.
    pub fn find_entry(&self, name: &str) -> Option<&ExerciseEntry> {
        for group in &self.topics {
            for entry in &group.exercises {
                if entry.name == name {
                    return Some(entry);
                }
            }
        }
        None
    }

    /// Collect all entries from every topic, in manifest order.
    pub fn all_entries(&self) -> Vec<&ExerciseEntry> {
        self.topics
            .iter()
            .flat_map(|g| g.exercises.iter())
            .collect()
    }

    /// Total number of exercises across all topics.
    pub fn total_count(&self) -> usize {
        self.topics.iter().map(|g| g.exercises.len()).sum()
    }
}

/// Load the exercise manifest from `exercises_dir/bufflings.toml`.
///
/// If the file does not exist, returns an empty manifest (no exercises
/// found). If the file exists but cannot be parsed, returns an error.
pub fn load_manifest(exercises_dir: &Path) -> anyhow::Result<ExerciseManifest> {
    let manifest_path = exercises_dir.join("bufflings.toml");
    if !manifest_path.exists() {
        return Ok(ExerciseManifest::default());
    }
    let content = std::fs::read_to_string(&manifest_path)?;
    let mut manifest: ExerciseManifest = toml::from_str(&content)?;

    // Resolve relative paths against exercises_dir
    for group in manifest.topics.iter_mut() {
        for entry in group.exercises.iter_mut() {
            if entry.path.is_relative() {
                entry.path = exercises_dir.join(&entry.path);
            }
        }
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_manifest_has_zero_total() {
        let m = ExerciseManifest::default();
        assert_eq!(m.total_count(), 0);
        assert!(m.all_entries().is_empty());
        assert!(m.find_entry("nonexistent").is_none());
    }

    #[test]
    fn manifest_finds_entry_by_name() {
        let manifest = ExerciseManifest {
            topics: vec![TopicGroup {
                name: "basics".to_string(),
                exercises: vec![ExerciseEntry {
                    name: "hello1".to_string(),
                    path: PathBuf::from("basics/hello1.buff"),
                    hint: Some("Say hello!".to_string()),
                }],
            }],
        };
        assert!(manifest.find_entry("hello1").is_some());
        assert!(manifest.find_entry("missing").is_none());
        assert_eq!(manifest.total_count(), 1);
    }
}
