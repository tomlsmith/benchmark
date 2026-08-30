use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::external_process::{
    DEFAULT_PROCESS_TIMEOUT_SECONDS, GO_BINARY_ENV, OptionalToolAvailability, PROCESS_TIMEOUT_ENV,
    ProductId, ProductStatus, product_statuses,
};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BenchmarkSettings {
    pub warm_up_seconds: f64,
    pub measurement_seconds: f64,
    pub sample_size: usize,
    pub process_timeout_seconds: u64,
    pub result_root: String,
    pub filter: Option<String>,
}

impl Default for BenchmarkSettings {
    fn default() -> Self {
        Self {
            warm_up_seconds: 3.0,
            measurement_seconds: 5.0,
            sample_size: 30,
            process_timeout_seconds: DEFAULT_PROCESS_TIMEOUT_SECONDS,
            result_root: "results".to_owned(),
            filter: None,
        }
    }
}

impl BenchmarkSettings {
    /// Reads Criterion controls from `TOMLSMITH_BENCH_*` environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error when a value is not numeric, durations are non-positive, or the sample
    /// size is below Criterion's minimum of 10.
    pub fn from_env() -> Result<Self, EnvironmentError> {
        reject_cargo_bench_profile_overrides()?;
        let defaults = Self::default();
        let warm_up_seconds =
            parse_env_f64("TOMLSMITH_BENCH_WARMUP_SECS", defaults.warm_up_seconds)?;
        let measurement_seconds =
            parse_env_f64("TOMLSMITH_BENCH_MEASUREMENT_SECS", defaults.measurement_seconds)?;
        let sample_size = parse_env_usize("TOMLSMITH_BENCH_SAMPLE_SIZE", defaults.sample_size)?;
        let process_timeout_seconds =
            parse_env_u64(PROCESS_TIMEOUT_ENV, defaults.process_timeout_seconds)?;
        if !warm_up_seconds.is_finite() || warm_up_seconds <= 0.0 {
            return Err(EnvironmentError::InvalidSetting {
                name: "TOMLSMITH_BENCH_WARMUP_SECS",
                reason: "must be finite and greater than zero".to_owned(),
            });
        }
        if !measurement_seconds.is_finite() || measurement_seconds <= 0.0 {
            return Err(EnvironmentError::InvalidSetting {
                name: "TOMLSMITH_BENCH_MEASUREMENT_SECS",
                reason: "must be finite and greater than zero".to_owned(),
            });
        }
        if sample_size < 10 {
            return Err(EnvironmentError::InvalidSetting {
                name: "TOMLSMITH_BENCH_SAMPLE_SIZE",
                reason: "must be at least 10".to_owned(),
            });
        }
        if process_timeout_seconds == 0 {
            return Err(EnvironmentError::InvalidSetting {
                name: PROCESS_TIMEOUT_ENV,
                reason: "must be greater than zero".to_owned(),
            });
        }
        let result_root =
            std::env::var("TOMLSMITH_BENCH_RESULT_ROOT").unwrap_or(defaults.result_root);
        let filter = match std::env::var("TOMLSMITH_BENCH_FILTER") {
            Ok(filter) if filter.is_empty() => {
                return Err(EnvironmentError::InvalidSetting {
                    name: "TOMLSMITH_BENCH_FILTER",
                    reason: "must not be empty when set".to_owned(),
                });
            }
            Ok(filter)
                if filter.chars().all(|character| {
                    character.is_ascii_alphanumeric() || "._/-".contains(character)
                }) =>
            {
                Some(filter)
            }
            Ok(_) => {
                return Err(EnvironmentError::InvalidSetting {
                    name: "TOMLSMITH_BENCH_FILTER",
                    reason:
                        "may contain only ASCII letters, numbers, dot, underscore, slash, or hyphen"
                            .to_owned(),
                });
            }
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => {
                return Err(EnvironmentError::InvalidSetting {
                    name: "TOMLSMITH_BENCH_FILTER",
                    reason: error.to_string(),
                });
            }
        };
        Ok(Self {
            warm_up_seconds,
            measurement_seconds,
            sample_size,
            process_timeout_seconds,
            result_root,
            filter,
        })
    }

    #[must_use]
    pub fn includes(&self, benchmark_id: &str) -> bool {
        self.filter.as_deref().is_none_or(|filter| benchmark_id.contains(filter))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolMetadata {
    pub command: String,
    pub version_verbose: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeMetadata {
    pub go: Option<ToolMetadata>,
    pub node: Option<ToolMetadata>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GitMetadata {
    pub path: PathBuf,
    pub revision: Option<String>,
    pub dirty: Option<bool>,
    pub dirty_diff_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BuildMetadata {
    pub target_triple: &'static str,
    pub cargo_profile: CargoProfileMetadata,
    pub metadata_collector_profile: &'static str,
    pub rustflags: Option<String>,
    pub rustdocflags: Option<String>,
    pub allocator: &'static str,
    pub allocator_note: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct CargoProfileMetadata {
    pub name: &'static str,
    pub opt_level: u8,
    pub debug: bool,
    pub lto: bool,
    pub codegen_units: u8,
    pub incremental: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PowerMetadata {
    pub governor: Option<String>,
    pub power_source: Option<String>,
    pub snapshot: Option<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnvironmentReport {
    pub captured_unix_seconds: u64,
    pub os: &'static str,
    pub arch: &'static str,
    pub cpu_model: Option<String>,
    pub logical_cpus: usize,
    pub rustc: ToolMetadata,
    pub cargo: ToolMetadata,
    pub runtimes: RuntimeMetadata,
    pub benchmark_settings: BenchmarkSettings,
    pub corpus_manifest_sha256: String,
    pub cargo_lock_sha256: String,
    pub build: BuildMetadata,
    pub power: PowerMetadata,
    pub products: Vec<ProductStatus>,
    pub benchmark_git: GitMetadata,
}

impl EnvironmentReport {
    /// Captures enough machine, toolchain, checkout, corpus, and Criterion configuration metadata
    /// to interpret a benchmark run.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be read, a required tool cannot be queried, system
    /// time is unavailable, or benchmark settings are invalid.
    pub fn capture(corpus_root: impl AsRef<Path>) -> Result<Self, EnvironmentError> {
        let manifest_path = corpus_root.as_ref().join("fixtures/manifest.json");
        let manifest = fs::read(&manifest_path)
            .map_err(|source| EnvironmentError::Read { path: manifest_path, source })?;
        let captured_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(EnvironmentError::Clock)?
            .as_secs();
        let benchmark_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .ok_or(EnvironmentError::WorkspaceLayout)?
            .to_path_buf();
        let cargo_lock_path = benchmark_root.join("Cargo.lock");
        let cargo_lock = fs::read(&cargo_lock_path)
            .map_err(|source| EnvironmentError::Read { path: cargo_lock_path, source })?;

        let products = product_statuses();
        let runtimes = runtime_metadata(&products)?;

        Ok(Self {
            captured_unix_seconds,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            cpu_model: cpu_model(),
            logical_cpus: std::thread::available_parallelism().map_or(1, usize::from),
            rustc: required_tool_metadata(
                selected_tool_command("TOMLSMITH_BENCH_RUSTC_COMMAND", "RUSTC", "rustc")?,
                &["-Vv"],
            )?,
            cargo: required_tool_metadata(
                selected_tool_command("TOMLSMITH_BENCH_CARGO_COMMAND", "CARGO", "cargo")?,
                &["-Vv"],
            )?,
            runtimes,
            benchmark_settings: BenchmarkSettings::from_env()?,
            corpus_manifest_sha256: format!("{:x}", Sha256::digest(manifest)),
            cargo_lock_sha256: format!("{:x}", Sha256::digest(cargo_lock)),
            build: BuildMetadata {
                target_triple: env!("TOMLSMITH_BUILD_TARGET"),
                cargo_profile: CargoProfileMetadata {
                    name: "bench",
                    opt_level: 3,
                    debug: false,
                    lto: false,
                    codegen_units: 16,
                    incremental: false,
                },
                metadata_collector_profile: env!("TOMLSMITH_BUILD_PROFILE"),
                rustflags: nonempty(option_env!("TOMLSMITH_BUILD_ENCODED_RUSTFLAGS"))
                    .map(|flags| flags.replace('\u{1f}', " ")),
                rustdocflags: nonempty(option_env!("TOMLSMITH_BUILD_RUSTDOCFLAGS"))
                    .map(str::to_owned),
                allocator: "rust_std_default_system",
                allocator_note: "no custom #[global_allocator] is configured by either benchmark binary",
            },
            power: power_metadata(),
            products,
            benchmark_git: git_metadata(benchmark_root),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnvironmentError {
    #[error("invalid benchmark setting {name}: {reason}")]
    InvalidSetting { name: &'static str, reason: String },
    #[error(
        "Cargo benchmark profile override {name} is not allowed; edit the checked-in bench profile instead"
    )]
    ForbiddenCargoBenchOverride { name: String },
    #[error("tool command environment {name} is not valid Unicode: {source}")]
    ToolCommandEnvironment { name: &'static str, source: std::env::VarError },
    #[error("failed to read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("failed to run {program}: {source}")]
    Command { program: String, source: std::io::Error },
    #[error("{program} exited unsuccessfully: {stderr}")]
    CommandFailed { program: String, stderr: String },
    #[error("{program} produced no version output")]
    EmptyCommandOutput { program: String },
    #[error("system clock is earlier than the Unix epoch: {0}")]
    Clock(std::time::SystemTimeError),
    #[error("compiled workspace layout does not contain the benchmark root")]
    WorkspaceLayout,
}

fn parse_env_f64(name: &'static str, default: f64) -> Result<f64, EnvironmentError> {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| EnvironmentError::InvalidSetting { name, reason: format!("{error}") }),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(EnvironmentError::InvalidSetting { name, reason: error.to_string() }),
    }
}

fn parse_env_usize(name: &'static str, default: usize) -> Result<usize, EnvironmentError> {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| EnvironmentError::InvalidSetting { name, reason: format!("{error}") }),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(EnvironmentError::InvalidSetting { name, reason: error.to_string() }),
    }
}

fn parse_env_u64(name: &'static str, default: u64) -> Result<u64, EnvironmentError> {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| EnvironmentError::InvalidSetting { name, reason: format!("{error}") }),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(EnvironmentError::InvalidSetting { name, reason: error.to_string() }),
    }
}

fn required_tool_metadata(
    program: String,
    arguments: &[&str],
) -> Result<ToolMetadata, EnvironmentError> {
    let output = Command::new(&program)
        .args(arguments)
        .output()
        .map_err(|source| EnvironmentError::Command { program: program.clone(), source })?;
    if !output.status.success() {
        return Err(EnvironmentError::CommandFailed {
            program,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version_verbose = if stdout.trim().is_empty() { stderr.trim() } else { stdout.trim() };
    if version_verbose.is_empty() {
        return Err(EnvironmentError::EmptyCommandOutput { program });
    }
    Ok(ToolMetadata { command: program, version_verbose: version_verbose.to_owned() })
}

fn runtime_metadata(products: &[ProductStatus]) -> Result<RuntimeMetadata, EnvironmentError> {
    let go = has_enabled_product(products, &[ProductId::BurntSushiToml, ProductId::GoTomlTomll])
        .then(|| {
            required_tool_metadata(selected_runtime_command(GO_BINARY_ENV, "go")?, &["version"])
        })
        .transpose()?;
    let node = has_enabled_product(products, &[ProductId::TomlSmith, ProductId::Prettier])
        .then(|| {
            required_tool_metadata(
                selected_runtime_command("TOMLSMITH_BENCH_NODE_COMMAND", "node")?,
                &["--version"],
            )
        })
        .transpose()?;
    Ok(RuntimeMetadata { go, node })
}

fn has_enabled_product(products: &[ProductStatus], ids: &[ProductId]) -> bool {
    products.iter().any(|status| {
        ids.contains(&status.descriptor.id)
            && matches!(&status.availability, OptionalToolAvailability::Enabled)
    })
}

fn selected_runtime_command(
    name: &'static str,
    default: &'static str,
) -> Result<String, EnvironmentError> {
    match std::env::var(name) {
        Ok(command) => Ok(command),
        Err(std::env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(source) => Err(EnvironmentError::ToolCommandEnvironment { name, source }),
    }
}

fn selected_tool_command(
    internal_name: &'static str,
    public_name: &'static str,
    default: &'static str,
) -> Result<String, EnvironmentError> {
    match std::env::var(internal_name) {
        Ok(command) => Ok(command),
        Err(std::env::VarError::NotPresent) => match std::env::var(public_name) {
            Ok(command) => Ok(command),
            Err(std::env::VarError::NotPresent) => Ok(default.to_owned()),
            Err(source) => {
                Err(EnvironmentError::ToolCommandEnvironment { name: public_name, source })
            }
        },
        Err(source) => {
            Err(EnvironmentError::ToolCommandEnvironment { name: internal_name, source })
        }
    }
}

fn reject_cargo_bench_profile_overrides() -> Result<(), EnvironmentError> {
    let mut forbidden = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| {
            name.starts_with("CARGO_PROFILE_BENCH_")
                || matches!(name.as_str(), "CARGO_INCREMENTAL" | "CARGO_BUILD_INCREMENTAL")
        })
        .collect::<Vec<_>>();
    forbidden.sort_unstable();
    forbidden.first().map_or(Ok(()), |name| {
        Err(EnvironmentError::ForbiddenCargoBenchOverride { name: name.clone() })
    })
}

fn optional_command(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

fn cpu_model() -> Option<String> {
    if std::env::consts::OS == "macos" {
        optional_command("sysctl", &["-n", "machdep.cpu.brand_string"])
            .or_else(|| optional_command("sysctl", &["-n", "hw.model"]))
    } else if std::env::consts::OS == "linux" {
        fs::read_to_string("/proc/cpuinfo").ok().and_then(|cpuinfo| {
            cpuinfo.lines().find_map(|line| {
                line.strip_prefix("model name\t:")
                    .or_else(|| line.strip_prefix("Hardware\t:"))
                    .map(str::trim)
                    .map(str::to_owned)
            })
        })
    } else {
        None
    }
}

fn git_metadata(path: PathBuf) -> GitMetadata {
    let path_text = path.to_string_lossy();
    let revision = optional_command("git", &["-C", &path_text, "rev-parse", "HEAD"]);
    let dirty = optional_command("git", &["-C", &path_text, "status", "--porcelain"])
        .map(|status| !status.is_empty());
    let dirty_diff_sha256 = (dirty == Some(true)).then(|| working_tree_digest(&path));
    GitMetadata { path, revision, dirty, dirty_diff_sha256 }
}

fn working_tree_digest(path: &Path) -> String {
    let path_text = path.to_string_lossy();
    let mut hasher = Sha256::new();
    for arguments in [
        vec!["-C", path_text.as_ref(), "diff", "--binary", "--no-ext-diff"],
        vec!["-C", path_text.as_ref(), "diff", "--binary", "--cached", "--no-ext-diff"],
    ] {
        if let Ok(output) = Command::new("git").args(arguments).output()
            && output.status.success()
        {
            hasher.update(output.stdout);
        }
    }
    if let Ok(output) = Command::new("git")
        .args(["-C", path_text.as_ref(), "ls-files", "--others", "--exclude-standard", "-z"])
        .output()
        && output.status.success()
    {
        for relative in output.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
            hasher.update((relative.len() as u64).to_be_bytes());
            hasher.update(relative);
            let relative = String::from_utf8_lossy(relative);
            if let Ok(bytes) = fs::read(path.join(relative.as_ref())) {
                hasher.update((bytes.len() as u64).to_be_bytes());
                hasher.update(bytes);
            }
        }
    }
    format!("{:x}", hasher.finalize())
}

fn power_metadata() -> PowerMetadata {
    if std::env::consts::OS == "linux" {
        let governor = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
            .ok()
            .map(|value| value.trim().to_owned());
        let power_source =
            fs::read_to_string("/sys/class/power_supply/AC/online").ok().map(|value| {
                if value.trim() == "1" { "AC".to_owned() } else { "battery".to_owned() }
            });
        let unavailable_reason = (governor.is_none() || power_source.is_none()).then(|| {
            "one or more Linux cpufreq/power-supply sysfs fields were unavailable".to_owned()
        });
        PowerMetadata { governor, power_source, snapshot: None, unavailable_reason }
    } else if std::env::consts::OS == "macos" {
        let snapshot = optional_command("pmset", &["-g", "batt"]);
        let power_source = snapshot.as_deref().and_then(|snapshot| {
            if snapshot.contains("AC Power") {
                Some("AC".to_owned())
            } else if snapshot.contains("Battery Power") {
                Some("battery".to_owned())
            } else {
                None
            }
        });
        PowerMetadata {
            governor: None,
            power_source,
            snapshot,
            unavailable_reason: Some(
                "macOS does not expose the Linux scaling_governor interface".to_owned(),
            ),
        }
    } else {
        PowerMetadata {
            governor: None,
            power_source: None,
            snapshot: None,
            unavailable_reason: Some(
                "power and frequency metadata collection is not implemented for this OS".to_owned(),
            ),
        }
    }
}
