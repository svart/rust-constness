use std::{cmp::Ordering, fs::File, path::Path};

use serde::{Deserialize, Serialize};

use crate::Result;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseObservation {
    pub release: String,
    pub total_methods: usize,
    pub const_methods: usize,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dataset {
    pub schema_version: u32,
    pub observations: Vec<ReleaseObservation>,
}

impl Dataset {
    pub fn new(mut observations: Vec<ReleaseObservation>) -> Result<Self> {
        if observations.is_empty() {
            return Err("the dataset must contain at least one release".into());
        }

        observations.sort_by(|left, right| compare_releases(&left.release, &right.release));

        for pair in observations.windows(2) {
            if pair[0].release == pair[1].release {
                return Err(format!("duplicate Rust release: {}", pair[0].release).into());
            }
        }

        for observation in &observations {
            if observation.const_methods > observation.total_methods {
                return Err(format!(
                    "const method count exceeds total for Rust {}",
                    observation.release
                )
                .into());
            }
        }

        Ok(Self {
            schema_version: SCHEMA_VERSION,
            observations,
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let dataset: Self = serde_json::from_reader(file)?;
        if dataset.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported dataset schema version: {}",
                dataset.schema_version
            )
            .into());
        }

        Self::new(dataset.observations)
    }
}

pub(crate) fn compare_releases(left: &str, right: &str) -> Ordering {
    match (release_key(left), release_key(right)) {
        (Some(left_key), Some(right_key)) => left_key.cmp(&right_key).then_with(|| left.cmp(right)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.cmp(right),
    }
}

fn release_key(release: &str) -> Option<(u64, u64, u64)> {
    let mut parts = release.trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.split('-').next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(
        release: &str,
        total_methods: usize,
        const_methods: usize,
    ) -> ReleaseObservation {
        ReleaseObservation {
            release: release.to_owned(),
            total_methods,
            const_methods,
            source: "fixture".to_owned(),
        }
    }

    #[test]
    fn sorts_releases_numerically_and_rejects_invalid_counts() {
        let dataset = Dataset::new(vec![
            observation("1.10.0", 4, 2),
            observation("1.9.0", 3, 1),
        ])
        .unwrap();

        assert_eq!(dataset.observations[0].release, "1.9.0");
        assert!(Dataset::new(vec![observation("1.0.0", 1, 2)]).is_err());
    }
}
