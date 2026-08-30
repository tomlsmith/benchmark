use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::TomlVersion;

const MANIFEST_PATH: &str = "fixtures/manifest.json";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureClass {
    Small,
    Medium,
    Large,
    Xlarge,
    Engineering,
    Edge,
    Invalid,
}

impl FixtureClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Xlarge => "xlarge",
            Self::Engineering => "engineering",
            Self::Edge => "edge",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadShape {
    SpecificationMix,
    StructuredScaling,
    StressMix,
    LockfilePackages,
    WorkspaceManifest,
    ApplicationConfig,
    Crlf,
    InvalidTruncatedArray,
    VersionBoundaryEscapeE,
    VersionBoundaryHexEscape,
    VersionBoundaryOmittedSeconds,
    VersionBoundaryInlineTable,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixturePurpose {
    Headline,
    Scaling,
    Diagnostic,
    Stress,
    Correctness,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxProfile {
    CommonSubset,
    Toml11Native,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatState {
    Formatted,
    Edited,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    SpecificationGenerated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FixtureManifestEntry {
    pub id: String,
    pub toml_version: TomlVersion,
    pub class: FixtureClass,
    pub workload_shape: WorkloadShape,
    pub purpose: FixturePurpose,
    pub syntax_profile: SyntaxProfile,
    pub format_state: FormatState,
    pub source_kind: SourceKind,
    pub tags: Vec<String>,
    pub path: String,
    pub expected_valid: bool,
    pub bytes: usize,
    pub lines: usize,
    pub sha256: String,
    pub provenance: String,
    pub source_url: Option<String>,
    pub source_revision: Option<String>,
    pub license: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FixtureManifest {
    pub fixtures: Vec<FixtureManifestEntry>,
}

#[derive(Clone, Debug)]
pub struct Fixture {
    pub id: String,
    pub toml_version: TomlVersion,
    pub class: FixtureClass,
    pub workload_shape: WorkloadShape,
    pub purpose: FixturePurpose,
    pub syntax_profile: SyntaxProfile,
    pub format_state: FormatState,
    pub source_kind: SourceKind,
    pub tags: Vec<String>,
    pub relative_path: PathBuf,
    pub expected_valid: bool,
    pub bytes: usize,
    pub lines: usize,
    pub sha256: String,
    pub provenance: String,
    pub source_url: Option<String>,
    pub source_revision: Option<String>,
    pub license: String,
    source: String,
}

impl Fixture {
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

#[derive(Clone, Debug)]
pub struct FixtureCorpus {
    root: PathBuf,
    manifest: FixtureManifest,
    fixtures: Vec<Fixture>,
}

impl FixtureCorpus {
    /// Loads the checked-in corpus and verifies every declared byte count and SHA-256 checksum.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures, malformed manifests, unsafe paths, duplicate entries,
    /// missing version/class coverage, invalid UTF-8, or integrity mismatches.
    #[allow(clippy::too_many_lines)]
    pub fn load(root: impl AsRef<Path>) -> Result<Self, CorpusError> {
        let root = root.as_ref().to_path_buf();
        let manifest_path = root.join(MANIFEST_PATH);
        let manifest_bytes = read(&manifest_path)?;
        let manifest = serde_json::from_slice::<FixtureManifest>(&manifest_bytes)
            .map_err(|source| CorpusError::ManifestJson { path: manifest_path, source })?;
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        let mut classes = HashSet::new();
        let mut version_classes = HashSet::new();
        let mut fixtures = Vec::with_capacity(manifest.fixtures.len());
        for entry in &manifest.fixtures {
            let relative_path = PathBuf::from(&entry.path);
            if !is_safe_relative_path(&relative_path) {
                return Err(CorpusError::UnsafePath(entry.path.clone()));
            }
            if !ids.insert(entry.id.clone()) {
                return Err(CorpusError::DuplicateId(entry.id.clone()));
            }
            if !paths.insert(entry.path.clone()) {
                return Err(CorpusError::DuplicatePath(entry.path.clone()));
            }
            let mut tags = HashSet::new();
            for tag in &entry.tags {
                if tag.is_empty()
                    || !tag.chars().all(|character| {
                        character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || matches!(character, '-' | '_')
                    })
                {
                    return Err(CorpusError::InvalidTag {
                        fixture: entry.id.clone(),
                        tag: tag.clone(),
                    });
                }
                if !tags.insert(tag) {
                    return Err(CorpusError::DuplicateTag {
                        fixture: entry.id.clone(),
                        tag: tag.clone(),
                    });
                }
            }
            if entry.tags.is_empty() {
                return Err(CorpusError::MissingTags(entry.id.clone()));
            }
            classes.insert(entry.class);
            if entry.expected_valid {
                version_classes.insert((entry.toml_version, entry.class));
            }

            let fixture_path = root.join(&relative_path);
            let bytes = read(&fixture_path)?;
            if bytes.len() != entry.bytes {
                return Err(CorpusError::ByteCount {
                    path: entry.path.clone(),
                    expected: entry.bytes,
                    actual: bytes.len(),
                });
            }
            let checksum = sha256(&bytes);
            if checksum != entry.sha256 {
                return Err(CorpusError::Checksum {
                    path: entry.path.clone(),
                    expected: entry.sha256.clone(),
                    actual: checksum,
                });
            }
            let source = String::from_utf8(bytes)
                .map_err(|source| CorpusError::InvalidUtf8 { path: entry.path.clone(), source })?;
            let lines = source.lines().count();
            if lines != entry.lines {
                return Err(CorpusError::LineCount {
                    path: entry.path.clone(),
                    expected: entry.lines,
                    actual: lines,
                });
            }
            fixtures.push(Fixture {
                id: entry.id.clone(),
                toml_version: entry.toml_version,
                class: entry.class,
                workload_shape: entry.workload_shape,
                purpose: entry.purpose,
                syntax_profile: entry.syntax_profile,
                format_state: entry.format_state,
                source_kind: entry.source_kind,
                tags: entry.tags.clone(),
                relative_path,
                expected_valid: entry.expected_valid,
                bytes: entry.bytes,
                lines: entry.lines,
                sha256: entry.sha256.clone(),
                provenance: entry.provenance.clone(),
                source_url: entry.source_url.clone(),
                source_revision: entry.source_revision.clone(),
                license: entry.license.clone(),
                source,
            });
        }

        for class in [
            FixtureClass::Small,
            FixtureClass::Medium,
            FixtureClass::Large,
            FixtureClass::Xlarge,
            FixtureClass::Engineering,
            FixtureClass::Edge,
            FixtureClass::Invalid,
        ] {
            if !classes.contains(&class) {
                return Err(CorpusError::MissingClass(class));
            }
        }
        for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
            for class in [
                FixtureClass::Small,
                FixtureClass::Medium,
                FixtureClass::Large,
                FixtureClass::Xlarge,
            ] {
                if !version_classes.contains(&(version, class)) {
                    return Err(CorpusError::MissingVersionClass { version, class });
                }
            }
        }
        for fixture in fixtures.iter().filter(|fixture| fixture.class == FixtureClass::Xlarge) {
            if fixture.bytes < 10 * 1024 * 1024 {
                return Err(CorpusError::XlargeTooSmall {
                    fixture: fixture.id.clone(),
                    bytes: fixture.bytes,
                });
            }
        }
        Ok(Self { root, manifest, fixtures })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub const fn manifest(&self) -> &FixtureManifest {
        &self.manifest
    }

    #[must_use]
    pub fn fixtures(&self) -> &[Fixture] {
        &self.fixtures
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    #[error("failed to read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("failed to write {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("invalid corpus manifest {path}: {source}")]
    ManifestJson { path: PathBuf, source: serde_json::Error },
    #[error("failed to serialize the generated corpus manifest: {0}")]
    ManifestSerialize(serde_json::Error),
    #[error("fixture path is not a safe relative path: {0}")]
    UnsafePath(String),
    #[error("duplicate fixture id: {0}")]
    DuplicateId(String),
    #[error("duplicate fixture path: {0}")]
    DuplicatePath(String),
    #[error("fixture {fixture} has invalid workload tag {tag:?}")]
    InvalidTag { fixture: String, tag: String },
    #[error("fixture {fixture} has duplicate workload tag {tag:?}")]
    DuplicateTag { fixture: String, tag: String },
    #[error("fixture has no workload tags: {0}")]
    MissingTags(String),
    #[error("required fixture class is missing: {0:?}")]
    MissingClass(FixtureClass),
    #[error("required valid fixture is missing for TOML {version} {class:?}")]
    MissingVersionClass { version: TomlVersion, class: FixtureClass },
    #[error("xlarge fixture {fixture} is only {bytes} bytes; at least 10 MiB is required")]
    XlargeTooSmall { fixture: String, bytes: usize },
    #[error("byte count mismatch for {path}: expected {expected}, got {actual}")]
    ByteCount { path: String, expected: usize, actual: usize },
    #[error("line count mismatch for {path}: expected {expected}, got {actual}")]
    LineCount { path: String, expected: usize, actual: usize },
    #[error("checksum mismatch for {path}: expected {expected}, got {actual}")]
    Checksum { path: String, expected: String, actual: String },
    #[error("fixture {path} is not UTF-8: {source}")]
    InvalidUtf8 { path: String, source: std::string::FromUtf8Error },
    #[error("checked-in corpus does not match the deterministic generator")]
    GeneratedMismatch,
}

/// Writes the deterministic project-authored TOML 1.0/1.1 corpus and integrity manifest.
///
/// # Errors
///
/// Returns an error when a directory or file cannot be written or the generated manifest cannot
/// be encoded as JSON.
pub fn generate_corpus(root: impl AsRef<Path>) -> Result<FixtureManifest, CorpusError> {
    let root = root.as_ref();
    let generated = generated_sources();

    for fixture in &generated {
        let path = root.join(fixture.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|source| CorpusError::Write { path: parent.to_path_buf(), source })?;
        }
        fs::write(&path, fixture.source.as_bytes())
            .map_err(|source| CorpusError::Write { path: path.clone(), source })?;
    }

    let manifest = generated_manifest(&generated);
    let manifest_path = root.join(MANIFEST_PATH);
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(CorpusError::ManifestSerialize)?;
    fs::write(&manifest_path, [bytes.as_slice(), b"\n"].concat())
        .map_err(|source| CorpusError::Write { path: manifest_path, source })?;
    Ok(manifest)
}

/// Verifies that the checked-in corpus is byte-for-byte equivalent to the generator.
///
/// # Errors
///
/// Returns a corpus integrity error or [`CorpusError::GeneratedMismatch`] when the checked-in
/// manifest does not describe the deterministic generated sources.
pub fn check_generated_corpus(root: impl AsRef<Path>) -> Result<FixtureManifest, CorpusError> {
    let corpus = FixtureCorpus::load(root)?;
    let expected = generated_manifest(&generated_sources());
    if corpus.manifest() != &expected {
        return Err(CorpusError::GeneratedMismatch);
    }
    Ok(expected)
}

#[derive(Debug)]
struct GeneratedFixture {
    id: &'static str,
    version: TomlVersion,
    class: FixtureClass,
    workload_shape: WorkloadShape,
    purpose: FixturePurpose,
    syntax_profile: SyntaxProfile,
    format_state: FormatState,
    source_kind: SourceKind,
    tags: &'static [&'static str],
    relative_path: &'static str,
    expected_valid: bool,
    provenance: &'static str,
    source_url: Option<&'static str>,
    source_revision: Option<&'static str>,
    license: &'static str,
    source: String,
}

fn generated_sources() -> Vec<GeneratedFixture> {
    let mut fixtures = Vec::with_capacity(19);
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        fixtures.extend([
            generated_valid(version, FixtureClass::Small),
            generated_valid(version, FixtureClass::Medium),
            generated_valid(version, FixtureClass::Large),
            generated_valid(version, FixtureClass::Xlarge),
            generated_crlf(version),
        ]);
    }
    fixtures.extend(generated_engineering());
    fixtures.extend([
        generated_invalid(
            "v1_0_invalid",
            TomlVersion::V1_0,
            WorkloadShape::InvalidTruncatedArray,
            &["invalid", "truncated-array", "correctness-only"],
            "fixtures/v1.0/invalid/truncated-array.toml",
            "title = \"invalid fixture\"\nvalues = [1, 2\n",
        ),
        generated_invalid(
            "v1_1_invalid",
            TomlVersion::V1_1,
            WorkloadShape::InvalidTruncatedArray,
            &["invalid", "truncated-array", "correctness-only"],
            "fixtures/v1.1/invalid/truncated-array.toml",
            "title = \"invalid fixture\"\nvalues = [1, 2\n",
        ),
        generated_invalid(
            "v1_0_boundary_escape_e",
            TomlVersion::V1_0,
            WorkloadShape::VersionBoundaryEscapeE,
            &["invalid", "version-boundary", "escape-e", "correctness-only"],
            "fixtures/v1.0/invalid/toml-1.1-escape-e.toml",
            "value = \"\\e\"\n",
        ),
        generated_invalid(
            "v1_0_boundary_hex_escape",
            TomlVersion::V1_0,
            WorkloadShape::VersionBoundaryHexEscape,
            &["invalid", "version-boundary", "hex-escape", "correctness-only"],
            "fixtures/v1.0/invalid/toml-1.1-hex-escape.toml",
            "value = \"\\x41\"\n",
        ),
        generated_invalid(
            "v1_0_boundary_omitted_seconds",
            TomlVersion::V1_0,
            WorkloadShape::VersionBoundaryOmittedSeconds,
            &["invalid", "version-boundary", "omitted-seconds", "correctness-only"],
            "fixtures/v1.0/invalid/toml-1.1-omitted-seconds.toml",
            "value = 07:32\n",
        ),
        generated_invalid(
            "v1_0_boundary_inline_table",
            TomlVersion::V1_0,
            WorkloadShape::VersionBoundaryInlineTable,
            &[
                "invalid",
                "version-boundary",
                "multiline-inline-table",
                "trailing-comma",
                "correctness-only",
            ],
            "fixtures/v1.0/invalid/toml-1.1-inline-table.toml",
            "value = {\n  key = \"value\",\n}\n",
        ),
    ]);
    fixtures
}

#[allow(clippy::too_many_lines)]
fn generated_valid(version: TomlVersion, class: FixtureClass) -> GeneratedFixture {
    let (id, relative_path, workload_shape, purpose, format_state, tags, target_bytes, crlf) =
        match (version, class) {
            (TomlVersion::V1_0, FixtureClass::Small) => (
                "v1_0_small",
                "fixtures/v1.0/small/specification.toml",
                WorkloadShape::SpecificationMix,
                FixturePurpose::Headline,
                FormatState::Formatted,
                &["benchmark", "syntax-mix", "latency"] as &[_],
                4 * 1024,
                false,
            ),
            (TomlVersion::V1_0, FixtureClass::Medium) => (
                "v1_0_medium",
                "fixtures/v1.0/medium/structured.toml",
                WorkloadShape::StructuredScaling,
                FixturePurpose::Scaling,
                FormatState::Edited,
                &["benchmark", "syntax-mix", "throughput"] as &[_],
                128 * 1024,
                false,
            ),
            (TomlVersion::V1_0, FixtureClass::Large) => (
                "v1_0_large",
                "fixtures/v1.0/large/structured.toml",
                WorkloadShape::StructuredScaling,
                FixturePurpose::Scaling,
                FormatState::Edited,
                &["benchmark", "syntax-mix", "throughput"] as &[_],
                1024 * 1024,
                false,
            ),
            (TomlVersion::V1_0, FixtureClass::Xlarge) => (
                "v1_0_stress",
                "fixtures/v1.0/xlarge/stress.toml",
                WorkloadShape::StressMix,
                FixturePurpose::Stress,
                FormatState::Edited,
                &["benchmark", "syntax-mix", "stress"] as &[_],
                10 * 1024 * 1024,
                false,
            ),
            (TomlVersion::V1_1, FixtureClass::Small) => (
                "v1_1_small",
                "fixtures/v1.1/small/specification.toml",
                WorkloadShape::SpecificationMix,
                FixturePurpose::Headline,
                FormatState::Formatted,
                &["benchmark", "syntax-mix", "toml-1-1", "latency"] as &[_],
                4 * 1024,
                false,
            ),
            (TomlVersion::V1_1, FixtureClass::Medium) => (
                "v1_1_medium",
                "fixtures/v1.1/medium/structured.toml",
                WorkloadShape::StructuredScaling,
                FixturePurpose::Scaling,
                FormatState::Edited,
                &["benchmark", "syntax-mix", "toml-1-1", "throughput"] as &[_],
                128 * 1024,
                false,
            ),
            (TomlVersion::V1_1, FixtureClass::Large) => (
                "v1_1_large",
                "fixtures/v1.1/large/structured.toml",
                WorkloadShape::StructuredScaling,
                FixturePurpose::Scaling,
                FormatState::Edited,
                &["benchmark", "syntax-mix", "toml-1-1", "throughput"] as &[_],
                1024 * 1024,
                false,
            ),
            (TomlVersion::V1_1, FixtureClass::Xlarge) => (
                "v1_1_stress",
                "fixtures/v1.1/xlarge/stress.toml",
                WorkloadShape::StressMix,
                FixturePurpose::Stress,
                FormatState::Edited,
                &["benchmark", "syntax-mix", "toml-1-1", "stress"] as &[_],
                10 * 1024 * 1024,
                false,
            ),
            (_, FixtureClass::Engineering | FixtureClass::Edge | FixtureClass::Invalid) => {
                unreachable!(
                    "engineering, diagnostic and invalid fixtures use dedicated generators"
                )
            }
        };

    GeneratedFixture {
        id,
        version,
        class,
        workload_shape,
        purpose,
        syntax_profile: if version == TomlVersion::V1_1 {
            SyntaxProfile::Toml11Native
        } else {
            SyntaxProfile::CommonSubset
        },
        format_state,
        source_kind: SourceKind::SpecificationGenerated,
        tags,
        relative_path,
        expected_valid: true,
        provenance: "deterministic synthetic corpus generated from the published TOML specification; no external corpus bytes",
        source_url: None,
        source_revision: None,
        license: "MIT",
        source: scaled_specification_fixture(version, target_bytes, crlf, format_state),
    }
}

fn generated_crlf(version: TomlVersion) -> GeneratedFixture {
    let (id, relative_path) = match version {
        TomlVersion::V1_0 => ("v1_0_crlf", "fixtures/v1.0/edge/crlf.toml"),
        TomlVersion::V1_1 => ("v1_1_crlf", "fixtures/v1.1/edge/crlf.toml"),
    };
    GeneratedFixture {
        id,
        version,
        class: FixtureClass::Edge,
        workload_shape: WorkloadShape::Crlf,
        purpose: FixturePurpose::Diagnostic,
        syntax_profile: if version == TomlVersion::V1_1 {
            SyntaxProfile::Toml11Native
        } else {
            SyntaxProfile::CommonSubset
        },
        format_state: FormatState::Formatted,
        source_kind: SourceKind::SpecificationGenerated,
        tags: &["diagnostic", "crlf", "line-endings"],
        relative_path,
        expected_valid: true,
        provenance: "deterministic CRLF diagnostic generated from the published TOML specification; no external corpus bytes",
        source_url: None,
        source_revision: None,
        license: "MIT",
        source: crlf_diagnostic_fixture(version),
    }
}

const ENGINEERING_PROVENANCE: &str = "deterministic synthetic engineering-shaped corpus generated from the published TOML specification; no external corpus bytes";

fn generated_engineering() -> [GeneratedFixture; 3] {
    let build = |id: &'static str,
                 workload_shape: WorkloadShape,
                 tags: &'static [&'static str],
                 relative_path: &'static str,
                 source: String| GeneratedFixture {
        id,
        version: TomlVersion::V1_0,
        class: FixtureClass::Engineering,
        workload_shape,
        purpose: FixturePurpose::Headline,
        syntax_profile: SyntaxProfile::CommonSubset,
        format_state: FormatState::Edited,
        source_kind: SourceKind::SpecificationGenerated,
        tags,
        relative_path,
        expected_valid: true,
        provenance: ENGINEERING_PROVENANCE,
        source_url: None,
        source_revision: None,
        license: "MIT",
        source,
    };
    [
        build(
            "v1_0_cargo_lock",
            WorkloadShape::LockfilePackages,
            &["benchmark", "engineering", "lockfile", "throughput"],
            "fixtures/v1.0/engineering/cargo-lock.toml",
            cargo_lock_fixture(),
        ),
        build(
            "v1_0_workspace_manifest",
            WorkloadShape::WorkspaceManifest,
            &["benchmark", "engineering", "workspace-manifest", "inline-tables"],
            "fixtures/v1.0/engineering/workspace-manifest.toml",
            workspace_manifest_fixture(),
        ),
        build(
            "v1_0_app_config",
            WorkloadShape::ApplicationConfig,
            &["benchmark", "engineering", "app-config", "unicode", "comments"],
            "fixtures/v1.0/engineering/app-config.toml",
            app_config_fixture(),
        ),
    ]
}

/// Deterministic light formatting drift shared by the engineering fixtures: every 7th
/// assignment drops the spaces around `=`, every 11th doubles them, and every 13th
/// (offset by 5) leaves trailing spaces, so the `format` lane always has real work.
/// The drift never changes TOML semantics and never touches multi-line strings.
#[derive(Debug)]
struct FormatDrift {
    emitted: usize,
}

impl FormatDrift {
    const fn new() -> Self {
        Self { emitted: 0 }
    }

    fn assignment(&mut self, source: &mut String, key: &str, rendered_value: &str) {
        self.emitted += 1;
        let separator = if self.emitted.is_multiple_of(7) {
            "="
        } else if self.emitted.is_multiple_of(11) {
            "  =  "
        } else {
            " = "
        };
        source.push_str(key);
        source.push_str(separator);
        source.push_str(rendered_value);
        if self.emitted % 13 == 5 {
            source.push_str("  ");
        }
        source.push('\n');
    }
}

const ENGINEERING_NAME_STEMS: [&str; 20] = [
    "serde",
    "tokio",
    "hyper",
    "tracing",
    "futures",
    "axum",
    "tower",
    "clap",
    "regex",
    "rayon",
    "anyhow",
    "thiserror",
    "reqwest",
    "quote",
    "syn",
    "indexmap",
    "hashbrown",
    "parking-lot",
    "crossbeam",
    "smallvec",
];

const ENGINEERING_NAME_SUFFIXES: [&str; 8] =
    ["core", "util", "derive", "macros", "io", "codec", "net", "sync"];

const CARGO_LOCK_PACKAGE_COUNT: usize = 1900;

fn engineering_package_name(index: usize) -> String {
    format!(
        "{}-{}-{}",
        ENGINEERING_NAME_STEMS[index % ENGINEERING_NAME_STEMS.len()],
        ENGINEERING_NAME_SUFFIXES
            [(index / ENGINEERING_NAME_STEMS.len()) % ENGINEERING_NAME_SUFFIXES.len()],
        index
    )
}

fn engineering_package_version(index: usize) -> String {
    format!("{}.{}.{}", 1 + index % 3, (index * 7) % 40, (index * 13) % 90)
}

fn engineering_checksum(index: usize) -> String {
    let seed = index as u64;
    format!(
        "{:016x}{:016x}{:016x}{:016x}",
        seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(0x243f_6a88_85a3_08d3),
        seed.wrapping_mul(0xc2b2_ae3d_27d4_eb4f).wrapping_add(0x1319_8a2e_0370_7344),
        seed.wrapping_mul(0x1656_67b1_9e37_79f9).wrapping_add(0xa409_3822_299f_31d0),
        seed.wrapping_mul(0x27d4_eb2f_1656_67c5).wrapping_add(0x082e_fa98_ec4e_6c89),
    )
}

fn cargo_lock_fixture() -> String {
    use std::fmt::Write as _;

    let mut source = String::with_capacity(520 * 1024);
    let mut drift = FormatDrift::new();
    source.push_str(
        "# Deterministic lockfile-shaped corpus generated by the TomlSmith benchmark generator.\n\
         # It mirrors the registry snapshot layout of a large generated Rust lockfile without\n\
         # copying any external bytes; every package below is synthetic.\n\
         version = 4\n",
    );
    for index in 0..CARGO_LOCK_PACKAGE_COUNT {
        source.push_str("\n[[package]]\n");
        drift.assignment(&mut source, "name", &format!("\"{}\"", engineering_package_name(index)));
        drift.assignment(
            &mut source,
            "version",
            &format!("\"{}\"", engineering_package_version(index)),
        );
        drift.assignment(
            &mut source,
            "source",
            "\"registry+https://github.com/rust-lang/crates.io-index\"",
        );
        drift.assignment(&mut source, "checksum", &format!("\"{}\"", engineering_checksum(index)));

        let dependency_count = index % 9;
        if index % 5 < 3 && dependency_count > 0 {
            let mut dependencies = (0..dependency_count)
                .map(|slot| {
                    let mut reference = (index * 37 + slot * 211 + 1) % CARGO_LOCK_PACKAGE_COUNT;
                    if reference == index {
                        reference = (reference + 1) % CARGO_LOCK_PACKAGE_COUNT;
                    }
                    engineering_package_name(reference)
                })
                .collect::<Vec<_>>();
            dependencies.sort_unstable();
            dependencies.dedup();
            if index % 3 == 1 {
                let joined = dependencies
                    .iter()
                    .map(|dependency| format!("\"{dependency}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                drift.assignment(&mut source, "dependencies", &format!("[{joined}]"));
            } else {
                source.push_str("dependencies = [\n");
                for dependency in &dependencies {
                    writeln!(source, " \"{dependency}\",")
                        .expect("writing to a String cannot fail");
                }
                source.push_str("]\n");
            }
        }
    }
    source
}

const WORKSPACE_MEMBER_COUNT: usize = 200;
const WORKSPACE_DEPENDENCY_COUNT: usize = 340;
const WORKSPACE_RELEASE_OVERRIDE_COUNT: usize = 44;
const WORKSPACE_DEV_OVERRIDE_COUNT: usize = 32;

const WORKSPACE_DOMAINS: [&str; 8] = [
    "runtime",
    "storage",
    "network",
    "observability",
    "tooling",
    "security",
    "data-plane",
    "control-plane",
];

const WORKSPACE_FEATURE_POOL: [&str; 12] = [
    "derive",
    "serde-support",
    "async-runtime",
    "tls-native-roots",
    "http2-transport",
    "tracing-log-bridge",
    "std",
    "alloc-only",
    "macros-full",
    "codec-io",
    "runtime-metrics",
    "unicode-normalization",
];

const WORKSPACE_RUST_LINTS: [&str; 18] = [
    "unsafe_code",
    "missing_docs",
    "unused_must_use",
    "unreachable_pub",
    "unused_lifetimes",
    "unused_qualifications",
    "trivial_casts",
    "trivial_numeric_casts",
    "single_use_lifetimes",
    "variant_size_differences",
    "meta_variable_misuse",
    "macro_use_extern_crate",
    "elided_lifetimes_in_paths",
    "explicit_outlives_requirements",
    "let_underscore_drop",
    "non_ascii_idents",
    "unused_extern_crates",
    "unused_import_braces",
];

const WORKSPACE_CLIPPY_LINTS: [&str; 24] = [
    "unwrap_used",
    "expect_used",
    "dbg_macro",
    "todo",
    "unimplemented",
    "print_stdout",
    "print_stderr",
    "indexing_slicing",
    "panic_in_result_fn",
    "string_slice",
    "shadow_unrelated",
    "wildcard_imports",
    "redundant_clone",
    "large_stack_arrays",
    "cognitive_complexity",
    "missing_const_for_fn",
    "cast_lossless",
    "map_unwrap_or",
    "semicolon_if_nothing_returned",
    "single_match_else",
    "inefficient_to_string",
    "needless_pass_by_value",
    "trivially_copy_pass_by_ref",
    "unnecessary_wraps",
];

#[allow(clippy::too_many_lines)]
fn workspace_manifest_fixture() -> String {
    use std::fmt::Write as _;

    let mut source = String::with_capacity(72 * 1024);
    let mut drift = FormatDrift::new();
    source.push_str(
        "# Deterministic workspace-manifest-shaped corpus generated by the TomlSmith benchmark\n\
         # generator. It mirrors a large Rust monorepo manifest — members, shared package\n\
         # metadata, pinned workspace dependencies, profiles and lint tables — without copying\n\
         # any external bytes.\n\n[workspace]\n",
    );
    drift.assignment(&mut source, "resolver", "\"2\"");
    source.push_str("members = [\n");
    for index in 0..WORKSPACE_MEMBER_COUNT {
        if index.is_multiple_of(25) {
            writeln!(source, "  # {} crates", WORKSPACE_DOMAINS[index / 25])
                .expect("writing to a String cannot fail");
        }
        writeln!(
            source,
            "  \"crates/{}/team-{:02}/{}\",",
            WORKSPACE_DOMAINS[index / 25],
            index % 12,
            engineering_package_name(index)
        )
        .expect("writing to a String cannot fail");
    }
    source.push_str("]\n");
    source.push_str(
        "# Local build shortcuts operate on the hot member subset.\ndefault-members = [\n",
    );
    for index in 0..56 {
        let member = index * 3 + 1;
        writeln!(
            source,
            "  \"crates/{}/team-{:02}/{}\",",
            WORKSPACE_DOMAINS[member / 25],
            member % 12,
            engineering_package_name(member)
        )
        .expect("writing to a String cannot fail");
    }
    source.push_str("]\n");
    drift.assignment(
        &mut source,
        "exclude",
        "[\"target\", \"vendor\", \"third-party/archived\", \"tools/scratch\"]",
    );

    source.push_str(
        "\n# Shared package metadata inherited by every member crate.\n[workspace.package]\n",
    );
    drift.assignment(&mut source, "version", "\"0.42.7\"");
    drift.assignment(&mut source, "edition", "\"2021\"");
    drift.assignment(&mut source, "rust-version", "\"1.92\"");
    drift.assignment(&mut source, "license", "\"MIT OR Apache-2.0\"");
    drift.assignment(&mut source, "authors", "[\"TomlSmith Benchmark Corpus Generator\"]");
    drift.assignment(
        &mut source,
        "repository",
        "\"https://github.com/tomlsmith/tomlsmith-benchmark\"",
    );
    drift.assignment(&mut source, "homepage", "\"https://benchmark.example.org/workspace\"");
    drift.assignment(&mut source, "documentation", "\"https://docs.example.org/workspace\"");
    drift.assignment(&mut source, "keywords", "[\"toml\", \"benchmark\", \"workspace\"]");
    drift.assignment(&mut source, "categories", "[\"development-tools\", \"parsing\"]");
    drift.assignment(&mut source, "readme", "\"README.md\"");
    drift.assignment(&mut source, "publish", "false");

    source.push_str("\n# Centrally pinned dependency versions shared across the workspace.\n[workspace.dependencies]\n");
    for index in 0..WORKSPACE_DEPENDENCY_COUNT {
        if index.is_multiple_of(6) {
            writeln!(
                source,
                "# ---- {} pin group {:02}: audited 2026-08 ----",
                WORKSPACE_DOMAINS[(index / 6) % WORKSPACE_DOMAINS.len()],
                index / 6
            )
            .expect("writing to a String cannot fail");
        }
        let name = engineering_package_name(index);
        let version = engineering_package_version(index);
        if index.is_multiple_of(2) {
            drift.assignment(&mut source, &name, &format!("\"{version}\""));
        } else {
            let feature_count = 5 + index % 7;
            let features = (0..feature_count)
                .map(|slot| {
                    format!(
                        "\"{}\"",
                        WORKSPACE_FEATURE_POOL[(index + slot * 5) % WORKSPACE_FEATURE_POOL.len()]
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let rename = if index % 8 == 3 {
                format!(", package = \"{}-impl\"", engineering_package_name(index + 1))
            } else {
                String::new()
            };
            let rendered = if index % 4 == 1 {
                format!(
                    "{{ version = \"{version}\", features = [{features}], default-features = false{rename} }}"
                )
            } else {
                format!("{{ version = \"{version}\", features = [{features}]{rename} }}")
            };
            drift.assignment(&mut source, &name, &rendered);
        }
    }

    source.push_str("\n# Release binaries ship fully optimized.\n[profile.release]\n");
    drift.assignment(&mut source, "opt-level", "3");
    drift.assignment(&mut source, "lto", "\"thin\"");
    drift.assignment(&mut source, "codegen-units", "1");
    drift.assignment(&mut source, "panic", "\"abort\"");
    drift.assignment(&mut source, "strip", "\"symbols\"");
    for index in 0..WORKSPACE_RELEASE_OVERRIDE_COUNT {
        writeln!(source, "\n[profile.release.package.{}]", engineering_package_name(index * 17))
            .expect("writing to a String cannot fail");
        drift.assignment(&mut source, "opt-level", "3");
        drift.assignment(&mut source, "debug", "false");
    }

    source.push_str("\n# Development builds trade speed for debuggability.\n[profile.dev]\n");
    drift.assignment(&mut source, "opt-level", "0");
    drift.assignment(&mut source, "debug", "true");
    drift.assignment(&mut source, "split-debuginfo", "\"unpacked\"");
    drift.assignment(&mut source, "incremental", "true");
    source.push_str("# Hot proc-macro and codegen crates stay optimized even in dev builds.\n");
    for index in 0..WORKSPACE_DEV_OVERRIDE_COUNT {
        writeln!(source, "\n[profile.dev.package.{}]", engineering_package_name(index * 23 + 2))
            .expect("writing to a String cannot fail");
        drift.assignment(&mut source, "opt-level", "2");
    }

    source.push_str("\n[profile.bench]\n");
    drift.assignment(&mut source, "opt-level", "3");
    drift.assignment(&mut source, "debug", "false");
    drift.assignment(&mut source, "codegen-units", "16");

    source.push_str("\n# Workspace-wide lint policy.\n[workspace.lints.rust]\n");
    drift.assignment(&mut source, "rust_2018_idioms", "{ level = \"warn\", priority = -1 }");
    for (index, lint) in WORKSPACE_RUST_LINTS.iter().enumerate() {
        let level = if index.is_multiple_of(3) { "\"deny\"" } else { "\"warn\"" };
        drift.assignment(&mut source, lint, level);
    }

    source.push_str("\n[workspace.lints.clippy]\n");
    drift.assignment(&mut source, "all", "{ level = \"warn\", priority = -1 }");
    drift.assignment(&mut source, "pedantic", "{ level = \"warn\", priority = -1 }");
    for (index, lint) in WORKSPACE_CLIPPY_LINTS.iter().enumerate() {
        let level = if index % 4 == 2 { "\"allow\"" } else { "\"warn\"" };
        drift.assignment(&mut source, lint, level);
    }
    source
}

const APP_CONFIG_MODULE_COUNT: usize = 64;
const APP_CONFIG_SERVICE_TIERS: usize = 5;
const APP_CONFIG_SERVICES_PER_TIER: usize = 16;
const APP_CONFIG_MENU_COUNT: usize = 40;

const APP_CONFIG_MODULE_LABELS: [&str; 6] = [
    "文档搜索 🔍",
    "release notes",
    "多语言导航",
    "metrics dashboard",
    "评论系统 💬",
    "syntax highlighting",
];

const APP_CONFIG_MODULE_SUMMARIES: [&str; 4] = [
    "Deterministic module summary shared by the benchmark corpus generator.",
    "该模块由基准语料生成器确定性生成，覆盖注释密集的配置树。",
    "Summary text mixes English and 中文 so Unicode handling stays exercised.",
    "決定論的に生成されたモジュール概要です。",
];

const APP_CONFIG_SERVICE_REGIONS: [&str; 5] =
    ["eu-west-1", "us-east-2", "ap-northeast-1", "sa-east-1", "af-south-1"];

const APP_CONFIG_LANGUAGES: [(&str, &str, &str, &str); 6] = [
    ("en", "English", "TomlSmith Benchmark", "Deterministic corpus site"),
    ("zh", "简体中文", "TomlSmith 基准语料", "确定性语料生成器"),
    ("ja", "日本語", "TomlSmith ベンチマーク", "決定論的コーパス"),
    ("ru", "Русский", "Бенчмарк TomlSmith", "Детерминированный корпус"),
    ("fr", "Français", "Banc d'essai TomlSmith", "Corpus déterministe"),
    ("de", "Deutsch", "TomlSmith Benchmark", "Deterministisches Korpus"),
];

#[allow(clippy::too_many_lines)]
fn app_config_fixture() -> String {
    use std::fmt::Write as _;

    let mut source = String::with_capacity(56 * 1024);
    let mut drift = FormatDrift::new();
    source.push_str(
        "# Deterministic application-config-shaped corpus generated by the TomlSmith benchmark\n\
         # generator. It mirrors a comment-dense static-site configuration — top-level scalars,\n\
         # a nested params tree, menus, languages and markup policies — without copying any\n\
         # external bytes.\n\n# Site identity.\n",
    );
    drift.assignment(&mut source, "baseURL", "\"https://benchmark.example.org/\"");
    drift.assignment(&mut source, "title", "\"TomlSmith 基准示例站点 🚀\"");
    drift.assignment(&mut source, "languageCode", "\"en-us\"");
    drift.assignment(&mut source, "defaultContentLanguage", "\"en\"");
    drift.assignment(&mut source, "theme", "\"engineering-benchmark\"");
    drift.assignment(&mut source, "copyright", "\"© 2026 TomlSmith Benchmark contributors\"");
    source.push_str("# Build metadata is fixed so the corpus stays deterministic.\n");
    drift.assignment(&mut source, "buildDate", "2026-08-29T12:00:00+08:00");
    drift.assignment(&mut source, "lastmod", "2026-08-29");
    drift.assignment(&mut source, "paginate", "24");
    drift.assignment(&mut source, "summaryLength", "70");
    drift.assignment(&mut source, "timeout", "45.5");
    drift.assignment(&mut source, "enableRobotsTXT", "true");
    drift.assignment(&mut source, "enableGitInfo", "false");

    source.push_str("\n# Root of the nested parameter tree.\n[params]\n");
    source.push_str(
        "description = \"\"\"\nTomlSmith Benchmark 的确定性示例配置。\nThe description spans multiple lines and mixes English, 中文 and emoji 😀\nso multi-line basic strings stay covered.\n\"\"\"\n",
    );
    drift.assignment(&mut source, "mainSections", "[\"posts\", \"docs\", \"releases\"]");
    source.push_str("# Dotted keys extend sibling tables without opening new sections.\n");
    drift.assignment(&mut source, "logo.light", "\"/images/logo-light.svg\"");
    drift.assignment(&mut source, "logo.dark", "\"/images/logo-dark.svg\"");
    drift.assignment(&mut source, "search.enabled", "true");
    drift.assignment(&mut source, "search.provider", "\"flexsearch\"");
    drift.assignment(&mut source, "search.maxResults", "25");

    source.push_str("\n[params.author]\n");
    drift.assignment(&mut source, "name", "\"TomlSmith Benchmark\"");
    drift.assignment(&mut source, "email", "\"corpus@example.org\"");
    drift.assignment(&mut source, "bio", "\"Deterministic fixtures only — 确定性生成\"");

    source.push_str("\n[params.templates]\n");
    source.push_str(
        "release_notes = '''\nLiteral template: {{ .Title }} — {{ .Date }}\nWindows path C:\\hugo\\templates stays literal here.\n'''\n",
    );

    for index in 0..APP_CONFIG_MODULE_COUNT {
        writeln!(
            source,
            "\n# Feature module {index:02} keeps the three-level params tree busy.\n[params.modules.module-{index:02}]"
        )
        .expect("writing to a String cannot fail");
        drift.assignment(
            &mut source,
            "enabled",
            if index.is_multiple_of(2) { "true" } else { "false" },
        );
        drift.assignment(&mut source, "weight", &format!("{}", index + 1));
        drift.assignment(&mut source, "ratio", &format!("{}.{}", index % 10, index % 100));
        drift.assignment(
            &mut source,
            "label",
            &format!("\"{}\"", APP_CONFIG_MODULE_LABELS[index % APP_CONFIG_MODULE_LABELS.len()]),
        );
        drift.assignment(
            &mut source,
            "summary",
            &format!(
                "\"{}\"",
                APP_CONFIG_MODULE_SUMMARIES[index % APP_CONFIG_MODULE_SUMMARIES.len()]
            ),
        );
        drift.assignment(&mut source, "docsURL", &format!("\"/docs/modules/module-{index:02}/\""));
        drift.assignment(
            &mut source,
            "tags",
            &format!("[\"module\", \"tier-{}\", \"slot-{index:02}\"]", index % 4),
        );
    }

    for tier in 0..APP_CONFIG_SERVICE_TIERS {
        writeln!(source, "\n# Service tier {tier} — four levels deep in the params tree.")
            .expect("writing to a String cannot fail");
        for slot in 0..APP_CONFIG_SERVICES_PER_TIER {
            writeln!(source, "\n[params.services.tier-{tier}.service-{slot:02}]")
                .expect("writing to a String cannot fail");
            drift.assignment(
                &mut source,
                "endpoint",
                &format!("\"https://svc-{tier}-{slot:02}.internal.example.org\""),
            );
            drift.assignment(
                &mut source,
                "timeout_ms",
                &format!("{}", 250 + tier * 250 + slot * 10),
            );
            drift.assignment(&mut source, "retries", &format!("{}", slot % 5));
            drift.assignment(
                &mut source,
                "enabled",
                if slot.is_multiple_of(3) { "true" } else { "false" },
            );
            drift.assignment(
                &mut source,
                "protocol",
                if slot.is_multiple_of(2) { "\"grpc\"" } else { "\"http2\"" },
            );
            drift.assignment(
                &mut source,
                "region",
                &format!(
                    "\"{}\"",
                    APP_CONFIG_SERVICE_REGIONS[(tier + slot) % APP_CONFIG_SERVICE_REGIONS.len()]
                ),
            );
            drift.assignment(
                &mut source,
                "owner.team",
                &format!("\"platform-squad-{:02}\"", slot % 6),
            );
            drift.assignment(&mut source, "owner.oncall", &format!("\"oncall-rotation-{tier}\""));
        }
    }

    source.push_str("\n# Primary navigation entries.\n");
    for index in 0..APP_CONFIG_MENU_COUNT {
        if index.is_multiple_of(5) {
            writeln!(source, "# menu block {}", index / 5)
                .expect("writing to a String cannot fail");
        }
        source.push_str("[[menu.main]]\n");
        drift.assignment(&mut source, "name", &format!("\"Docs {index:02}\""));
        drift.assignment(&mut source, "url", &format!("\"/docs/section-{index:02}/\""));
        drift.assignment(&mut source, "weight", &format!("{}", index + 1));
        drift.assignment(&mut source, "identifier", &format!("\"docs-{index:02}\""));
        if index % 4 == 3 {
            drift.assignment(&mut source, "parent", &format!("\"docs-{:02}\"", index - 3));
        }
    }
    source.push_str("\n# Footer navigation stays short.\n");
    for index in 0..8 {
        source.push_str("[[menu.footer]]\n");
        drift.assignment(&mut source, "name", &format!("\"Footer {index}\""));
        drift.assignment(&mut source, "url", &format!("\"/footer/link-{index}/\""));
        drift.assignment(&mut source, "weight", &format!("{}", index + 1));
    }

    source.push_str("\n# Localized sections with Unicode titles.\n");
    for (index, (code, language_name, title, subtitle)) in APP_CONFIG_LANGUAGES.iter().enumerate() {
        writeln!(source, "\n[languages.{code}]").expect("writing to a String cannot fail");
        drift.assignment(&mut source, "languageName", &format!("\"{language_name}\""));
        drift.assignment(&mut source, "weight", &format!("{}", index + 1));
        drift.assignment(&mut source, "title", &format!("\"{title}\""));
        writeln!(source, "\n[languages.{code}.params]").expect("writing to a String cannot fail");
        drift.assignment(&mut source, "subtitle", &format!("\"{subtitle}\""));
        drift.assignment(&mut source, "dateFormat", "\"2006-01-02\"");
    }

    source.push_str("\n# Markup pipeline configuration.\n[markup]\n");
    drift.assignment(&mut source, "defaultMarkdownHandler", "\"goldmark\"");
    source.push_str("\n[markup.goldmark.renderer]\n");
    drift.assignment(&mut source, "unsafe", "false");
    drift.assignment(&mut source, "hardWraps", "false");
    drift.assignment(&mut source, "xhtml", "false");
    source.push_str("\n[markup.goldmark.parser.attribute]\n");
    drift.assignment(&mut source, "block", "true");
    drift.assignment(&mut source, "title", "true");
    source.push_str("\n[markup.highlight]\n");
    drift.assignment(&mut source, "style", "\"monokai\"");
    drift.assignment(&mut source, "lineNos", "true");
    drift.assignment(&mut source, "lineNumbersInTable", "false");
    drift.assignment(&mut source, "tabWidth", "4");
    source.push_str("\n[markup.tableOfContents]\n");
    drift.assignment(&mut source, "startLevel", "2");
    drift.assignment(&mut source, "endLevel", "4");
    drift.assignment(&mut source, "ordered", "false");
    source
}

fn generated_invalid(
    id: &'static str,
    version: TomlVersion,
    workload_shape: WorkloadShape,
    tags: &'static [&'static str],
    relative_path: &'static str,
    source: &'static str,
) -> GeneratedFixture {
    GeneratedFixture {
        id,
        version,
        class: FixtureClass::Invalid,
        workload_shape,
        purpose: FixturePurpose::Correctness,
        syntax_profile: SyntaxProfile::Invalid,
        format_state: FormatState::Edited,
        source_kind: SourceKind::SpecificationGenerated,
        tags,
        relative_path,
        expected_valid: false,
        provenance: "deterministic syntax-boundary case generated from the published TOML specification; no external corpus bytes",
        source_url: None,
        source_revision: None,
        license: "MIT",
        source: source.to_owned(),
    }
}

fn generated_manifest(generated: &[GeneratedFixture]) -> FixtureManifest {
    let entries = generated
        .iter()
        .map(|fixture| FixtureManifestEntry {
            id: fixture.id.to_owned(),
            toml_version: fixture.version,
            class: fixture.class,
            workload_shape: fixture.workload_shape,
            purpose: fixture.purpose,
            syntax_profile: fixture.syntax_profile,
            format_state: fixture.format_state,
            source_kind: fixture.source_kind,
            tags: fixture.tags.iter().map(|tag| (*tag).to_owned()).collect(),
            path: fixture.relative_path.to_owned(),
            expected_valid: fixture.expected_valid,
            bytes: fixture.source.len(),
            lines: fixture.source.lines().count(),
            sha256: sha256(fixture.source.as_bytes()),
            provenance: fixture.provenance.to_owned(),
            source_url: fixture.source_url.map(str::to_owned),
            source_revision: fixture.source_revision.map(str::to_owned),
            license: fixture.license.to_owned(),
        })
        .collect();
    FixtureManifest { fixtures: entries }
}

fn scaled_specification_fixture(
    version: TomlVersion,
    target_bytes: usize,
    crlf: bool,
    format_state: FormatState,
) -> String {
    let mut source = specification_fixture(version);
    source.reserve(target_bytes.saturating_sub(source.len()) + 512);
    let structured_target =
        if target_bytes >= 10 * 1024 * 1024 { 1280 * 1024 } else { target_bytes };
    let mut index = 0;
    while source.len() < structured_target {
        append_structured_record(&mut source, index, format_state);
        index += 1;
    }
    if target_bytes >= 10 * 1024 * 1024 {
        append_stress_scalar(&mut source, target_bytes);
    }
    if crlf { source.replace('\n', "\r\n") } else { source }
}

fn append_stress_scalar(source: &mut String, target_bytes: usize) {
    const HEADER: &str = "\n[stress_payload]\ndata = '''\n";
    const FOOTER: &str = "\n'''\n";

    let payload_bytes = target_bytes.saturating_sub(source.len() + HEADER.len() + FOOTER.len());
    source.push_str(HEADER);
    source.extend(std::iter::repeat_n('x', payload_bytes));
    source.push_str(FOOTER);
}

fn append_structured_record(source: &mut String, index: usize, format_state: FormatState) {
    use std::fmt::Write as _;

    let enabled = index.is_multiple_of(2);
    let previous = index.saturating_sub(1);
    let result = match format_state {
        FormatState::Formatted => writeln!(
            source,
            "\n[[records]]\nid = {index}\nname = \"synthetic-record-{index:06}\"\nenabled = {enabled}\nratio = {}.{}\ncreated_at = 2026-08-29T12:34:56Z\nlabels = [\"parser\", \"formatter\", \"record-{index:06}\"]\nmetadata = {{ shard = {}, previous = {previous}, digest = \"{index:064x}\" }}",
            index % 100,
            index % 10,
            index % 32,
        ),
        FormatState::Edited => writeln!(
            source,
            "\n[[records]]\nid={index}\nname=\"synthetic-record-{index:06}\"\nenabled={enabled}\nratio={}.{}\ncreated_at=2026-08-29T12:34:56Z\nlabels=[\"parser\",\"formatter\",\"record-{index:06}\"]\nmetadata={{shard={},previous={previous},digest=\"{index:064x}\"}}",
            index % 100,
            index % 10,
            index % 32,
        ),
    };
    result.expect("writing to a String cannot fail");
}

// The decomposed `e` + combining acute accent is deliberate workload coverage.
#[allow(clippy::unicode_not_nfc)]
#[allow(clippy::too_many_lines)]
fn specification_fixture(version: TomlVersion) -> String {
    let mut source = String::from(
        r#"# Deterministic synthetic coverage of the published TOML specification.
# English, 中文, русский and emoji comments 😀 are valid UTF-8.
bare_key = "bare key"
"quoted.key" = "the dot is part of this key"
'literal.key' = 'literal quoted key'
"" = "empty quoted key"
1234 = "numeric bare key"
unicode_key = "café 東京 🚀" # trailing comment
combining_unicode = "é and é remain distinct scalar sequences"
dotted . key = "whitespace around dots"
quoted_dotted . "part.with.dot" = "quoted dotted key"

basic_string = "basic UTF-8 string"
escaped_string = "backspace=\b tab=\t newline=\n formfeed=\f carriage=\r quote=\" slash=\\"
unicode_escape = "delta=\u03B4 rocket=\U0001F680"
raw_tab = "before	after"
multiline_basic = """
The first newline is trimmed.
A continuation \
    removes surrounding whitespace.
Here are two quotation marks: "".
"""
literal_string = 'C:\Users\nodejs\templates'
multiline_literal = '''
The first newline is trimmed.
C:\Users\nodejs\templates
Here are two apostrophes: ''.
'''

decimal_integer = 42
positive_integer = +17
negative_integer = -17
zero_integer = 0
underscored_integer = 1_000_000
hex_integer = 0xDEAD_BEEF
octal_integer = 0o755
binary_integer = 0b1101_0010
fractional_float = 3.1415
exponent_float = 5e+22
signed_exponent_float = -2E-2
underscored_float = 224_617.445_991_228
positive_infinity = inf
negative_infinity = -inf
not_a_number = nan
negative_not_a_number = -nan
enabled = true
disabled = false

offset_datetime = 1979-05-27T07:32:00Z
offset_datetime_space = 1979-05-27 07:32:00-07:00
offset_datetime_fraction = 1979-05-27T00:32:00.999999-07:00
local_datetime = 1979-05-27T07:32:00
local_datetime_space = 1979-05-27 07:32:00.123456
local_date = 1979-05-27
local_time = 07:32:00.999999

empty_array = []
integer_array = [1, 2, 3]
mixed_array = [0.1, 2, "three", true, 1979-05-27T07:32:00Z]
nested_array = [[1, 2], ["a", "b"]]
multiline_array = [
  "alpha",
  # comments and trailing commas are valid in arrays
  "beta",
]
empty_inline_table = {}
inline_table = { first = "Tom", nested = { enabled = true }, dotted.key = "value" }
inline_table_array = [{ x = 1, y = 2 }, { x = 3, y = 4 }]
"#,
    );

    if version == TomlVersion::V1_1 {
        source.push_str(
            r#"
# TOML 1.1 additions.
escape_character = "\e"
hex_escape = "\x41"
short_offset_datetime = 1979-05-27T07:32Z
short_local_datetime = 1979-05-27T07:32
short_local_time = 07:32
multiline_inline_table = {
  alpha = 1, # comments and newlines are valid in TOML 1.1 inline tables
  nested = {
    enabled = true,
  },
  trailing = true,
}
"#,
        );
    }

    source.push_str(
        r#"
[standard_table]
bare = "value"
"quoted key" = "value"
'literal key' = "value"

[standard_table.child]
depth = 1

[dog."tater.man"]
type.name = "pug"

[implicit.parent.child]
created_before_parent = true

[implicit]
declared_after_child = true

[empty_table]

[[products]]
name = "Hammer"
sku = 738594937

[products.details]
weight = 1.25
dimensions = { width = 10, height = 4 }

[[products.variants]]
name = "steel"

[[products.variants]]
name = "titanium"

[[products]]
name = "Nail"
sku = 284758393

[[products.variants]]
name = "gray"
"#,
    );
    source
}

fn crlf_diagnostic_fixture(version: TomlVersion) -> String {
    let mut lines = vec![
        "# Deterministic CRLF diagnostic; 中文 comment",
        "title = \"CRLF line endings\"",
        "values = [1, 2, 3] # trailing comment",
        "",
        "[owner]",
        "name = \"Nirvana-Jie\"",
    ];
    if version == TomlVersion::V1_1 {
        lines.push("hex_escape = \"\\x43\"");
        lines.push("short_time = 08:15");
    }
    format!("{}\r\n", lines.join("\r\n"))
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|component| matches!(component, Component::Normal(_)))
}

fn read(path: &Path) -> Result<Vec<u8>, CorpusError> {
    fs::read(path).map_err(|source| CorpusError::Read { path: path.to_path_buf(), source })
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
