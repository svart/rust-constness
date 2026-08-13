use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use flate2::read::GzDecoder;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use tar::Archive;
use ureq::Agent;

use crate::{
    Result,
    artifact::{
        DocsArtifact, legacy_docs_checksum_url, manifest_url, resolve_docs_artifact,
        resolve_legacy_docs_artifact, uses_legacy_docs_metadata,
    },
    dataset::{Dataset, ReleaseObservation},
    html::count_methods_in_html_root,
};

const DEFAULT_CACHE_ROOT: &str = "target/rust-docs-cache";
const MAX_MANIFEST_BYTES: u64 = 10 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RustDocsCollector {
    cache_root: PathBuf,
    host_target: String,
}

impl RustDocsCollector {
    pub fn new() -> Result<Self> {
        Ok(Self {
            cache_root: PathBuf::from(DEFAULT_CACHE_ROOT),
            host_target: current_host_target()?,
        })
    }

    #[cfg(test)]
    fn with_cache_root(cache_root: PathBuf, host_target: &str) -> Self {
        Self {
            cache_root,
            host_target: host_target.to_owned(),
        }
    }

    pub fn collect(&self, releases: &[String], parallel_downloads: usize) -> Result<Dataset> {
        if releases.is_empty() {
            return Err("at least one Rust release is required".into());
        }
        if parallel_downloads == 0 {
            return Err("parallel download count must be greater than zero".into());
        }

        let progress = ProgressBars::new(releases);
        let next_release = AtomicUsize::new(0);
        let worker_count = parallel_downloads.min(releases.len());
        let worker_results = thread::scope(|scope| {
            let next_release = &next_release;
            let progress = &progress;
            let mut workers = Vec::with_capacity(worker_count);
            for _ in 0..worker_count {
                let collector = self.clone();
                workers.push(scope.spawn(move || {
                    let mut observations = Vec::new();
                    let mut first_error = None;

                    loop {
                        let index = next_release.fetch_add(1, Ordering::Relaxed);
                        let Some(release) = releases.get(index) else {
                            break;
                        };

                        let progress_bar = progress.bars[index].clone();
                        match collector.collect_release_with_progress(release, &progress_bar) {
                            Ok(observation) => observations.push(observation),
                            Err(error) => {
                                progress_bar.abandon_with_message(format!("failed: {error}"));
                                if first_error.is_none() {
                                    first_error = Some(error.to_string());
                                }
                            }
                        }
                    }

                    (observations, first_error)
                }));
            }

            workers
                .into_iter()
                .map(|worker| {
                    worker
                        .join()
                        .map_err(|_| std::io::Error::other("collector worker panicked"))
                })
                .collect::<std::result::Result<Vec<_>, _>>()
        })?;

        let mut observations = Vec::with_capacity(releases.len());
        let mut first_error = None;
        for (worker_observations, worker_error) in worker_results {
            observations.extend(worker_observations);
            if first_error.is_none() {
                first_error = worker_error;
            }
        }
        if let Some(error) = first_error {
            return Err(error.into());
        }

        Dataset::new(observations)
    }

    pub fn collect_release(&self, release: &str) -> Result<ReleaseObservation> {
        self.collect_release_with_progress(release, &ProgressBar::hidden())
    }

    fn collect_release_with_progress(
        &self,
        release: &str,
        progress: &ProgressBar,
    ) -> Result<ReleaseObservation> {
        validate_release_name(release)?;
        let html_root = self.ensure_docs(release, progress)?;
        let counts = count_methods_in_html_root(&html_root, release)?;
        progress.finish_with_message("done");

        Ok(ReleaseObservation {
            release: release.to_owned(),
            total_methods: counts.total_methods,
            const_methods: counts.const_methods,
            source: format!("rust-docs-archive:{}", html_root.display()),
        })
    }

    fn ensure_docs(&self, release: &str, progress: &ProgressBar) -> Result<PathBuf> {
        let release_cache = self.cache_root.join(release);
        if let Some(html_root) = find_html_root(&release_cache)? {
            progress.finish_with_message("cached");
            return Ok(html_root);
        }

        fs::create_dir_all(&self.cache_root)?;
        let artifact = if uses_legacy_docs_metadata(release)? {
            progress.set_message("fetching checksum");
            let checksum = download_text(&legacy_docs_checksum_url(release, &self.host_target))?;
            resolve_legacy_docs_artifact(release, &self.host_target, &checksum)?
        } else {
            progress.set_message("fetching manifest");
            let manifest = download_text(&manifest_url(release))?;
            resolve_docs_artifact(&manifest, &self.host_target)?
        };
        let archive_path = self.cache_root.join(format!(
            ".rust-docs-{release}-{}.tar.gz",
            std::process::id()
        ));
        progress.set_message("downloading docs");
        let download_result = download_file(&artifact.url, &archive_path, progress);
        if let Err(error) = download_result {
            let _ = fs::remove_file(&archive_path);
            return Err(error);
        }

        let result = self.extract_archive(release, &archive_path, &artifact);
        let _ = fs::remove_file(&archive_path);
        result
    }

    fn extract_archive(
        &self,
        release: &str,
        archive_path: &Path,
        artifact: &DocsArtifact,
    ) -> Result<PathBuf> {
        verify_sha256(archive_path, &artifact.sha256)?;

        let temporary_dir = self
            .cache_root
            .join(format!(".rust-docs-{release}-extract-{}", unique_suffix()));
        fs::create_dir_all(&temporary_dir)?;

        let extraction_result = (|| {
            extract_std_archive(archive_path, &temporary_dir)?;

            let _html_root = find_html_root(&temporary_dir)?.ok_or_else(|| {
                format!("rust-docs archive for Rust {release} has no std HTML documentation")
            })?;

            let release_cache = self.cache_root.join(release);
            if release_cache.exists() {
                fs::remove_dir_all(&release_cache)?;
            }
            fs::rename(&temporary_dir, &release_cache)?;
            find_html_root(&release_cache)?.ok_or_else(|| {
                format!("cached rust-docs for Rust {release} has no std HTML documentation").into()
            })
        })();

        if extraction_result.is_err() {
            let _ = fs::remove_dir_all(&temporary_dir);
        }
        extraction_result
    }
}

fn current_host_target() -> Result<String> {
    let output = Command::new("rustc").arg("-vV").output()?;
    if !output.status.success() {
        return Err(command_error("rustc -vV", &output).into());
    }

    String::from_utf8(output.stdout)?
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV did not report a host target".into())
}

fn validate_release_name(release: &str) -> Result<()> {
    if release.is_empty()
        || !release
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
    {
        return Err(format!("invalid Rust release name {release}").into());
    }
    Ok(())
}

fn download_text(url: &str) -> Result<String> {
    let agent = http_agent(120);
    let response = agent.get(url).call()?;
    let body = response.into_body();
    Ok(body
        .into_with_config()
        .limit(MAX_MANIFEST_BYTES)
        .read_to_string()?)
}

fn download_file(url: &str, destination: &Path, progress: &ProgressBar) -> Result<()> {
    let agent = http_agent(900);
    let response = agent.get(url).call()?;
    if let Some(content_length) = response.body().content_length() {
        if content_length > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "rust-docs archive is larger than the {} byte limit",
                MAX_ARCHIVE_BYTES
            )
            .into());
        }
        progress.set_length(content_length);
    }

    let body = response.into_body();
    let mut reader = body.into_with_config().limit(MAX_ARCHIVE_BYTES).reader();
    let mut file = File::create(destination)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded_bytes = 0_u64;

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        downloaded_bytes = downloaded_bytes
            .checked_add(bytes_read as u64)
            .ok_or("rust-docs archive size overflow")?;
        if downloaded_bytes > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "rust-docs archive is larger than the {} byte limit",
                MAX_ARCHIVE_BYTES
            )
            .into());
        }
        file.write_all(&buffer[..bytes_read])?;
        progress.inc(bytes_read as u64);
    }
    file.flush()?;
    Ok(())
}

fn http_agent(timeout_seconds: u64) -> Agent {
    Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(timeout_seconds)))
        .timeout_connect(Some(std::time::Duration::from_secs(30)))
        .build()
        .into()
}

fn command_error(command: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!(
        "{command} failed with status {}: {}{}",
        output.status,
        stderr.trim(),
        if stdout.trim().is_empty() {
            String::new()
        } else {
            format!(" {}", stdout.trim())
        }
    )
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let actual = format!("{:x}", hasher.finalize());
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "sha256 mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )
        .into())
    }
}

fn find_html_root(root: &Path) -> Result<Option<PathBuf>> {
    find_html_root_at_depth(root, 0)
}

fn extract_std_archive(archive_path: &Path, destination: &Path) -> Result<()> {
    let archive = File::open(archive_path)?;
    let decoder = GzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    let mut extracted_anything = false;
    let mut extracted_bytes: u64 = 0;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_type = entry.header().entry_type();
        let path = entry.path()?.into_owned();
        let Some(relative) = std_relative_path(&path) else {
            continue;
        };
        if relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(format!("unsafe path in rust-docs archive: {}", path.display()).into());
        }

        let target = destination.join("share/doc/rust/html/std").join(relative);
        if entry_type.is_dir() {
            fs::create_dir_all(target)?;
            continue;
        }
        if !entry_type.is_file() {
            return Err(format!(
                "unsupported entry type in rust-docs archive: {}",
                path.display()
            )
            .into());
        }
        let entry_size = entry.header().size()?;
        extracted_bytes = extracted_bytes
            .checked_add(entry_size)
            .ok_or("rust-docs archive extracted size overflow")?;
        if extracted_bytes > MAX_EXTRACTED_BYTES {
            return Err("rust-docs archive exceeds the extracted size limit".into());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(target)?;
        extracted_anything = true;
    }

    if extracted_anything {
        Ok(())
    } else {
        Err("rust-docs archive does not contain standard-library HTML".into())
    }
}

fn std_relative_path(path: &Path) -> Option<PathBuf> {
    let components = path.components().collect::<Vec<_>>();
    let prefix = ["share", "doc", "rust", "html", "std"];

    components
        .windows(prefix.len())
        .position(|window| {
            window.iter().zip(prefix).all(|(component, expected)| {
                matches!(component, Component::Normal(name) if name.to_str() == Some(expected))
            })
        })
        .map(|index| {
            components[index + prefix.len()..]
                .iter()
                .fold(PathBuf::new(), |mut path, component| {
                    path.push(component.as_os_str());
                    path
                })
        })
}

fn find_html_root_at_depth(root: &Path, depth: usize) -> Result<Option<PathBuf>> {
    let std_root = root.join("share/doc/rust/html/std");
    if std_root.join("all.html").is_file() || std_root.join("index.html").is_file() {
        return Ok(Some(root.join("share/doc/rust/html")));
    }

    if depth >= 4 || !root.is_dir() {
        return Ok(None);
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Some(candidate) = find_html_root_at_depth(&entry.path(), depth + 1)?
        {
            return Ok(Some(candidate));
        }
    }

    Ok(None)
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

struct ProgressBars {
    _multi: MultiProgress,
    bars: Vec<ProgressBar>,
}

impl ProgressBars {
    fn new(releases: &[String]) -> Self {
        let multi = MultiProgress::new();
        let style = ProgressStyle::with_template(
            "{prefix:.bold.dim} {spinner:.green} [{bar:30.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} {eta} {msg}",
        )
        .expect("progress bar template is valid")
        .progress_chars("=>-");
        let bars = releases
            .iter()
            .map(|release| {
                let progress = multi.add(ProgressBar::new(0));
                progress.set_style(style.clone());
                progress.set_prefix(format!("Rust {release}"));
                progress.set_message("waiting");
                progress.enable_steady_tick(std::time::Duration::from_millis(100));
                progress
            })
            .collect();

        Self {
            _multi: multi,
            bars,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use tar::Builder;

    #[test]
    fn reads_documentation_from_an_existing_cache_without_installing_a_toolchain() {
        let cache_root =
            std::env::temp_dir().join(format!("rust-constness-doc-cache-{}", std::process::id()));
        let html_root = cache_root.join(
            "1.97.1/rust-docs-1.97.1-x86_64-unknown-linux-gnu/rust-docs/share/doc/rust/html/std",
        );
        fs::create_dir_all(&html_root).unwrap();
        fs::write(html_root.join("all.html"), "").unwrap();

        let collector = RustDocsCollector::with_cache_root(cache_root.clone(), "test-host");
        let observation = collector.collect_release("1.97.1").unwrap();

        assert_eq!(observation.total_methods, 0);
        assert!(observation.source.contains("rust-docs-archive:"));
        fs::remove_dir_all(cache_root).unwrap();
    }

    #[test]
    fn rejects_path_like_release_names_before_network_access() {
        let collector = RustDocsCollector::with_cache_root(
            std::env::temp_dir().join("rust-constness-unused-cache"),
            "test-host",
        );

        let error = collector
            .collect_release("../1.97.1")
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid Rust release name"));
    }

    #[test]
    fn collects_cached_releases_with_a_bounded_worker_count() {
        let cache_root = std::env::temp_dir().join(format!(
            "rust-constness-parallel-cache-{}",
            std::process::id()
        ));
        for release in ["1.96.1", "1.97.1"] {
            let html_root = cache_root.join(format!("{release}/rust-docs/share/doc/rust/html/std"));
            fs::create_dir_all(&html_root).unwrap();
            fs::write(html_root.join("all.html"), "").unwrap();
        }

        let collector = RustDocsCollector::with_cache_root(cache_root.clone(), "test-host");
        let releases = vec!["1.96.1".to_owned(), "1.97.1".to_owned()];
        let dataset = collector.collect(&releases, 2).unwrap();

        assert_eq!(dataset.observations.len(), 2);
        assert_eq!(dataset.observations[0].release, "1.96.1");
        assert_eq!(dataset.observations[1].release, "1.97.1");
        fs::remove_dir_all(cache_root).unwrap();
    }

    #[test]
    fn rejects_zero_parallel_downloads() {
        let collector = RustDocsCollector::with_cache_root(
            std::env::temp_dir().join("rust-constness-unused-cache"),
            "test-host",
        );
        let error = collector
            .collect(&["1.97.1".to_owned()], 0)
            .unwrap_err()
            .to_string();

        assert!(error.contains("parallel download count"));
    }

    #[test]
    fn extracts_only_standard_library_html_from_the_archive() {
        let root = std::env::temp_dir().join(format!(
            "rust-constness-archive-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("rust-docs.tar.gz");
        let archive_file = File::create(&archive_path).unwrap();
        let encoder = GzEncoder::new(archive_file, Compression::default());
        let mut builder = Builder::new(encoder);

        append_archive_file(
            &mut builder,
            "rust-docs-1.0.0/rust-docs/share/doc/rust/html/std/index.html",
            "std",
        );
        append_archive_file(
            &mut builder,
            "rust-docs-1.97.1/rust-docs/share/doc/rust/html/book/index.html",
            "book",
        );
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();

        let mut hasher = Sha256::new();
        let mut archive = File::open(&archive_path).unwrap();
        let mut buffer = [0_u8; 4096];
        loop {
            let bytes_read = archive.read(&mut buffer).unwrap();
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let collector = RustDocsCollector::with_cache_root(root.clone(), "test-host");
        let artifact = DocsArtifact {
            url: String::new(),
            sha256: format!("{:x}", hasher.finalize()),
        };
        let html_root = collector
            .extract_archive("1.97.1", &archive_path, &artifact)
            .unwrap();

        assert!(html_root.join("std/index.html").is_file());
        assert!(
            !root
                .join("1.97.1/share/doc/rust/html/book/index.html")
                .exists()
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn append_archive_file(builder: &mut Builder<GzEncoder<File>>, path: &str, contents: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, contents.as_bytes())
            .unwrap();
    }
}
