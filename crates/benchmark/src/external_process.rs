use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use command_group::CommandGroup;
use serde::Serialize;
use sha2::{Digest, Sha256};
use wait_timeout::ChildExt;

use crate::TomlVersion;

pub const TOMBI_REQUIRED_VERSION: &str = "1.4.1";
pub const TOMBI_BINARY_ENV: &str = "TOMLSMITH_TOMBI_BIN";
pub const PRETTIER_PLUGIN_ENV: &str = "TOMLSMITH_PRETTIER_PLUGIN";
pub const PRETTIER_PLUGIN_REQUIRED_VERSION: &str = "2.0.6";
pub const GO_BINARY_ENV: &str = "TOMLSMITH_GO_BIN";
pub const TIME_BINARY_ENV: &str = "TOMLSMITH_BENCH_TIME_COMMAND";
/// Overrides the `TomlSmith` version pin for a locally built product executable: `any` accepts
/// every version, another value replaces the pinned string. Published lanes leave it unset so
/// results stay tied to the exact crates.io release.
pub const TOMLSMITH_EXPECTED_VERSION_ENV: &str = "TOMLSMITH_BIN_EXPECTED_VERSION";
pub const PROCESS_TIMEOUT_ENV: &str = "TOMLSMITH_BENCH_PROCESS_TIMEOUT_SECS";
pub const DEFAULT_PROCESS_TIMEOUT_SECONDS: u64 = 120;
pub const DPRINT_TOML_PLUGIN_URL: &str = "https://plugins.dprint.dev/toml-0.8.0.wasm";

/// A product executable measured through its real command-line entry point.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProductId {
    #[serde(rename = "tomlsmith")]
    TomlSmith,
    Tombi,
    Taplo,
    Prettier,
    Dprint,
    #[serde(rename = "burntsushi-toml")]
    BurntSushiToml,
    GoTomlTomll,
}

impl ProductId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TomlSmith => "tomlsmith",
            Self::Tombi => "tombi",
            Self::Taplo => "taplo",
            Self::Prettier => "prettier",
            Self::Dprint => "dprint",
            Self::BurntSushiToml => "burntsushi-toml",
            Self::GoTomlTomll => "go-toml-tomll",
        }
    }
}

/// A product-level operation. Each invocation includes process startup and all standard-stream I/O.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductOperation {
    Check,
    Format,
}

impl ProductOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Format => "format",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductVersionSupport {
    StrictSelectable,
    Fixed,
    CompatibleSubset,
}

/// Stable metadata for one executable product adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProductDescriptor {
    pub id: ProductId,
    pub display_name: &'static str,
    pub implementation_language: &'static str,
    pub upstream: Option<&'static str>,
    pub required_version: &'static str,
    pub binary_env: &'static str,
    pub companion_env: Option<&'static str>,
    pub operations: &'static [ProductOperation],
    pub toml_versions: &'static [TomlVersion],
    pub version_support: ProductVersionSupport,
    pub input_transport: &'static str,
    pub isolation: &'static str,
}

impl ProductDescriptor {
    #[must_use]
    pub fn supports_version(self, version: TomlVersion) -> bool {
        self.toml_versions.contains(&version)
    }
}

const CHECK_AND_FORMAT: &[ProductOperation] = &[ProductOperation::Check, ProductOperation::Format];
const CHECK_ONLY: &[ProductOperation] = &[ProductOperation::Check];
const FORMAT_ONLY: &[ProductOperation] = &[ProductOperation::Format];
const TOML_1_0_ONLY: &[TomlVersion] = &[TomlVersion::V1_0];
const TOML_1_0_AND_1_1: &[TomlVersion] = &[TomlVersion::V1_0, TomlVersion::V1_1];

const PRODUCT_CATALOG: &[ProductDescriptor] = &[
    ProductDescriptor {
        id: ProductId::TomlSmith,
        display_name: "TomlSmith native CLI",
        implementation_language: "Rust",
        upstream: None,
        required_version: "0.3.1",
        binary_env: "TOMLSMITH_BIN",
        companion_env: None,
        operations: CHECK_AND_FORMAT,
        toml_versions: TOML_1_0_AND_1_1,
        version_support: ProductVersionSupport::StrictSelectable,
        input_transport: "native executable; stdin/stdout/stderr",
        isolation: "per-version empty working directory",
    },
    ProductDescriptor {
        id: ProductId::Tombi,
        display_name: "Tombi",
        implementation_language: "Rust",
        upstream: Some("https://github.com/tombi-toml/tombi"),
        required_version: TOMBI_REQUIRED_VERSION,
        binary_env: TOMBI_BINARY_ENV,
        companion_env: None,
        operations: CHECK_AND_FORMAT,
        toml_versions: TOML_1_0_AND_1_1,
        version_support: ProductVersionSupport::StrictSelectable,
        input_transport: "stdin/stdout/stderr",
        isolation: "per-version tombi.toml; schema.enabled=false; --offline",
    },
    ProductDescriptor {
        id: ProductId::Taplo,
        display_name: "Taplo CLI",
        implementation_language: "Rust",
        upstream: Some("https://github.com/tamasfe/taplo"),
        required_version: "0.10.0",
        binary_env: "TOMLSMITH_TAPLO_BIN",
        companion_env: None,
        operations: CHECK_AND_FORMAT,
        toml_versions: TOML_1_0_ONLY,
        version_support: ProductVersionSupport::Fixed,
        input_transport: "stdin/stdout/stderr",
        isolation: "per-version empty working directory; schema disabled",
    },
    ProductDescriptor {
        id: ProductId::Prettier,
        display_name: "Prettier + prettier-plugin-toml",
        implementation_language: "TypeScript",
        upstream: Some("https://github.com/un-ts/prettier/tree/master/packages/toml"),
        required_version: "3.9.6",
        binary_env: "TOMLSMITH_PRETTIER_BIN",
        companion_env: Some(PRETTIER_PLUGIN_ENV),
        operations: FORMAT_ONLY,
        toml_versions: TOML_1_0_ONLY,
        version_support: ProductVersionSupport::Fixed,
        input_transport: "stdin/stdout/stderr",
        isolation: "explicit TOML plugin and stdin filepath; no project config",
    },
    ProductDescriptor {
        id: ProductId::Dprint,
        display_name: "dprint + TOML plugin",
        implementation_language: "Rust + WebAssembly",
        upstream: Some("https://github.com/dprint/dprint-plugin-toml"),
        required_version: "0.56.1",
        binary_env: "TOMLSMITH_DPRINT_BIN",
        companion_env: None,
        operations: FORMAT_ONLY,
        toml_versions: TOML_1_0_AND_1_1,
        version_support: ProductVersionSupport::CompatibleSubset,
        input_transport: "stdin/stdout/stderr",
        isolation: "per-version dprint.json with the TOML plugin pinned",
    },
    ProductDescriptor {
        id: ProductId::BurntSushiToml,
        display_name: "BurntSushi/toml tomlv",
        implementation_language: "Go",
        upstream: Some("https://github.com/BurntSushi/toml"),
        required_version: "1.6.0",
        binary_env: "TOMLSMITH_BURNTSUSHI_TOMLV_BIN",
        companion_env: None,
        operations: CHECK_ONLY,
        toml_versions: TOML_1_0_AND_1_1,
        version_support: ProductVersionSupport::CompatibleSubset,
        input_transport: "stdin/stdout/stderr",
        isolation: "official tomlv validator mode; empty working directory",
    },
    ProductDescriptor {
        id: ProductId::GoTomlTomll,
        display_name: "pelletier/go-toml tomll",
        implementation_language: "Go",
        upstream: Some("https://github.com/pelletier/go-toml"),
        required_version: "2.4.3",
        binary_env: "TOMLSMITH_GO_TOMLL_BIN",
        companion_env: None,
        operations: FORMAT_ONLY,
        toml_versions: TOML_1_0_AND_1_1,
        version_support: ProductVersionSupport::CompatibleSubset,
        input_transport: "stdin/stdout/stderr",
        isolation: "official tomll stdin format mode; empty working directory",
    },
];

#[must_use]
pub const fn product_catalog() -> &'static [ProductDescriptor] {
    PRODUCT_CATALOG
}

/// A probed product executable. Creating a runner never searches `PATH`.
#[derive(Clone, Debug)]
struct PrettierPluginIdentity {
    entry_path: PathBuf,
    entry_sha256: String,
    package_json_path: PathBuf,
    package_json_sha256: String,
    version: String,
}

#[derive(Clone, Debug)]
pub struct ProductRunner {
    descriptor: ProductDescriptor,
    binary: PathBuf,
    version_output: String,
    detected_version: Option<String>,
    binary_sha256: String,
    prettier_plugin: Option<PrettierPluginIdentity>,
    process_timeout: Duration,
}

impl ProductRunner {
    /// Resolves an opt-in product selection without consulting `PATH`.
    ///
    /// `None` for the binary means the product is intentionally skipped. Prettier additionally
    /// requires the absolute `prettier-plugin-toml` entry point or package directory.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::probe`], plus companion-path errors for Prettier.
    pub fn from_explicit_paths(
        product_id: ProductId,
        binary: Option<PathBuf>,
        companion: Option<PathBuf>,
    ) -> Result<Option<Self>, ProductProcessError> {
        let Some(binary) = binary else {
            return Ok(None);
        };
        let descriptor = descriptor(product_id);
        if binary.as_os_str().is_empty() {
            return Err(ProductProcessError::EmptyConfiguredPath {
                product: product_id,
                environment: descriptor.binary_env,
            });
        }
        let mut runner = Self::probe(product_id, binary)?;
        if product_id == ProductId::Prettier {
            let companion = companion.ok_or(ProductProcessError::MissingCompanion {
                product: product_id,
                environment: PRETTIER_PLUGIN_ENV,
            })?;
            if companion.as_os_str().is_empty() {
                return Err(ProductProcessError::EmptyConfiguredPath {
                    product: product_id,
                    environment: PRETTIER_PLUGIN_ENV,
                });
            }
            runner = runner.with_prettier_plugin(companion)?;
        }
        Ok(Some(runner))
    }

    /// Resolves the product only from its catalogued opt-in environment variables.
    ///
    /// # Errors
    ///
    /// Returns errors from explicit path validation and probing.
    pub fn from_env(product_id: ProductId) -> Result<Option<Self>, ProductProcessError> {
        let descriptor = descriptor(product_id);
        let binary = std::env::var_os(descriptor.binary_env).map(PathBuf::from);
        let companion = descriptor.companion_env.and_then(std::env::var_os).map(PathBuf::from);
        Self::from_explicit_paths(product_id, binary, companion)
    }

    /// Probes an explicitly selected product executable.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is not absolute, the executable cannot be read or spawned,
    /// or its version command fails or produces no output.
    pub fn probe(
        product_id: ProductId,
        path: impl AsRef<Path>,
    ) -> Result<Self, ProductProcessError> {
        let go_helper =
            std::env::var_os(GO_BINARY_ENV).map_or_else(|| PathBuf::from("go"), PathBuf::from);
        Self::probe_with_go_binary(product_id, path, go_helper)
    }

    /// Probes an explicitly selected product executable while using an explicit Go tool for
    /// Go build-info verification.
    ///
    /// Go products are identified from their embedded module path and exact module version. Their
    /// own `--version` behavior is deliberately ignored, so a different executable cannot satisfy
    /// the version pin by printing a matching string. The Go tool is ignored for non-Go products.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::probe`], including Go build-info identity failures.
    pub fn probe_with_go_binary(
        product_id: ProductId,
        path: impl AsRef<Path>,
        go_binary: impl AsRef<Path>,
    ) -> Result<Self, ProductProcessError> {
        let descriptor = descriptor(product_id);
        let binary = path.as_ref().to_path_buf();
        if !binary.is_absolute() {
            return Err(ProductProcessError::BinaryPathMustBeAbsolute {
                product: product_id,
                path: binary,
            });
        }
        let bytes = fs::read(&binary).map_err(|source| ProductProcessError::ReadBinary {
            product: product_id,
            path: binary.clone(),
            source,
        })?;
        let version_output =
            if let Some((expected_module, expected_version)) = go_module_requirement(product_id) {
                probe_go_build_info(
                    product_id,
                    &binary,
                    go_binary.as_ref(),
                    expected_module,
                    expected_version,
                )?
            } else {
                let output = Command::new(&binary).arg("--version").output().map_err(|source| {
                    ProductProcessError::Spawn { product: product_id, path: binary.clone(), source }
                })?;
                if !output.status.success() {
                    return Err(ProductProcessError::VersionCommand {
                        product: product_id,
                        path: binary,
                        status: output.status.code(),
                        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                    });
                }
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stdout.trim().is_empty() {
                    stderr.trim().to_owned()
                } else {
                    stdout.trim().to_owned()
                }
            };
        if version_output.is_empty() {
            return Err(ProductProcessError::EmptyVersionOutput { product: product_id });
        }
        let detected_version = extract_version(&version_output);
        let expected_version = expected_version(product_id, descriptor);
        if expected_version
            .as_deref()
            .is_some_and(|expected| detected_version.as_deref() != Some(expected))
        {
            return Err(ProductProcessError::VersionMismatch {
                product: product_id,
                required: expected_version.unwrap_or_default(),
                detected: detected_version,
                output: version_output,
            });
        }
        Ok(Self {
            descriptor,
            binary,
            detected_version,
            version_output,
            binary_sha256: format!("{:x}", Sha256::digest(bytes)),
            prettier_plugin: None,
            process_timeout: configured_process_timeout(product_id)?,
        })
    }

    /// Overrides the correctness and resource-sampling process timeout.
    ///
    /// Timed Criterion iterations deliberately use [`Self::run_prepared`] without containment so
    /// process-group setup does not contaminate latency. They are preceded by bounded correctness
    /// and preflight calls.
    ///
    /// # Errors
    ///
    /// Returns an error when `timeout` is zero.
    pub fn with_process_timeout(mut self, timeout: Duration) -> Result<Self, ProductProcessError> {
        if timeout.is_zero() {
            return Err(ProductProcessError::InvalidProcessTimeout {
                product: self.product_id(),
                value: "0".to_owned(),
                reason: "must be greater than zero".to_owned(),
            });
        }
        self.process_timeout = timeout;
        Ok(self)
    }

    #[must_use]
    pub const fn product_id(&self) -> ProductId {
        self.descriptor.id
    }

    #[must_use]
    pub const fn descriptor(&self) -> ProductDescriptor {
        self.descriptor
    }

    #[must_use]
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    #[must_use]
    pub fn version_output(&self) -> &str {
        &self.version_output
    }

    #[must_use]
    pub fn detected_version(&self) -> Option<&str> {
        self.detected_version.as_deref()
    }

    #[must_use]
    pub fn binary_sha256(&self) -> &str {
        &self.binary_sha256
    }

    /// Attaches the separately installed `prettier-plugin-toml` entry point.
    ///
    /// # Errors
    ///
    /// Returns an error when used for another product, or when the plugin path is not an existing
    /// absolute path. A plugin may be either its module entry file or its package directory.
    pub fn with_prettier_plugin(
        mut self,
        path: impl AsRef<Path>,
    ) -> Result<Self, ProductProcessError> {
        if self.product_id() != ProductId::Prettier {
            return Err(ProductProcessError::CompanionNotApplicable { product: self.product_id() });
        }
        let path = path.as_ref().to_path_buf();
        if !path.is_absolute() {
            return Err(ProductProcessError::CompanionPathMustBeAbsolute {
                product: self.product_id(),
                path,
            });
        }
        self.prettier_plugin = Some(inspect_prettier_plugin(self.product_id(), &path)?);
        Ok(self)
    }

    #[must_use]
    pub fn prettier_plugin(&self) -> Option<&Path> {
        self.prettier_plugin.as_ref().map(|plugin| plugin.entry_path.as_path())
    }

    #[must_use]
    pub fn status(&self) -> ProductStatus {
        ProductStatus {
            descriptor: self.descriptor,
            availability: OptionalToolAvailability::Enabled,
            binary_path: Some(self.binary.clone()),
            detected_version: self.detected_version.clone(),
            version_output: Some(self.version_output.clone()),
            binary_sha256: Some(self.binary_sha256.clone()),
            companion_path: self.prettier_plugin.as_ref().map(|plugin| plugin.entry_path.clone()),
            companion_version: self.prettier_plugin.as_ref().map(|plugin| plugin.version.clone()),
            companion_package_json_path: self
                .prettier_plugin
                .as_ref()
                .map(|plugin| plugin.package_json_path.clone()),
            companion_package_json_sha256: self
                .prettier_plugin
                .as_ref()
                .map(|plugin| plugin.package_json_sha256.clone()),
            companion_entry_sha256: self
                .prettier_plugin
                .as_ref()
                .map(|plugin| plugin.entry_sha256.clone()),
            process_timeout_millis: Some(
                u64::try_from(self.process_timeout.as_millis()).unwrap_or(u64::MAX),
            ),
        }
    }

    /// Creates the product's isolated working directory and fixed configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when a directory or configuration file cannot be written.
    pub fn prepare_isolation(
        &self,
        root: impl AsRef<Path>,
        version: TomlVersion,
    ) -> Result<PathBuf, ProductProcessError> {
        let directory =
            root.as_ref().join(self.product_id().as_str()).join(version_directory(version));
        fs::create_dir_all(&directory).map_err(|source| ProductProcessError::Isolation {
            product: self.product_id(),
            path: directory.clone(),
            source,
        })?;
        if self.product_id() == ProductId::Tombi {
            let config_path = directory.join("tombi.toml");
            fs::write(
                &config_path,
                format!(
                    "toml-version = \"{}\"\n\n[schema]\nenabled = false\n",
                    version_config_label(version)
                ),
            )
            .map_err(|source| ProductProcessError::Isolation {
                product: self.product_id(),
                path: config_path,
                source,
            })?;
        } else if self.product_id() == ProductId::Dprint {
            let config_path = directory.join("dprint.json");
            fs::write(
                &config_path,
                format!(
                    "{{\n  \"plugins\": [\"{DPRINT_TOML_PLUGIN_URL}\"],\n  \"toml\": {{}}\n}}\n"
                ),
            )
            .map_err(|source| ProductProcessError::Isolation {
                product: self.product_id(),
                path: config_path,
                source,
            })?;
        }
        Ok(directory)
    }

    /// Runs one fresh product process in an already prepared isolated directory.
    ///
    /// This is the uncontained latency-measurement seam: it avoids process-group and timeout
    /// overhead inside Criterion iterations. Call [`Self::run_prepared_bounded`] for correctness,
    /// preflight, and other untimed execution.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported operation or any process/I/O failure.
    pub fn run_prepared(
        &self,
        operation: ProductOperation,
        version: TomlVersion,
        source: &str,
        isolation_directory: impl AsRef<Path>,
    ) -> Result<ProductProcessOutput, ProductProcessError> {
        let isolation_directory = isolation_directory.as_ref();
        let arguments = self.command_arguments(operation, version, isolation_directory)?;
        let mut command = Command::new(&self.binary);
        command
            .args(&arguments)
            .current_dir(isolation_directory)
            .env("NO_COLOR", "1")
            .env("TOMBI_NO_COLOR", "1");
        self.execute_prepared(command, operation, source, isolation_directory, &self.binary)
    }

    /// Runs one fresh product process in an isolated directory with whole-process-tree timeout
    /// containment.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported operation, any process/I/O failure, or when the process
    /// tree exceeds the configured timeout.
    pub fn run_prepared_bounded(
        &self,
        operation: ProductOperation,
        version: TomlVersion,
        source: &str,
        isolation_directory: impl AsRef<Path>,
    ) -> Result<ProductProcessOutput, ProductProcessError> {
        let isolation_directory = isolation_directory.as_ref();
        let arguments = self.command_arguments(operation, version, isolation_directory)?;
        let mut command = Command::new(&self.binary);
        command
            .args(&arguments)
            .current_dir(isolation_directory)
            .env("NO_COLOR", "1")
            .env("TOMBI_NO_COLOR", "1");
        self.execute_prepared_bounded(command, operation, source, isolation_directory, &self.binary)
    }

    /// Runs one fresh product process under the platform's process resource meter.
    ///
    /// This is deliberately separate from Criterion timing so resource measurement overhead never
    /// contaminates latency samples.
    ///
    /// # Errors
    ///
    /// Returns an error when peak RSS measurement is unavailable, the product fails, or the
    /// platform resource output cannot be parsed.
    pub fn run_prepared_with_peak_rss(
        &self,
        operation: ProductOperation,
        version: TomlVersion,
        source: &str,
        isolation_directory: impl AsRef<Path>,
    ) -> Result<PeakMemoryProcessOutput, ProductProcessError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let isolation_directory = isolation_directory.as_ref();
            let arguments = self.command_arguments(operation, version, isolation_directory)?;
            let meter = std::env::var_os(TIME_BINARY_ENV)
                .map_or_else(|| PathBuf::from("/usr/bin/time"), PathBuf::from);
            if !meter.is_absolute() {
                return Err(ProductProcessError::TimePathMustBeAbsolute { path: meter });
            }
            let stats = tempfile::NamedTempFile::new_in(isolation_directory).map_err(|source| {
                ProductProcessError::MemoryStatsFile {
                    product: self.product_id(),
                    path: isolation_directory.to_path_buf(),
                    source,
                }
            })?;
            let stats_path = stats.into_temp_path();
            let mut command = Command::new(&meter);
            #[cfg(target_os = "linux")]
            command.arg("-v");
            #[cfg(target_os = "macos")]
            command.arg("-l");
            command
                .arg("-o")
                .arg(&stats_path)
                .arg(&self.binary)
                .args(&arguments)
                .current_dir(isolation_directory)
                .env("NO_COLOR", "1")
                .env("TOMBI_NO_COLOR", "1")
                .env("LC_ALL", "C");
            let process = self.execute_prepared_bounded(
                command,
                operation,
                source,
                isolation_directory,
                &meter,
            )?;
            let stats =
                fs::read(&stats_path).map_err(|source| ProductProcessError::MemoryStatsFile {
                    product: self.product_id(),
                    path: stats_path.to_path_buf(),
                    source,
                })?;
            let peak_rss_bytes = parse_peak_rss_bytes(&stats).ok_or_else(|| {
                ProductProcessError::MissingPeakRss {
                    product: self.product_id(),
                    output: String::from_utf8_lossy(&stats).trim().to_owned(),
                }
            })?;
            Ok(PeakMemoryProcessOutput { peak_rss_bytes, process })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (operation, version, source, isolation_directory);
            Err(ProductProcessError::PeakRssUnsupportedPlatform)
        }
    }

    fn execute_prepared(
        &self,
        mut command: Command,
        operation: ProductOperation,
        source: &str,
        isolation_directory: &Path,
        spawn_path: &Path,
    ) -> Result<ProductProcessOutput, ProductProcessError> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| ProductProcessError::Spawn {
                product: self.product_id(),
                path: spawn_path.to_path_buf(),
                source,
            })?;
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProductProcessError::MissingStdinPipe { product: self.product_id() });
        };
        let (output, writer_result) = std::thread::scope(|scope| {
            let writer = scope.spawn(move || stdin.write_all(source.as_bytes()));
            let output = child.wait_with_output();
            let writer_result = writer
                .join()
                .map_err(|_| ProductProcessError::WriterPanicked { product: self.product_id() })
                .and_then(|result| {
                    result.map_err(|source| ProductProcessError::WriteStdin {
                        product: self.product_id(),
                        source,
                    })
                });
            (output, writer_result)
        });
        writer_result?;
        let output = output
            .map_err(|source| ProductProcessError::Wait { product: self.product_id(), source })?;
        if !output.status.success() {
            return Err(ProductProcessError::CommandFailed {
                product: self.product_id(),
                operation,
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        let fingerprint =
            output.stdout.len().wrapping_mul(31).wrapping_add(output.stderr.len()) as u64;
        Ok(ProductProcessOutput {
            status_success: true,
            stdout: output.stdout,
            stderr: output.stderr,
            fingerprint,
            isolation_directory: Some(isolation_directory.to_path_buf()),
        })
    }

    fn execute_prepared_bounded(
        &self,
        mut command: Command,
        operation: ProductOperation,
        source: &str,
        isolation_directory: &Path,
        spawn_path: &Path,
    ) -> Result<ProductProcessOutput, ProductProcessError> {
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.group_spawn().map_err(|source| ProductProcessError::Spawn {
            product: self.product_id(),
            path: spawn_path.to_path_buf(),
            source,
        })?;
        let stdin = child
            .inner()
            .stdin
            .take()
            .ok_or(ProductProcessError::MissingStdinPipe { product: self.product_id() })?;
        let stdout = child.inner().stdout.take().ok_or(ProductProcessError::MissingOutputPipe {
            product: self.product_id(),
            stream: "stdout",
        })?;
        let stderr = child.inner().stderr.take().ok_or(ProductProcessError::MissingOutputPipe {
            product: self.product_id(),
            stream: "stderr",
        })?;

        let (status, writer_result, stdout_result, stderr_result) = std::thread::scope(|scope| {
            let writer = scope.spawn(move || {
                let mut stdin = stdin;
                stdin.write_all(source.as_bytes())
            });
            let stdout_reader = scope.spawn(move || read_stream(stdout));
            let stderr_reader = scope.spawn(move || read_stream(stderr));
            let status = match child.inner().wait_timeout(self.process_timeout) {
                Ok(Some(status)) => Ok(Some(status)),
                Ok(None) => {
                    child.kill().and_then(|()| child.wait()).map(|_| None).map_err(|source| {
                        ProductProcessError::TerminateTimedOut {
                            product: self.product_id(),
                            source,
                        }
                    })
                }
                Err(source) => {
                    Err(ProductProcessError::Wait { product: self.product_id(), source })
                }
            };
            let writer_result = writer
                .join()
                .map_err(|_| ProductProcessError::WriterPanicked { product: self.product_id() });
            let stdout_result = stdout_reader.join().map_err(|_| {
                ProductProcessError::ReaderPanicked { product: self.product_id(), stream: "stdout" }
            });
            let stderr_result = stderr_reader.join().map_err(|_| {
                ProductProcessError::ReaderPanicked { product: self.product_id(), stream: "stderr" }
            });
            (status, writer_result, stdout_result, stderr_result)
        });

        let status = status?;
        let stdout = stdout_result?.map_err(|source| ProductProcessError::ReadOutput {
            product: self.product_id(),
            stream: "stdout",
            source,
        })?;
        let stderr = stderr_result?.map_err(|source| ProductProcessError::ReadOutput {
            product: self.product_id(),
            stream: "stderr",
            source,
        })?;
        if status.is_none() {
            return Err(ProductProcessError::ProcessTimedOut {
                product: self.product_id(),
                operation,
                timeout_millis: u64::try_from(self.process_timeout.as_millis()).unwrap_or(u64::MAX),
                stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
            });
        }
        let status = status.expect("completed status was checked above");
        if !status.success() {
            return Err(ProductProcessError::CommandFailed {
                product: self.product_id(),
                operation,
                status: status.code(),
                stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
            });
        }
        writer_result?.map_err(|source| ProductProcessError::WriteStdin {
            product: self.product_id(),
            source,
        })?;
        let fingerprint = stdout.len().wrapping_mul(31).wrapping_add(stderr.len()) as u64;
        Ok(ProductProcessOutput {
            status_success: true,
            stdout,
            stderr,
            fingerprint,
            isolation_directory: Some(isolation_directory.to_path_buf()),
        })
    }

    /// Prepares isolation and runs one fresh product process with timeout containment.
    ///
    /// # Errors
    ///
    /// Returns an isolation, command construction, or process execution error.
    pub fn run(
        &self,
        operation: ProductOperation,
        version: TomlVersion,
        source: &str,
        isolation_root: impl AsRef<Path>,
    ) -> Result<ProductProcessOutput, ProductProcessError> {
        let directory = self.prepare_isolation(isolation_root, version)?;
        self.run_prepared_bounded(operation, version, source, directory)
    }

    /// Returns the exact argument vector used after the executable path.
    ///
    /// # Errors
    ///
    /// Returns an error when the product does not expose the requested operation.
    pub fn command_arguments(
        &self,
        operation: ProductOperation,
        version: TomlVersion,
        isolation_directory: impl AsRef<Path>,
    ) -> Result<Vec<std::ffi::OsString>, ProductProcessError> {
        if !self.descriptor.operations.contains(&operation) {
            return Err(ProductProcessError::UnsupportedOperation {
                product: self.product_id(),
                operation,
            });
        }
        if !self.descriptor.supports_version(version) {
            return Err(ProductProcessError::UnsupportedTomlVersion {
                product: self.product_id(),
                version,
            });
        }
        match (self.product_id(), operation) {
            (ProductId::TomlSmith, ProductOperation::Check) => {
                Ok(["--toml-version", version_cli_label(version), "check", "-"]
                    .into_iter()
                    .map(Into::into)
                    .collect())
            }
            (ProductId::TomlSmith, ProductOperation::Format) => {
                Ok(["--toml-version", version_cli_label(version), "fmt", "-"]
                    .into_iter()
                    .map(Into::into)
                    .collect())
            }
            (ProductId::Tombi, ProductOperation::Check) => {
                Ok(["lint", "--offline", "--quiet", "--stdin-filename", "fixture.toml", "-"]
                    .into_iter()
                    .map(Into::into)
                    .collect())
            }
            (ProductId::Tombi, ProductOperation::Format) => {
                Ok(["format", "--offline", "--quiet", "--stdin-filename", "fixture.toml", "-"]
                    .into_iter()
                    .map(Into::into)
                    .collect())
            }
            (ProductId::Taplo, ProductOperation::Check) => {
                Ok(["lint", "--colors", "never", "--no-auto-config", "--no-schema", "-"]
                    .into_iter()
                    .map(Into::into)
                    .collect())
            }
            (ProductId::Taplo, ProductOperation::Format) => {
                Ok(["format", "--colors", "never", "--no-auto-config", "-"]
                    .into_iter()
                    .map(Into::into)
                    .collect())
            }
            (ProductId::Prettier, ProductOperation::Format) => {
                let plugin = self.prettier_plugin.as_ref().map(|plugin| &plugin.entry_path).ok_or(
                    ProductProcessError::MissingCompanion {
                        product: self.product_id(),
                        environment: PRETTIER_PLUGIN_ENV,
                    },
                )?;
                let mut plugin_argument = std::ffi::OsString::from("--plugin=");
                plugin_argument.push(plugin);
                Ok(vec![
                    plugin_argument,
                    std::ffi::OsString::from("--stdin-filepath=fixture.toml"),
                    std::ffi::OsString::from("--no-config"),
                    std::ffi::OsString::from("--no-editorconfig"),
                ])
            }
            (ProductId::Dprint, ProductOperation::Format) => {
                let config = isolation_directory.as_ref().join("dprint.json");
                Ok(vec![
                    "fmt".into(),
                    "--config".into(),
                    config.into_os_string(),
                    "--config-discovery=false".into(),
                    "--log-level".into(),
                    "silent".into(),
                    "--stdin".into(),
                    "fixture.toml".into(),
                ])
            }
            (ProductId::GoTomlTomll, ProductOperation::Format) => Ok(Vec::new()),
            (ProductId::BurntSushiToml, ProductOperation::Check) => Ok(vec!["-".into()]),
            _ => unreachable!("operation membership was checked above"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProductProcessOutput {
    pub status_success: bool,
    #[serde(skip)]
    pub stdout: Vec<u8>,
    #[serde(skip)]
    pub stderr: Vec<u8>,
    pub fingerprint: u64,
    #[serde(skip)]
    pub isolation_directory: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PeakMemoryProcessOutput {
    pub peak_rss_bytes: u64,
    pub process: ProductProcessOutput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OptionalToolAvailability {
    Enabled,
    Skipped { reason: String },
    Invalid { reason: String },
}

/// Discovery and reproducibility metadata for one product executable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProductStatus {
    pub descriptor: ProductDescriptor,
    pub availability: OptionalToolAvailability,
    pub binary_path: Option<PathBuf>,
    pub detected_version: Option<String>,
    pub version_output: Option<String>,
    pub binary_sha256: Option<String>,
    pub companion_path: Option<PathBuf>,
    pub companion_version: Option<String>,
    pub companion_package_json_path: Option<PathBuf>,
    pub companion_package_json_sha256: Option<String>,
    pub companion_entry_sha256: Option<String>,
    pub process_timeout_millis: Option<u64>,
}

/// Discovers one product from its explicit opt-in environment variables.
#[must_use]
pub fn product_status(product_id: ProductId) -> ProductStatus {
    let descriptor = descriptor(product_id);
    match ProductRunner::from_env(product_id) {
        Ok(Some(runner)) => runner.status(),
        Ok(None) => ProductStatus {
            descriptor,
            availability: OptionalToolAvailability::Skipped {
                reason: format!("{} is not set", descriptor.binary_env),
            },
            binary_path: None,
            detected_version: None,
            version_output: None,
            binary_sha256: None,
            companion_path: None,
            companion_version: None,
            companion_package_json_path: None,
            companion_package_json_sha256: None,
            companion_entry_sha256: None,
            process_timeout_millis: None,
        },
        Err(error) => ProductStatus {
            descriptor,
            availability: OptionalToolAvailability::Invalid { reason: error.to_string() },
            binary_path: None,
            detected_version: None,
            version_output: None,
            binary_sha256: None,
            companion_path: None,
            companion_version: None,
            companion_package_json_path: None,
            companion_package_json_sha256: None,
            companion_entry_sha256: None,
            process_timeout_millis: None,
        },
    }
}

/// Discovers the complete product catalog without searching `PATH`.
#[must_use]
pub fn product_statuses() -> Vec<ProductStatus> {
    product_catalog().iter().map(|product| product_status(product.id)).collect()
}

#[cfg(target_os = "linux")]
fn parse_peak_rss_bytes(output: &[u8]) -> Option<u64> {
    let output = String::from_utf8_lossy(output);
    parse_gnu_time_peak_rss(&output)
}

#[cfg(target_os = "macos")]
fn parse_peak_rss_bytes(output: &[u8]) -> Option<u64> {
    let output = String::from_utf8_lossy(output);
    parse_bsd_time_peak_rss(&output)
}

#[cfg(any(target_os = "macos", test))]
fn parse_bsd_time_peak_rss(output: &str) -> Option<u64> {
    exactly_one(output.lines().filter_map(|line| {
        line.trim().strip_suffix("maximum resident set size")?.trim().parse::<u64>().ok()
    }))
}

#[cfg(any(target_os = "linux", test))]
fn parse_gnu_time_peak_rss(output: &str) -> Option<u64> {
    exactly_one(output.lines().filter_map(|line| {
        line.trim()
            .strip_prefix("Maximum resident set size (kbytes):")?
            .trim()
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    }))
}

fn exactly_one(mut values: impl Iterator<Item = u64>) -> Option<u64> {
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

#[cfg(test)]
mod peak_rss_parser_tests {
    use super::{parse_bsd_time_peak_rss, parse_gnu_time_peak_rss};

    #[test]
    fn parses_bsd_time_bytes_without_confusing_peak_footprint() {
        let output = "  123456  maximum resident set size\n  999999  peak memory footprint\n";
        assert_eq!(parse_bsd_time_peak_rss(output), Some(123_456));
    }

    #[test]
    fn parses_gnu_time_kibibytes_as_bytes() {
        let output = "Maximum resident set size (kbytes): 12345\n";
        assert_eq!(parse_gnu_time_peak_rss(output), Some(12_641_280));
    }

    #[test]
    fn rejects_missing_duplicate_non_numeric_and_overflowing_values() {
        assert_eq!(parse_bsd_time_peak_rss("peak memory footprint: 1\n"), None);
        assert_eq!(
            parse_bsd_time_peak_rss("1 maximum resident set size\n2 maximum resident set size\n"),
            None
        );
        assert_eq!(parse_gnu_time_peak_rss("Maximum resident set size (kbytes): unknown\n"), None);
        assert_eq!(
            parse_gnu_time_peak_rss(&format!("Maximum resident set size (kbytes): {}\n", u64::MAX)),
            None
        );
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProductProcessError {
    #[error("{product:?} binary path must be absolute: {path}")]
    BinaryPathMustBeAbsolute { product: ProductId, path: PathBuf },
    #[error("failed to read {product:?} binary {path}: {source}")]
    ReadBinary { product: ProductId, path: PathBuf, source: std::io::Error },
    #[error("failed to spawn {product:?} binary {path}: {source}")]
    Spawn { product: ProductId, path: PathBuf, source: std::io::Error },
    #[error("invalid process timeout {value:?} for {product:?}: {reason}")]
    InvalidProcessTimeout { product: ProductId, value: String, reason: String },
    #[error("{product:?} --version failed for {path} with status {status:?}: {stderr}")]
    VersionCommand { product: ProductId, path: PathBuf, status: Option<i32>, stderr: String },
    #[error("{product:?} --version produced no output")]
    EmptyVersionOutput { product: ProductId },
    #[error("{product:?} requires exact version {required}, detected {detected:?} from: {output}")]
    VersionMismatch {
        product: ProductId,
        required: String,
        detected: Option<String>,
        output: String,
    },
    #[error("failed to run Go build-info probe {helper} for {product:?}: {source}")]
    GoBuildInfoSpawn { product: ProductId, helper: PathBuf, source: std::io::Error },
    #[error("Go build-info probe {helper} failed for {product:?} with status {status:?}: {stderr}")]
    GoBuildInfoCommand { product: ProductId, helper: PathBuf, status: Option<i32>, stderr: String },
    #[error(
        "Go build info for {product:?} does not contain {expected_module} {expected_version}: {output}"
    )]
    GoBuildInfoModule {
        product: ProductId,
        expected_module: &'static str,
        expected_version: &'static str,
        output: String,
    },
    #[error("{environment} is set to an empty path for {product:?}")]
    EmptyConfiguredPath { product: ProductId, environment: &'static str },
    #[error("a companion plugin is not applicable to {product:?}")]
    CompanionNotApplicable { product: ProductId },
    #[error("{product:?} companion path must be absolute: {path}")]
    CompanionPathMustBeAbsolute { product: ProductId, path: PathBuf },
    #[error("failed to read {product:?} companion {path}: {source}")]
    ReadCompanion { product: ProductId, path: PathBuf, source: std::io::Error },
    #[error("{product:?} companion package at {path} is invalid: {detail}")]
    InvalidCompanionPackage { product: ProductId, path: PathBuf, detail: String },
    #[error(
        "{product:?} companion requires exact version {required}, detected {detected} in {path}"
    )]
    CompanionVersionMismatch {
        product: ProductId,
        required: &'static str,
        detected: String,
        path: PathBuf,
    },
    #[error("{product:?} requires companion path environment {environment}")]
    MissingCompanion { product: ProductId, environment: &'static str },
    #[error("{product:?} does not support the {operation:?} product operation")]
    UnsupportedOperation { product: ProductId, operation: ProductOperation },
    #[error("{product:?} does not declare TOML {version} support")]
    UnsupportedTomlVersion { product: ProductId, version: TomlVersion },
    #[error("failed to prepare {product:?} isolation at {path}: {source}")]
    Isolation { product: ProductId, path: PathBuf, source: std::io::Error },
    #[error("failed to write {product:?} stdin: {source}")]
    WriteStdin { product: ProductId, source: std::io::Error },
    #[error("spawned {product:?} process did not expose its stdin pipe")]
    MissingStdinPipe { product: ProductId },
    #[error("spawned {product:?} process did not expose its {stream} pipe")]
    MissingOutputPipe { product: ProductId, stream: &'static str },
    #[error("{product:?} stdin writer thread panicked")]
    WriterPanicked { product: ProductId },
    #[error("{product:?} {stream} reader thread panicked")]
    ReaderPanicked { product: ProductId, stream: &'static str },
    #[error("failed to read {product:?} {stream}: {source}")]
    ReadOutput { product: ProductId, stream: &'static str, source: std::io::Error },
    #[error("failed to wait for {product:?}: {source}")]
    Wait { product: ProductId, source: std::io::Error },
    #[error("failed to terminate timed-out {product:?} process tree: {source}")]
    TerminateTimedOut { product: ProductId, source: std::io::Error },
    #[error(
        "{product:?} {operation:?} timed out after {timeout_millis} ms; captured stderr: {stderr}"
    )]
    ProcessTimedOut {
        product: ProductId,
        operation: ProductOperation,
        timeout_millis: u64,
        stderr: String,
    },
    #[error("{TIME_BINARY_ENV} must be an absolute path: {path}")]
    TimePathMustBeAbsolute { path: PathBuf },
    #[error("failed to create or read peak RSS statistics for {product:?} at {path}: {source}")]
    MemoryStatsFile { product: ProductId, path: PathBuf, source: std::io::Error },
    #[error("peak RSS measurement is supported only on macOS and Linux")]
    PeakRssUnsupportedPlatform,
    #[error("resource meter did not report peak RSS for {product:?}: {output}")]
    MissingPeakRss { product: ProductId, output: String },
    #[error("{product:?} {operation:?} exited with status {status:?}: {stderr}")]
    CommandFailed {
        product: ProductId,
        operation: ProductOperation,
        status: Option<i32>,
        stderr: String,
    },
}

fn read_stream(mut stream: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    stream.read_to_end(&mut output)?;
    Ok(output)
}

fn configured_process_timeout(product: ProductId) -> Result<Duration, ProductProcessError> {
    let value = match std::env::var(PROCESS_TIMEOUT_ENV) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => {
            return Ok(Duration::from_secs(DEFAULT_PROCESS_TIMEOUT_SECONDS));
        }
        Err(error) => {
            return Err(ProductProcessError::InvalidProcessTimeout {
                product,
                value: String::new(),
                reason: error.to_string(),
            });
        }
    };
    let seconds =
        value.parse::<u64>().map_err(|error| ProductProcessError::InvalidProcessTimeout {
            product,
            value: value.clone(),
            reason: error.to_string(),
        })?;
    if seconds == 0 {
        return Err(ProductProcessError::InvalidProcessTimeout {
            product,
            value,
            reason: "must be greater than zero".to_owned(),
        });
    }
    Ok(Duration::from_secs(seconds))
}

const fn descriptor(product_id: ProductId) -> ProductDescriptor {
    match product_id {
        ProductId::TomlSmith => PRODUCT_CATALOG[0],
        ProductId::Tombi => PRODUCT_CATALOG[1],
        ProductId::Taplo => PRODUCT_CATALOG[2],
        ProductId::Prettier => PRODUCT_CATALOG[3],
        ProductId::Dprint => PRODUCT_CATALOG[4],
        ProductId::BurntSushiToml => PRODUCT_CATALOG[5],
        ProductId::GoTomlTomll => PRODUCT_CATALOG[6],
    }
}

/// The version a probed executable must report: the catalog pin, unless the `TomlSmith` override
/// environment selects another string or disables the check with `any`.
fn expected_version(product_id: ProductId, descriptor: ProductDescriptor) -> Option<String> {
    if product_id != ProductId::TomlSmith {
        return Some(descriptor.required_version.to_owned());
    }
    match std::env::var(TOMLSMITH_EXPECTED_VERSION_ENV) {
        Ok(value) if value.trim().eq_ignore_ascii_case("any") => None,
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => Some(descriptor.required_version.to_owned()),
    }
}

const fn go_module_requirement(product_id: ProductId) -> Option<(&'static str, &'static str)> {
    match product_id {
        ProductId::BurntSushiToml => Some(("github.com/BurntSushi/toml", "v1.6.0")),
        ProductId::GoTomlTomll => Some(("github.com/pelletier/go-toml/v2", "v2.4.3")),
        ProductId::TomlSmith
        | ProductId::Tombi
        | ProductId::Taplo
        | ProductId::Prettier
        | ProductId::Dprint => None,
    }
}

fn probe_go_build_info(
    product_id: ProductId,
    binary: &Path,
    helper: &Path,
    expected_module: &'static str,
    expected_version: &'static str,
) -> Result<String, ProductProcessError> {
    let output =
        Command::new(helper).args(["version", "-m"]).arg(binary).output().map_err(|source| {
            ProductProcessError::GoBuildInfoSpawn {
                product: product_id,
                helper: helper.to_path_buf(),
                source,
            }
        })?;
    if !output.status.success() {
        return Err(ProductProcessError::GoBuildInfoCommand {
            product: product_id,
            helper: helper.to_path_buf(),
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let version_output = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let expected = format!("\tmod\t{expected_module}\t{expected_version}\t");
    if !version_output.contains(&expected) {
        return Err(ProductProcessError::GoBuildInfoModule {
            product: product_id,
            expected_module,
            expected_version,
            output: version_output,
        });
    }
    Ok(version_output)
}

fn inspect_prettier_plugin(
    product_id: ProductId,
    configured_path: &Path,
) -> Result<PrettierPluginIdentity, ProductProcessError> {
    let canonical_path = canonical_companion_path(product_id, configured_path)?;
    let metadata = companion_metadata(product_id, &canonical_path)?;
    let package_json_path = prettier_package_json_path(product_id, &canonical_path, &metadata)?;
    let package_json =
        fs::read(&package_json_path).map_err(|source| ProductProcessError::ReadCompanion {
            product: product_id,
            path: package_json_path.clone(),
            source,
        })?;
    let manifest: serde_json::Value = serde_json::from_slice(&package_json).map_err(|source| {
        ProductProcessError::InvalidCompanionPackage {
            product: product_id,
            path: package_json_path.clone(),
            detail: format!("package.json is not valid JSON: {source}"),
        }
    })?;
    let package_name = prettier_manifest_string(product_id, &package_json_path, &manifest, "name")?;
    if package_name != "prettier-plugin-toml" {
        return Err(invalid_companion(
            product_id,
            package_json_path,
            format!("expected package name prettier-plugin-toml, detected {package_name}"),
        ));
    }
    let version =
        prettier_manifest_string(product_id, &package_json_path, &manifest, "version")?.to_owned();
    if version != PRETTIER_PLUGIN_REQUIRED_VERSION {
        return Err(ProductProcessError::CompanionVersionMismatch {
            product: product_id,
            required: PRETTIER_PLUGIN_REQUIRED_VERSION,
            detected: version,
            path: package_json_path,
        });
    }
    let entry_path = resolve_prettier_entry(
        product_id,
        &canonical_path,
        &metadata,
        &package_json_path,
        &manifest,
    )?;
    let entry = fs::read(&entry_path).map_err(|source| ProductProcessError::ReadCompanion {
        product: product_id,
        path: entry_path.clone(),
        source,
    })?;

    Ok(PrettierPluginIdentity {
        entry_path,
        entry_sha256: format!("{:x}", Sha256::digest(entry)),
        package_json_path,
        package_json_sha256: format!("{:x}", Sha256::digest(package_json)),
        version,
    })
}

fn canonical_companion_path(
    product_id: ProductId,
    path: &Path,
) -> Result<PathBuf, ProductProcessError> {
    fs::canonicalize(path).map_err(|source| ProductProcessError::ReadCompanion {
        product: product_id,
        path: path.to_path_buf(),
        source,
    })
}

fn companion_metadata(
    product_id: ProductId,
    path: &Path,
) -> Result<fs::Metadata, ProductProcessError> {
    fs::metadata(path).map_err(|source| ProductProcessError::ReadCompanion {
        product: product_id,
        path: path.to_path_buf(),
        source,
    })
}

fn prettier_package_json_path(
    product_id: ProductId,
    configured_path: &Path,
    metadata: &fs::Metadata,
) -> Result<PathBuf, ProductProcessError> {
    let candidate = if metadata.is_dir() {
        configured_path.join("package.json")
    } else if metadata.is_file() {
        configured_path
            .parent()
            .and_then(|parent| {
                parent
                    .ancestors()
                    .map(|ancestor| ancestor.join("package.json"))
                    .find(|path| path.is_file())
            })
            .ok_or_else(|| {
                invalid_companion(
                    product_id,
                    configured_path.to_path_buf(),
                    "no containing package.json was found",
                )
            })?
    } else {
        return Err(invalid_companion(
            product_id,
            configured_path.to_path_buf(),
            "the configured path is neither a package directory nor an entry file",
        ));
    };
    canonical_companion_path(product_id, &candidate)
}

fn prettier_manifest_string<'a>(
    product_id: ProductId,
    package_json_path: &Path,
    manifest: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, ProductProcessError> {
    manifest.get(field).and_then(serde_json::Value::as_str).ok_or_else(|| {
        invalid_companion(
            product_id,
            package_json_path.to_path_buf(),
            format!("package.json does not contain a string {field}"),
        )
    })
}

fn resolve_prettier_entry(
    product_id: ProductId,
    configured_path: &Path,
    metadata: &fs::Metadata,
    package_json_path: &Path,
    manifest: &serde_json::Value,
) -> Result<PathBuf, ProductProcessError> {
    let package_root = package_json_path.parent().ok_or_else(|| {
        invalid_companion(product_id, package_json_path.to_path_buf(), "package.json has no parent")
    })?;
    let entry_path = if metadata.is_file() {
        configured_path.to_path_buf()
    } else {
        let entry = manifest
            .get("module")
            .and_then(serde_json::Value::as_str)
            .or_else(|| manifest.get("main").and_then(serde_json::Value::as_str))
            .ok_or_else(|| {
                invalid_companion(
                    product_id,
                    package_json_path.to_path_buf(),
                    "package.json contains neither a string module nor main entry",
                )
            })?;
        canonical_companion_path(product_id, &package_root.join(entry))?
    };
    if !entry_path.starts_with(package_root)
        || !matches!(
            entry_path.extension().and_then(std::ffi::OsStr::to_str),
            Some("js" | "cjs" | "mjs")
        )
    {
        return Err(invalid_companion(
            product_id,
            entry_path,
            "the resolved JavaScript entry must stay inside the package directory",
        ));
    }
    Ok(entry_path)
}

fn invalid_companion(
    product: ProductId,
    path: PathBuf,
    detail: impl Into<String>,
) -> ProductProcessError {
    ProductProcessError::InvalidCompanionPackage { product, path, detail: detail.into() }
}

fn extract_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|word| {
        let candidate = word.trim_start_matches('v').trim_matches(|character: char| {
            !character.is_ascii_alphanumeric()
                && character != '.'
                && character != '-'
                && character != '+'
        });
        (candidate.chars().next().is_some_and(|character| character.is_ascii_digit())
            && candidate.contains('.'))
        .then(|| candidate.to_owned())
    })
}

const fn version_directory(version: TomlVersion) -> &'static str {
    match version {
        TomlVersion::V1_0 => "toml-1.0",
        TomlVersion::V1_1 => "toml-1.1",
    }
}

const fn version_config_label(version: TomlVersion) -> &'static str {
    match version {
        TomlVersion::V1_0 => "v1.0.0",
        TomlVersion::V1_1 => "v1.1.0",
    }
}

const fn version_cli_label(version: TomlVersion) -> &'static str {
    match version {
        TomlVersion::V1_0 => "1.0",
        TomlVersion::V1_1 => "1.1",
    }
}
