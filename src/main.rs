use std::path::{Path, PathBuf};

use rust_constness::{
    Result, chart::write_chart, collector::RustDocsCollector, dataset::Dataset, versions,
};

const DEFAULT_DATA_PATH: &str = "data/methods.json";
const DEFAULT_OUTPUT_PATH: &str = "target/rust-constness.html";
const DEFAULT_PARALLEL_DOWNLOADS: usize = 5;

fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(());
    };

    match command {
        "collect" => collect_command(&args[1..]),
        "chart" => chart_command(&args[1..]),
        "generate" => generate_command(&args[1..]),
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{}", usage()).into()),
    }
}

fn collect_command(args: &[String]) -> Result<()> {
    if wants_help(args) {
        print_usage();
        return Ok(());
    }
    let options = parse_options(args, "collect")?;
    let releases = versions_from_options(&options)?;
    let dataset = RustDocsCollector::new()?.collect(&releases, options.parallel_downloads)?;
    dataset.save(Path::new(DEFAULT_DATA_PATH))?;
    println!("saved {} releases to {}", releases.len(), DEFAULT_DATA_PATH);
    Ok(())
}

fn chart_command(args: &[String]) -> Result<()> {
    if wants_help(args) {
        print_usage();
        return Ok(());
    }
    let options = parse_options(args, "chart")?;
    let dataset = Dataset::load(Path::new(DEFAULT_DATA_PATH))?;
    write_chart(&dataset, &options.output_path)?;
    println!("saved chart to {}", options.output_path.display());
    Ok(())
}

fn generate_command(args: &[String]) -> Result<()> {
    if wants_help(args) {
        print_usage();
        return Ok(());
    }
    let options = parse_options(args, "generate")?;
    let releases = versions_from_options(&options)?;
    let dataset = RustDocsCollector::new()?.collect(&releases, options.parallel_downloads)?;
    dataset.save(Path::new(DEFAULT_DATA_PATH))?;
    write_chart(&dataset, &options.output_path)?;
    println!("saved data to {DEFAULT_DATA_PATH}");
    println!("saved chart to {}", options.output_path.display());
    Ok(())
}

#[derive(Debug, Default)]
struct Options {
    version_spec: Option<String>,
    output_path: PathBuf,
    parallel_downloads: usize,
}

fn parse_options(args: &[String], command: &str) -> Result<Options> {
    let mut options = Options {
        output_path: PathBuf::from(DEFAULT_OUTPUT_PATH),
        parallel_downloads: DEFAULT_PARALLEL_DOWNLOADS,
        ..Options::default()
    };
    let mut index = 0;

    while index < args.len() {
        let argument = &args[index];
        match argument.as_str() {
            "--version" => {
                options.version_spec = Some(next_value(args, &mut index, argument)?);
            }
            "--output" => {
                options.output_path = PathBuf::from(next_value(args, &mut index, argument)?);
            }
            "--parallel" => {
                let value = next_value(args, &mut index, argument)?;
                options.parallel_downloads = value.parse().map_err(|_| {
                    format!("invalid value for --parallel: `{value}` (expected a positive integer)")
                })?;
                if options.parallel_downloads == 0 {
                    return Err("--parallel must be greater than zero".into());
                }
            }
            other => {
                return Err(format!("unknown {command} option `{other}`\n\n{}", usage()).into());
            }
        }
        index += 1;
    }

    Ok(options)
}

fn wants_help(args: &[String]) -> bool {
    args.iter()
        .any(|argument| argument == "--help" || argument == "-h")
}

fn next_value(args: &[String], index: &mut usize, flag: &str) -> Result<String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

fn versions_from_options(options: &Options) -> Result<Vec<String>> {
    let specification = options
        .version_spec
        .as_deref()
        .ok_or_else(|| "provide --version VERSION or --version START-END".to_owned())?;
    versions::select(specification)
}

fn usage() -> &'static str {
    "Usage:
  rust-constness collect  --version VERSION_OR_RANGE [--parallel N]
  rust-constness chart    [--output CHART.html]
  rust-constness generate --version VERSION_OR_RANGE [--parallel N] [--output CHART.html]

VERSION_OR_RANGE is an exact release such as 1.97.1 or an inclusive range
such as 1.50-1.97. Collected data is always saved to data/methods.json.
Downloads run in parallel, with 5 downloads by default; use --parallel N to
configure the number of concurrent version retrievals.

The collector downloads only the version-matched rust-docs archive, verifies
its checksum, and caches the extracted HTML under target/rust-docs-cache."
}

fn print_usage() {
    println!("{}", usage());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect()
    }

    #[test]
    fn parses_version_and_output_without_a_data_path() {
        let options = parse_options(
            &arguments(&["--version", "1.50-1.97", "--output", "chart.html"]),
            "generate",
        )
        .unwrap();

        assert_eq!(options.version_spec.as_deref(), Some("1.50-1.97"));
        assert_eq!(options.output_path, PathBuf::from("chart.html"));
        assert_eq!(options.parallel_downloads, 5);
    }

    #[test]
    fn parses_a_custom_parallel_download_count() {
        let options = parse_options(
            &arguments(&["--version", "1.97.1", "--parallel", "8"]),
            "collect",
        )
        .unwrap();

        assert_eq!(options.parallel_downloads, 8);
    }

    #[test]
    fn rejects_invalid_parallel_download_counts() {
        for value in ["0", "not-a-number"] {
            let error = parse_options(
                &arguments(&["--version", "1.97.1", "--parallel", value]),
                "collect",
            )
            .unwrap_err();
            assert!(error.to_string().contains("--parallel"));
        }
    }

    #[test]
    fn rejects_removed_input_flags() {
        for flag in ["--data", "--input", "--releases-file", "--release"] {
            let error = parse_options(&arguments(&[flag, "value"]), "generate").unwrap_err();
            assert!(error.to_string().contains("unknown generate option"));
        }
    }

    #[test]
    fn requires_a_version_for_collection() {
        let error =
            versions_from_options(&parse_options(&[], "collect").expect("empty options are valid"))
                .unwrap_err();

        assert!(error.to_string().contains("provide --version"));
    }
}
