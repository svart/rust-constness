use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{Result, collector::find_html_root, dataset::compare_releases};

#[derive(Debug, PartialEq, Eq)]
pub struct CacheStatus {
    pub location: PathBuf,
    pub releases: Vec<String>,
    pub size_bytes: u64,
}

impl CacheStatus {
    pub fn read(cache_root: &Path) -> Result<Self> {
        let location = std::path::absolute(cache_root)?;
        if !location.exists() {
            return Ok(Self {
                location,
                releases: Vec::new(),
                size_bytes: 0,
            });
        }

        let mut releases = Vec::new();
        for entry in fs::read_dir(&location)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            // Hidden entries are in-progress downloads and extractions.
            if !name.starts_with('.')
                && entry.file_type()?.is_dir()
                && find_html_root(&entry.path())?.is_some()
            {
                releases.push(name);
            }
        }
        releases.sort_by(|left, right| compare_releases(left, right));

        Ok(Self {
            size_bytes: directory_size(&location)?,
            location,
            releases,
        })
    }
}

fn directory_size(path: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        size += if entry.file_type()?.is_dir() {
            directory_size(&entry.path())?
        } else {
            entry.metadata()?.len()
        };
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_missing_cache_as_empty() {
        let cache_root = std::env::temp_dir().join(format!(
            "rust-constness-missing-cache-{}",
            std::process::id()
        ));

        let status = CacheStatus::read(&cache_root).unwrap();

        assert_eq!(status.location, cache_root);
        assert!(status.releases.is_empty());
        assert_eq!(status.size_bytes, 0);
    }

    #[test]
    fn lists_complete_releases_in_version_order_and_sums_every_file() {
        let cache_root = std::env::temp_dir().join(format!(
            "rust-constness-status-cache-{}",
            std::process::id()
        ));
        for release in ["1.10.0", "1.9.0"] {
            let std_root = cache_root.join(format!("{release}/share/doc/rust/html/std"));
            fs::create_dir_all(&std_root).unwrap();
            fs::write(std_root.join("all.html"), "0123456789").unwrap();
        }
        fs::create_dir_all(cache_root.join("1.5.0/share/doc")).unwrap();
        fs::write(cache_root.join("1.5.0/share/doc/partial"), "abc").unwrap();
        fs::write(cache_root.join(".rust-docs-1.2.0-42.tar.gz"), "abcde").unwrap();

        let status = CacheStatus::read(&cache_root).unwrap();

        assert_eq!(status.releases, ["1.9.0", "1.10.0"]);
        assert_eq!(status.size_bytes, 10 + 10 + 3 + 5);
        fs::remove_dir_all(cache_root).unwrap();
    }
}
