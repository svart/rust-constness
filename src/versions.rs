use crate::Result;

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

// Snapshot of the stable Rust releases through the latest release in the
// official release history: https://raw.githubusercontent.com/rust-lang/rust/master/RELEASES.md
const AVAILABLE_RELEASES: &[&str] = &[
    "1.0.0", "1.1.0", "1.2.0", "1.3.0", "1.4.0", "1.5.0", "1.6.0", "1.7.0", "1.8.0", "1.9.0",
    "1.10.0", "1.11.0", "1.12.0", "1.12.1", "1.13.0", "1.14.0", "1.15.0", "1.15.1", "1.16.0",
    "1.17.0", "1.18.0", "1.19.0", "1.20.0", "1.21.0", "1.22.0", "1.22.1", "1.23.0", "1.24.0",
    "1.24.1", "1.25.0", "1.26.0", "1.26.1", "1.26.2", "1.27.0", "1.27.1", "1.27.2", "1.28.0",
    "1.29.0", "1.29.1", "1.29.2", "1.30.0", "1.30.1", "1.31.0", "1.31.1", "1.32.0", "1.33.0",
    "1.34.0", "1.34.1", "1.34.2", "1.35.0", "1.36.0", "1.37.0", "1.38.0", "1.39.0", "1.40.0",
    "1.41.0", "1.41.1", "1.42.0", "1.43.0", "1.43.1", "1.44.0", "1.44.1", "1.45.0", "1.45.1",
    "1.45.2", "1.46.0", "1.47.0", "1.48.0", "1.49.0", "1.50.0", "1.51.0", "1.52.0", "1.52.1",
    "1.53.0", "1.54.0", "1.55.0", "1.56.0", "1.56.1", "1.57.0", "1.58.0", "1.58.1", "1.59.0",
    "1.60.0", "1.61.0", "1.62.0", "1.62.1", "1.63.0", "1.64.0", "1.65.0", "1.66.0", "1.66.1",
    "1.67.0", "1.67.1", "1.68.0", "1.68.1", "1.68.2", "1.69.0", "1.70.0", "1.71.0", "1.71.1",
    "1.72.0", "1.72.1", "1.73.0", "1.74.0", "1.74.1", "1.75.0", "1.76.0", "1.77.0", "1.77.1",
    "1.77.2", "1.78.0", "1.79.0", "1.80.0", "1.80.1", "1.81.0", "1.82.0", "1.83.0", "1.84.0",
    "1.84.1", "1.85.0", "1.85.1", "1.86.0", "1.87.0", "1.88.0", "1.89.0", "1.90.0", "1.91.0",
    "1.91.1", "1.92.0", "1.93.0", "1.93.1", "1.94.0", "1.94.1", "1.95.0", "1.96.0", "1.96.1",
    "1.97.0", "1.97.1", "1.98.0", "1.99.0",
];

pub fn select(specification: &str) -> Result<Vec<String>> {
    let specification = specification.trim();
    if specification.is_empty() {
        return Err("Rust version specification cannot be empty".into());
    }

    if let Some((start, end)) = specification.split_once('-') {
        if end.contains('-') {
            return Err(format!(
                "invalid Rust version range `{specification}`: expected START-END"
            )
            .into());
        }

        let start = parse_bound(start, false)?;
        let end = parse_bound(end, true)?;
        if start > end {
            return Err(format!(
                "invalid Rust version range `{specification}`: start is after end"
            )
            .into());
        }

        let releases = AVAILABLE_RELEASES
            .iter()
            .copied()
            .filter(|release| {
                let version = parse_release(release).expect("catalogue contains valid releases");
                version >= start && version <= end
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        return Ok(releases);
    }

    let requested = parse_release(specification)
        .map_err(|error| format!("invalid Rust version `{specification}`: {error}"))?;
    if AVAILABLE_RELEASES.iter().any(|release| {
        parse_release(release).expect("catalogue contains valid releases") == requested
            && *release == specification
    }) {
        return Ok(vec![specification.to_owned()]);
    }

    Err(format!("Rust version `{specification}` is not in the allowed list").into())
}

fn parse_bound(value: &str, upper: bool) -> Result<Version> {
    let value = value.trim();
    let mut parts = value.split('.');
    let major = parse_component(parts.next(), value)?;
    let minor = parse_component(parts.next(), value)?;
    let patch = match parts.next() {
        Some(patch) => {
            if parts.next().is_some() {
                return Err(
                    format!("expected MAJOR.MINOR or MAJOR.MINOR.PATCH, got `{value}`").into(),
                );
            }
            Some(
                patch
                    .parse()
                    .map_err(|_| format!("invalid patch number in `{value}`"))?,
            )
        }
        None => None,
    };

    // A bound must name a listed release, or a minor series with at least one
    // listed release, so a range never silently stops short of what was asked.
    let is_allowed = AVAILABLE_RELEASES.iter().any(|release| {
        let release = parse_release(release).expect("catalogue contains valid releases");
        release.major == major
            && release.minor == minor
            && patch.is_none_or(|patch| release.patch == patch)
    });
    if !is_allowed {
        return Err(format!("Rust version `{value}` is not in the allowed list").into());
    }

    Ok(Version {
        major,
        minor,
        patch: patch.unwrap_or(if upper { u64::MAX } else { 0 }),
    })
}

fn parse_release(value: &str) -> Result<Version> {
    let mut parts = value.trim().split('.');
    let major = parse_component(parts.next(), value)?;
    let minor = parse_component(parts.next(), value)?;
    let patch = parse_component(parts.next(), value)?;
    if parts.next().is_some() {
        return Err(format!("expected MAJOR.MINOR.PATCH, got `{value}`").into());
    }

    Ok(Version {
        major,
        minor,
        patch,
    })
}

pub(crate) fn release_is_before(release: &str, cutoff: &str) -> Result<bool> {
    Ok(parse_release(release)? < parse_release(cutoff)?)
}

fn parse_component(component: Option<&str>, value: &str) -> Result<u64> {
    let component = component
        .filter(|component| !component.is_empty())
        .ok_or_else(|| format!("expected numeric version component in `{value}`"))?;
    component
        .parse()
        .map_err(|_| format!("expected numeric version component in `{value}`").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_an_exact_release() {
        assert_eq!(select("1.97.1").unwrap(), ["1.97.1"]);
        assert_eq!(select("1.45.2").unwrap(), ["1.45.2"]);
    }

    #[test]
    fn selects_an_inclusive_minor_range() {
        let releases = select("1.50-1.97").unwrap();

        assert_eq!(releases.first().unwrap(), "1.50.0");
        assert_eq!(releases.last().unwrap(), "1.97.1");
        assert!(releases.iter().any(|release| release == "1.97.0"));
    }

    #[test]
    fn rejects_invalid_and_unsupported_specs() {
        let latest = parse_release(AVAILABLE_RELEASES.last().unwrap()).unwrap();
        let next_release = format!("{}.{}.0", latest.major, latest.minor + 1);

        assert!(select("1.97").is_err());
        assert!(select("1.97.1-1.50").is_err());
        assert!(select(&next_release).is_err());
        assert!(select("1.50-1.49").is_err());
    }

    #[test]
    fn rejects_range_bounds_outside_the_allowed_list() {
        let latest = parse_release(AVAILABLE_RELEASES.last().unwrap()).unwrap();
        let next_minor = format!("{}.{}", latest.major, latest.minor + 1);

        for (specification, version) in [
            (format!("1.0-{next_minor}"), next_minor.as_str()),
            ("0.9-1.50".to_owned(), "0.9"),
            ("1.50.0-1.97.2".to_owned(), "1.97.2"),
        ] {
            assert_eq!(
                select(&specification).unwrap_err().to_string(),
                format!("Rust version `{version}` is not in the allowed list")
            );
        }
    }

    #[test]
    fn rejects_an_exact_release_outside_the_allowed_list() {
        assert_eq!(
            select("1.97.2").unwrap_err().to_string(),
            "Rust version `1.97.2` is not in the allowed list"
        );
    }

    #[test]
    fn selects_a_range_with_patch_qualified_bounds() {
        assert_eq!(select("1.45.1-1.45.2").unwrap(), ["1.45.1", "1.45.2"]);
    }

    #[test]
    fn compares_exact_releases() {
        assert!(release_is_before("1.30.1", "1.31.0").unwrap());
        assert!(!release_is_before("1.31.0", "1.31.0").unwrap());
        assert!(!release_is_before("1.31.1", "1.31.0").unwrap());
    }
}
