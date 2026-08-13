use std::collections::HashMap;

use serde::Deserialize;

use crate::{Result, versions::release_is_before};

const DIST_SERVER: &str = "https://static.rust-lang.org/dist";
const MAX_SHA256_LENGTH: usize = 64;
const FIRST_MANIFEST_RELEASE: &str = "1.8.0";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DocsArtifact {
    pub(crate) url: String,
    pub(crate) sha256: String,
}

#[derive(Debug, Deserialize)]
struct DistributionManifest {
    pkg: Packages,
}

#[derive(Debug, Deserialize)]
struct Packages {
    #[serde(rename = "rust-docs")]
    rust_docs: RustDocs,
}

#[derive(Debug, Deserialize)]
struct RustDocs {
    target: HashMap<String, TargetArtifact>,
}

#[derive(Debug, Deserialize)]
struct TargetArtifact {
    available: bool,
    url: String,
    hash: String,
}

pub(crate) fn manifest_url(release: &str) -> String {
    format!("{DIST_SERVER}/channel-rust-{release}.toml")
}

pub(crate) fn uses_legacy_docs_metadata(release: &str) -> Result<bool> {
    release_is_before(release, FIRST_MANIFEST_RELEASE)
}

pub(crate) fn legacy_docs_checksum_url(release: &str, host: &str) -> String {
    format!(
        "{}/{}.sha256",
        DIST_SERVER,
        legacy_archive_name(release, host)
    )
}

pub(crate) fn resolve_legacy_docs_artifact(
    release: &str,
    host: &str,
    checksum: &str,
) -> Result<DocsArtifact> {
    let archive_name = legacy_archive_name(release, host);
    let mut fields = checksum.split_whitespace();
    let hash = fields.next().ok_or("legacy rust-docs checksum is empty")?;
    let checksum_name = fields
        .next()
        .map(|name| name.trim_start_matches('*'))
        .ok_or("legacy rust-docs checksum does not name its archive")?;
    if checksum_name != archive_name {
        return Err(format!(
            "legacy rust-docs checksum names `{checksum_name}`, expected `{archive_name}`"
        )
        .into());
    }

    validated_artifact(format!("{DIST_SERVER}/{archive_name}"), hash.to_owned())
}

pub(crate) fn resolve_docs_artifact(manifest: &str, host: &str) -> Result<DocsArtifact> {
    let manifest: DistributionManifest = toml::from_str(manifest)?;
    let artifact = manifest
        .pkg
        .rust_docs
        .target
        .get(host)
        .ok_or_else(|| format!("rust-docs is unavailable for host target `{host}`"))?;

    if !artifact.available {
        return Err(format!("rust-docs is unavailable for host target `{host}`").into());
    }
    validated_artifact(artifact.url.clone(), artifact.hash.clone())
}

fn legacy_archive_name(release: &str, host: &str) -> String {
    format!("rust-docs-{release}-{host}.tar.gz")
}

fn validated_artifact(url: String, sha256: String) -> Result<DocsArtifact> {
    if !url.starts_with("https://static.rust-lang.org/dist/") {
        return Err(format!("rust-docs artifact uses an untrusted URL: {url}").into());
    }
    if sha256.len() != MAX_SHA256_LENGTH
        || !sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(format!("rust-docs artifact has an invalid SHA-256 hash: {}", sha256).into());
    }

    Ok(DocsArtifact { url, sha256 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
        [pkg.rust-docs]
        version = "1.97.1"

        [pkg.rust-docs.target.x86_64-unknown-linux-gnu]
        available = true
        url = "https://static.rust-lang.org/dist/rust-docs.tar.gz"
        hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

        [pkg.rust-docs.target.aarch64-unknown-linux-gnu]
        available = false
        url = "https://static.rust-lang.org/dist/unavailable.tar.gz"
        hash = "deadbeef"
    "#;

    #[test]
    fn resolves_the_host_specific_docs_artifact() {
        assert_eq!(
            resolve_docs_artifact(MANIFEST, "x86_64-unknown-linux-gnu").unwrap(),
            DocsArtifact {
                url: "https://static.rust-lang.org/dist/rust-docs.tar.gz".to_owned(),
                sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_owned(),
            }
        );
    }

    #[test]
    fn rejects_an_unavailable_host_artifact() {
        let error = resolve_docs_artifact(MANIFEST, "aarch64-unknown-linux-gnu")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unavailable"));
    }

    #[test]
    fn rejects_an_artifact_url_outside_the_official_distribution_server() {
        let manifest = MANIFEST.replace(
            "https://static.rust-lang.org/dist/rust-docs.tar.gz",
            "https://example.invalid/rust-docs.tar.gz",
        );
        let error = resolve_docs_artifact(&manifest, "x86_64-unknown-linux-gnu")
            .unwrap_err()
            .to_string();
        assert!(error.contains("untrusted URL"));
    }

    #[test]
    fn rejects_an_artifact_url_with_a_lookalike_distribution_prefix() {
        let manifest = MANIFEST.replace(
            "https://static.rust-lang.org/dist/rust-docs.tar.gz",
            "https://static.rust-lang.org/dist.evil.invalid/rust-docs.tar.gz",
        );
        let error = resolve_docs_artifact(&manifest, "x86_64-unknown-linux-gnu")
            .unwrap_err()
            .to_string();
        assert!(error.contains("untrusted URL"));
    }

    #[test]
    fn builds_the_official_manifest_url() {
        assert_eq!(
            manifest_url("1.97.1"),
            "https://static.rust-lang.org/dist/channel-rust-1.97.1.toml"
        );
    }

    #[test]
    fn resolves_checksummed_docs_before_channel_manifests_existed() {
        let hash = "08d27f9418d66557a3bbeba30654a2e41211015ca247e348b3a92e3a322b667e";
        let artifact = resolve_legacy_docs_artifact(
            "1.0.0",
            "x86_64-unknown-linux-gnu",
            &format!("{hash}  rust-docs-1.0.0-x86_64-unknown-linux-gnu.tar.gz"),
        )
        .unwrap();

        assert_eq!(
            artifact.url,
            "https://static.rust-lang.org/dist/rust-docs-1.0.0-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(artifact.sha256, hash);
        assert_eq!(
            legacy_docs_checksum_url("1.0.0", "x86_64-unknown-linux-gnu"),
            "https://static.rust-lang.org/dist/rust-docs-1.0.0-x86_64-unknown-linux-gnu.tar.gz.sha256"
        );
    }

    #[test]
    fn switches_to_channel_manifests_at_rust_1_8() {
        assert!(uses_legacy_docs_metadata("1.7.0").unwrap());
        assert!(!uses_legacy_docs_metadata("1.8.0").unwrap());
    }

    #[test]
    fn rejects_a_legacy_checksum_for_another_archive() {
        let error = resolve_legacy_docs_artifact(
            "1.0.0",
            "x86_64-unknown-linux-gnu",
            "08d27f9418d66557a3bbeba30654a2e41211015ca247e348b3a92e3a322b667e  other.tar.gz",
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("expected"));
    }
}
