use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::json;
use tomlsmith_benchmark::{
    BenchmarkSettings, EnvironmentReport, FixtureCorpus, ProductId, ProductOperation,
    ProductRunner, TIME_BINARY_ENV, built_in_catalog, check_generated_corpus, generate_corpus,
    product_catalog, product_statuses, verify_corpus_with_product_filter,
};

#[derive(Debug, Parser)]
#[command(
    name = "tomlsmith-benchmark",
    version,
    about = "End-to-end benchmark harness for TOML products"
)]
struct Cli {
    #[arg(long, global = true, default_value = ".", value_name = "PATH")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List adapters, operation contracts, and corpus fixtures.
    List {
        /// Emit the stable JSON report schema.
        #[arg(long)]
        json: bool,
    },
    /// Verify corpus integrity and every supported adapter expectation.
    Verify {
        /// Emit the stable JSON report schema.
        #[arg(long)]
        json: bool,
    },
    /// Capture machine, toolchain, checkout, corpus, and benchmark configuration metadata.
    Env {
        /// Emit the stable JSON report schema.
        #[arg(long)]
        json: bool,
    },
    /// Generate the deterministic fixture corpus, or check it without writing.
    Generate {
        /// Verify checked-in fixtures against the generator without modifying files.
        #[arg(long)]
        check: bool,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Measure peak resident memory in fresh product processes, separately from latency timing.
    PeakRss {
        /// Valid fixture id from `list --json`.
        #[arg(long)]
        fixture: String,
        /// Product operation to measure.
        #[arg(long, value_enum)]
        operation: CliProductOperation,
        /// Number of fresh product processes sampled per case.
        #[arg(long, default_value_t = 3)]
        samples: usize,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliProductOperation {
    Check,
    Format,
}

impl From<CliProductOperation> for ProductOperation {
    fn from(operation: CliProductOperation) -> Self {
        match operation {
            CliProductOperation::Check => Self::Check,
            CliProductOperation::Format => Self::Format,
        }
    }
}

#[derive(Debug, Serialize)]
struct PeakRssCase {
    product_id: ProductId,
    peak_rss_bytes: Vec<u64>,
    median_peak_rss_bytes: u64,
    max_peak_rss_bytes: u64,
}

#[derive(Debug, Serialize)]
struct PeakRssReport {
    backend: &'static str,
    helper: PathBuf,
    fixture_id: String,
    toml_version: tomlsmith_benchmark::TomlVersion,
    operation: ProductOperation,
    input_bytes: usize,
    samples: usize,
    cases: Vec<PeakRssCase>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: &Cli) -> Result<bool, Box<dyn std::error::Error>> {
    match &cli.command {
        Command::List { json } => {
            list(&cli.root, *json)?;
            Ok(true)
        }
        Command::Verify { json } => verify(&cli.root, *json),
        Command::Env { json } => {
            environment(&cli.root, *json)?;
            Ok(true)
        }
        Command::Generate { check, json } => {
            generate(&cli.root, *check, *json)?;
            Ok(true)
        }
        Command::PeakRss { fixture, operation, samples, json } => {
            peak_rss(&cli.root, fixture, (*operation).into(), *samples, *json)?;
            Ok(true)
        }
    }
}

fn peak_rss(
    root: &std::path::Path,
    fixture_id: &str,
    operation: ProductOperation,
    samples: usize,
    as_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if samples == 0 {
        return Err("peak RSS samples must be at least one".into());
    }
    let corpus = FixtureCorpus::load(root)?;
    let fixture = corpus
        .fixtures()
        .iter()
        .find(|fixture| fixture.id == fixture_id)
        .ok_or_else(|| format!("unknown fixture id: {fixture_id}"))?;
    if !fixture.expected_valid {
        return Err(format!("peak RSS requires a valid fixture: {fixture_id}").into());
    }
    let group_id =
        format!("e2e/{}/cold-stdin/{}/{}", operation.as_str(), fixture.toml_version, fixture.id);
    if !BenchmarkSettings::from_env()?.includes(&group_id) {
        return Err(format!("benchmark filter excludes {group_id}").into());
    }

    let isolation = tempfile::tempdir()?;
    let mut cases = Vec::new();
    for descriptor in product_catalog().iter().filter(|descriptor| {
        descriptor.operations.contains(&operation)
            && descriptor.supports_version(fixture.toml_version)
    }) {
        let Some(runner) = ProductRunner::from_env(descriptor.id)? else {
            continue;
        };
        let working_directory = runner.prepare_isolation(isolation.path(), fixture.toml_version)?;
        runner.run_prepared_bounded(
            operation,
            fixture.toml_version,
            fixture.source(),
            &working_directory,
        )?;
        let mut peak_rss_bytes = Vec::with_capacity(samples);
        for _ in 0..samples {
            peak_rss_bytes.push(
                runner
                    .run_prepared_with_peak_rss(
                        operation,
                        fixture.toml_version,
                        fixture.source(),
                        &working_directory,
                    )?
                    .peak_rss_bytes,
            );
        }
        let mut sorted = peak_rss_bytes.clone();
        sorted.sort_unstable();
        let median_peak_rss_bytes = if sorted.len() % 2 == 0 {
            u64::midpoint(sorted[sorted.len() / 2 - 1], sorted[sorted.len() / 2])
        } else {
            sorted[sorted.len() / 2]
        };
        let max_peak_rss_bytes = sorted.last().copied().expect("samples is at least one");
        cases.push(PeakRssCase {
            product_id: descriptor.id,
            peak_rss_bytes,
            median_peak_rss_bytes,
            max_peak_rss_bytes,
        });
    }
    if cases.is_empty() {
        return Err("no enabled product supports the selected operation and TOML version".into());
    }

    let report = PeakRssReport {
        backend: peak_rss_backend(),
        helper: std::env::var_os(TIME_BINARY_ENV)
            .map_or_else(|| PathBuf::from("/usr/bin/time"), PathBuf::from),
        fixture_id: fixture.id.clone(),
        toml_version: fixture.toml_version,
        operation,
        input_bytes: fixture.bytes,
        samples,
        cases,
    };
    if as_json {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
        println!();
    } else {
        println!(
            "peak RSS: {} {} {} ({} bytes)",
            report.operation.as_str(),
            report.toml_version,
            report.fixture_id,
            report.input_bytes
        );
        for case in &report.cases {
            println!(
                "  {}: median {} bytes, max {} bytes",
                case.product_id.as_str(),
                case.median_peak_rss_bytes,
                case.max_peak_rss_bytes
            );
        }
    }
    Ok(())
}

const fn peak_rss_backend() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos_bsd_time"
    } else if cfg!(target_os = "linux") {
        "linux_gnu_time"
    } else {
        "unsupported"
    }
}

fn generate(
    root: &std::path::Path,
    check: bool,
    as_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = if check { check_generated_corpus(root)? } else { generate_corpus(root)? };
    if as_json {
        if check {
            let output = json!({ "matches": true, "fixture_count": manifest.fixtures.len() });
            serde_json::to_writer_pretty(std::io::stdout().lock(), &output)?;
        } else {
            serde_json::to_writer_pretty(std::io::stdout().lock(), &manifest)?;
        }
        println!();
    } else if check {
        println!("corpus matches deterministic generator");
    } else {
        println!("generated {} fixtures", manifest.fixtures.len());
    }
    Ok(())
}

fn environment(root: &std::path::Path, as_json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let report = EnvironmentReport::capture(root)?;
    if as_json {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
        println!();
    } else {
        println!("platform: {} {}", report.os, report.arch);
        println!("cpu: {}", report.cpu_model.as_deref().unwrap_or("unknown"));
        println!("logical CPUs: {}", report.logical_cpus);
        println!("rustc: {}", report.rustc.version_verbose.lines().next().unwrap_or("unknown"));
        println!("cargo: {}", report.cargo.version_verbose.lines().next().unwrap_or("unknown"));
        for (name, runtime) in
            [("Go", report.runtimes.go.as_ref()), ("Node.js", report.runtimes.node.as_ref())]
        {
            if let Some(runtime) = runtime {
                println!("{name}: {}", runtime.version_verbose.lines().next().unwrap_or("unknown"));
            }
        }
        println!("corpus manifest SHA-256: {}", report.corpus_manifest_sha256);
    }
    Ok(())
}

fn verify(root: &std::path::Path, as_json: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let corpus = FixtureCorpus::load(root)?;
    let settings = BenchmarkSettings::from_env()?;
    let report =
        verify_corpus_with_product_filter(&corpus, &built_in_catalog(), settings.filter.as_deref());
    if as_json {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
        println!();
    } else if report.passed {
        println!(
            "verified {} fixtures across {} comparable adapter cases",
            report.fixture_count,
            report.cases.len()
        );
    } else {
        eprintln!("verification failed:");
        for failure in &report.failures {
            eprintln!("  - {failure}");
        }
    }
    Ok(report.passed)
}

fn list(root: &std::path::Path, as_json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let corpus = FixtureCorpus::load(root)?;
    let adapters = built_in_catalog();
    if as_json {
        let descriptors = adapters.iter().map(|adapter| adapter.descriptor()).collect::<Vec<_>>();
        let report = json!({
            "library_operations": [
                {
                    "id": "document_pipeline",
                    "contract": "parse source into a source-backed syntax or editable document; included work is adapter-specific and disclosed"
                },
                {
                    "id": "semantic_decode",
                    "contract": "parse source and materialize a queryable semantic value or DOM, including semantic validation when the API exposes it"
                },
                {
                    "id": "format_pipeline",
                    "contract": "parse valid source and apply the implementation's source formatter policy; formatted bytes need not match across implementations"
                }
            ],
            "product_operations": [
                {
                    "id": "check",
                    "contract": "one cold product process validates one TOML document through stdin and drains stdout/stderr"
                },
                {
                    "id": "format",
                    "contract": "one cold product process formats one TOML document from stdin to stdout and drains stderr"
                }
            ],
            "adapters": descriptors,
            "products": product_statuses(),
            "corpus": corpus.manifest(),
            "fixtures": corpus.manifest().fixtures,
        });
        serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
        println!();
    } else {
        println!("Adapters:");
        for adapter in &adapters {
            let descriptor = adapter.descriptor();
            let operations = descriptor
                .seams
                .iter()
                .map(|seam| seam.operation.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            println!("  {:<10} {}", descriptor.id, operations);
        }
        println!("Products:");
        for product in product_statuses() {
            println!("  {:<12} {:?}", product.descriptor.id.as_str(), product.availability);
        }
        println!("Fixtures:");
        for fixture in corpus.fixtures() {
            println!(
                "  TOML {} {:<14} {:>8} bytes  {}",
                fixture.toml_version,
                fixture.id,
                fixture.bytes,
                fixture.relative_path.display()
            );
        }
    }
    Ok(())
}
