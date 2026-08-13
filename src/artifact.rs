use std::collections::HashMap;

use serde::Deserialize;

use crate::Result;

const DIST_SERVER: &str = "https://static.rust-lang.org/dist";
const MAX_SHA256_LENGTH: usize = 64;

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
    if !artifact
        .url
        .starts_with("https://static.rust-lang.org/dist/")
    {
        return Err(format!("rust-docs artifact uses an untrusted URL: {}", artifact.url).into());
    }
    if artifact.hash.len() != MAX_SHA256_LENGTH
        || !artifact
            .hash
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(format!(
            "rust-docs artifact has an invalid SHA-256 hash: {}",
            artifact.hash
        )
        .into());
    }

    Ok(DocsArtifact {
        url: artifact.url.clone(),
        sha256: artifact.hash.clone(),
    })
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
    fn builds_the_official_manifest_url() {
        assert_eq!(
            manifest_url("1.97.1"),
            "https://static.rust-lang.org/dist/channel-rust-1.97.1.toml"
        );
    }
}
