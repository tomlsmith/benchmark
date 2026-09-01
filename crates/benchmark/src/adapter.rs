use std::{fmt, hint::black_box};

use serde::Serialize;

use crate::{CanonicalValue, TomlVersion};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    DocumentPipeline,
    SemanticDecode,
    FormatPipeline,
}

impl Operation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DocumentPipeline => "document_pipeline",
            Self::SemanticDecode => "semantic_decode",
            Self::FormatPipeline => "format_pipeline",
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Availability {
    Available,
    Optional { enable_with: &'static str },
    Unavailable { reason: &'static str },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum VersionMode {
    StrictSelectable,
    CompatibleSubset { parser_specification: &'static str, participation: &'static str },
    Declared { version: TomlVersion },
    ExternalBinary { required_version: &'static str },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SeamDescriptor {
    pub operation: Operation,
    pub seam_id: &'static str,
    pub comparability_class: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct AdapterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub version: Option<&'static str>,
    pub upstream: &'static str,
    pub engine_family: &'static str,
    pub availability: Availability,
    pub seams: &'static [SeamDescriptor],
    pub toml_versions: &'static [TomlVersion],
    pub version_mode: VersionMode,
    pub included_work: &'static str,
    pub caveats: &'static [&'static str],
}

impl AdapterDescriptor {
    #[must_use]
    pub fn seam(&self, operation: Operation) -> Option<&SeamDescriptor> {
        self.seams.iter().find(|seam| seam.operation == operation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterOutput {
    pub diagnostics: usize,
    pub fingerprint: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("adapter {adapter} does not support {operation}")]
    Unsupported { adapter: &'static str, operation: Operation },
    #[error("adapter {adapter} does not support TOML {version}")]
    UnsupportedVersion { adapter: &'static str, version: TomlVersion },
    #[error("adapter {adapter} rejected the input: {message}")]
    Rejected { adapter: &'static str, message: String },
}

pub trait Adapter: fmt::Debug + Send + Sync {
    fn descriptor(&self) -> &'static AdapterDescriptor;

    /// Runs one supported normalized public operation.
    ///
    /// # Errors
    ///
    /// Returns an unsupported operation/version error or a rejection from a fail-fast parser.
    fn run(
        &self,
        operation: Operation,
        version: TomlVersion,
        source: &str,
    ) -> Result<AdapterOutput, AdapterError>;

    /// Produces the complete canonical semantic product for untimed correctness checks.
    ///
    /// Timed benchmarks must continue to use [`Self::run`] and its O(1) result consumption.
    ///
    /// # Errors
    ///
    /// Returns a parser rejection or version error when the source cannot be canonicalized.
    fn canonical_semantics(
        &self,
        _version: TomlVersion,
        _source: &str,
    ) -> Result<Option<CanonicalValue>, AdapterError> {
        Ok(None)
    }

    /// Returns formatted text for untimed correctness checks.
    ///
    /// # Errors
    ///
    /// Returns a formatter rejection or version error when formatting cannot be completed.
    fn format_for_verification(
        &self,
        _version: TomlVersion,
        _source: &str,
    ) -> Result<Option<String>, AdapterError> {
        Ok(None)
    }

    fn supports(&self, operation: Operation) -> bool {
        self.descriptor().seam(operation).is_some()
            && matches!(self.descriptor().availability, Availability::Available)
    }

    fn supports_version(&self, version: TomlVersion) -> bool {
        self.descriptor().toml_versions.contains(&version)
            && matches!(self.descriptor().availability, Availability::Available)
    }
}

#[derive(Clone, Copy, Debug)]
enum AdapterKind {
    TomlSmith,
    Toml,
    TomlEdit,
    Taplo,
}

#[derive(Debug)]
struct BuiltInAdapter {
    kind: AdapterKind,
    descriptor: &'static AdapterDescriptor,
}

impl Adapter for BuiltInAdapter {
    fn descriptor(&self) -> &'static AdapterDescriptor {
        self.descriptor
    }

    fn run(
        &self,
        operation: Operation,
        version: TomlVersion,
        source: &str,
    ) -> Result<AdapterOutput, AdapterError> {
        if !self.supports(operation) {
            return Err(AdapterError::Unsupported { adapter: self.descriptor.id, operation });
        }
        if !self.supports_version(version) {
            return Err(AdapterError::UnsupportedVersion { adapter: self.descriptor.id, version });
        }

        match (self.kind, operation) {
            (AdapterKind::TomlSmith, Operation::DocumentPipeline) => {
                Ok(run_tomlsmith(source, version, false))
            }
            (AdapterKind::TomlSmith, Operation::SemanticDecode) => {
                Ok(run_tomlsmith(source, version, true))
            }
            (AdapterKind::Toml, Operation::SemanticDecode) => run_toml(source),
            (AdapterKind::TomlEdit, Operation::DocumentPipeline) => run_toml_edit(source),
            (AdapterKind::Taplo, Operation::DocumentPipeline) => Ok(run_taplo_document(source)),
            (AdapterKind::Taplo, Operation::SemanticDecode) => Ok(run_taplo_semantic(source)),
            (AdapterKind::TomlSmith, Operation::FormatPipeline) => {
                Ok(run_tomlsmith_format(source, version))
            }
            (AdapterKind::Taplo, Operation::FormatPipeline) => Ok(run_taplo_format(source)),
            (_, operation) => {
                Err(AdapterError::Unsupported { adapter: self.descriptor.id, operation })
            }
        }
    }

    fn canonical_semantics(
        &self,
        version: TomlVersion,
        source: &str,
    ) -> Result<Option<CanonicalValue>, AdapterError> {
        if !self.supports_version(version) {
            return Err(AdapterError::UnsupportedVersion { adapter: self.descriptor.id, version });
        }
        let canonical = match self.kind {
            AdapterKind::TomlSmith => crate::correctness::canonical_tomlsmith(source, version)?,
            AdapterKind::Toml => crate::correctness::canonical_toml(source)?,
            AdapterKind::Taplo => crate::correctness::canonical_taplo(source)?,
            AdapterKind::TomlEdit => return Ok(None),
        };
        Ok(Some(canonical))
    }

    fn format_for_verification(
        &self,
        version: TomlVersion,
        source: &str,
    ) -> Result<Option<String>, AdapterError> {
        if !self.supports(Operation::FormatPipeline) {
            return Ok(None);
        }
        if !self.supports_version(version) {
            return Err(AdapterError::UnsupportedVersion { adapter: self.descriptor.id, version });
        }
        match self.kind {
            AdapterKind::TomlSmith => {
                crate::correctness::format_tomlsmith(source, version).map(Some)
            }
            AdapterKind::Taplo => {
                Ok(Some(taplo::formatter::format(source, taplo::formatter::Options::default())))
            }
            AdapterKind::Toml | AdapterKind::TomlEdit => Ok(None),
        }
    }
}

fn run_tomlsmith_format(source: &str, version: TomlVersion) -> AdapterOutput {
    let document = tomlsmith::Document::parse_as(source, tomlsmith_version(version));
    black_box(&document);
    let outcome = document.format();
    black_box(&outcome);
    match outcome {
        tomlsmith::FormatOutcome::Unchanged => AdapterOutput {
            diagnostics: document.diagnostics().len(),
            fingerprint: source.len() as u64,
        },
        tomlsmith::FormatOutcome::Changed { text, edits } => AdapterOutput {
            diagnostics: document.diagnostics().len(),
            fingerprint: text.len() as u64 + edits.len() as u64,
        },
        tomlsmith::FormatOutcome::Refused { diagnostics } => {
            AdapterOutput { diagnostics: diagnostics.len(), fingerprint: 0 }
        }
    }
}

fn run_tomlsmith(source: &str, version: TomlVersion, consume_semantics: bool) -> AdapterOutput {
    let document = tomlsmith::Document::parse_as(source, tomlsmith_version(version));
    black_box(&document);
    let diagnostics = document.diagnostics().len();
    let fingerprint = if consume_semantics {
        document.semantics().root().entries().len() as u64
    } else {
        u64::from(document.root().range().end())
    };
    AdapterOutput { diagnostics, fingerprint }
}

const fn tomlsmith_version(version: TomlVersion) -> tomlsmith::TomlVersion {
    match version {
        TomlVersion::V1_0 => tomlsmith::TomlVersion::V1_0,
        TomlVersion::V1_1 => tomlsmith::TomlVersion::V1_1,
    }
}

fn run_toml(source: &str) -> Result<AdapterOutput, AdapterError> {
    let value = toml::from_str::<toml::Value>(source)
        .map_err(|error| AdapterError::Rejected { adapter: TOML.id, message: error.to_string() })?;
    black_box(&value);
    let fingerprint = match &value {
        toml::Value::Table(table) => table.len() as u64,
        toml::Value::Array(array) => array.len() as u64,
        _ => 1,
    };
    Ok(AdapterOutput { diagnostics: 0, fingerprint })
}

fn run_toml_edit(source: &str) -> Result<AdapterOutput, AdapterError> {
    let document = source.parse::<toml_edit::DocumentMut>().map_err(|error| {
        AdapterError::Rejected { adapter: TOML_EDIT.id, message: error.to_string() }
    })?;
    black_box(&document);
    Ok(AdapterOutput { diagnostics: 0, fingerprint: document.as_table().len() as u64 })
}

fn run_taplo_document(source: &str) -> AdapterOutput {
    let parsed = taplo::parser::parse(source);
    black_box(&parsed);
    let diagnostics = parsed.errors.len();
    let fingerprint = u64::from(u32::from(parsed.green_node.text_len()));
    AdapterOutput { diagnostics, fingerprint }
}

fn run_taplo_semantic(source: &str) -> AdapterOutput {
    let parsed = taplo::parser::parse(source);
    black_box(&parsed);
    let syntax_diagnostics = parsed.errors.len();
    let dom = parsed.into_dom();
    black_box(&dom);
    let semantic_diagnostics = dom.validate().err().map_or(0, Iterator::count);
    let fingerprint = match &dom {
        taplo::dom::Node::Table(table) => table.entries().read().len() as u64,
        taplo::dom::Node::Array(array) => array.items().read().len() as u64,
        _ => 1,
    };
    AdapterOutput { diagnostics: syntax_diagnostics + semantic_diagnostics, fingerprint }
}

fn run_taplo_format(source: &str) -> AdapterOutput {
    let formatted = taplo::formatter::format(source, taplo::formatter::Options::default());
    black_box(&formatted);
    AdapterOutput { diagnostics: 0, fingerprint: formatted.len() as u64 }
}

const TOMLSMITH_SEAMS: &[SeamDescriptor] = &[
    SeamDescriptor {
        operation: Operation::DocumentPipeline,
        seam_id: "tomlsmith.document.full_document",
        comparability_class: "tomlsmith_lossless_document",
    },
    SeamDescriptor {
        operation: Operation::SemanticDecode,
        seam_id: "tomlsmith.semantic.document_root",
        comparability_class: "tomlsmith_semantic_document",
    },
    SeamDescriptor {
        operation: Operation::FormatPipeline,
        seam_id: "tomlsmith.format.document",
        comparability_class: "source_to_formatted_text",
    },
];
const TOML_SEAMS: &[SeamDescriptor] = &[SeamDescriptor {
    operation: Operation::SemanticDecode,
    seam_id: "toml.semantic.value",
    comparability_class: "toml_semantic_value",
}];
const TOML_EDIT_SEAMS: &[SeamDescriptor] = &[SeamDescriptor {
    operation: Operation::DocumentPipeline,
    seam_id: "toml_edit.document.document_mut",
    comparability_class: "toml_edit_document_mut",
}];
const TAPLO_SEAMS: &[SeamDescriptor] = &[
    SeamDescriptor {
        operation: Operation::DocumentPipeline,
        seam_id: "taplo.document.syntax_tree",
        comparability_class: "taplo_syntax_tree",
    },
    SeamDescriptor {
        operation: Operation::SemanticDecode,
        seam_id: "taplo.semantic.dom_validate",
        comparability_class: "taplo_dom_with_validation",
    },
    SeamDescriptor {
        operation: Operation::FormatPipeline,
        seam_id: "taplo.format.formatter",
        comparability_class: "source_to_formatted_text",
    },
];
const TOML_1_0_AND_1_1: &[TomlVersion] = &[TomlVersion::V1_0, TomlVersion::V1_1];
const TOML_1_0_ONLY: &[TomlVersion] = &[TomlVersion::V1_0];

static TOMLSMITH: AdapterDescriptor = AdapterDescriptor {
    id: "tomlsmith",
    display_name: "TomlSmith",
    version: Some("0.3.1 (crates.io)"),
    upstream: "https://github.com/tomlsmith/tomlsmith",
    engine_family: "tomlsmith",
    availability: Availability::Available,
    seams: TOMLSMITH_SEAMS,
    toml_versions: TOML_1_0_AND_1_1,
    version_mode: VersionMode::StrictSelectable,
    included_work: "document_pipeline runs version-selected Document::parse_as, including lossless CST, validation, semantic root, diagnostics, and highlights; semantic_decode additionally consumes the root; format_pipeline additionally runs Document::format",
    caveats: &[
        "TomlSmith has no public syntax-only entry point; document_pipeline includes more work than the other source-backed parsers.",
    ],
};

static TOML: AdapterDescriptor = AdapterDescriptor {
    id: "toml",
    display_name: "toml",
    version: Some("1.1.4+spec-1.1.0"),
    upstream: "https://github.com/toml-rs/toml",
    engine_family: "toml-rs",
    availability: Availability::Available,
    seams: TOML_SEAMS,
    toml_versions: TOML_1_0_AND_1_1,
    version_mode: VersionMode::CompatibleSubset {
        parser_specification: "1.1.0",
        participation: "TOML 1.0 fixtures are a compatible subset; no strict 1.0 selector",
    },
    included_work: "toml::from_str::<toml::Value>: parse and materialize a semantic value tree",
    caveats: &[
        "toml and toml_edit share the toml-rs parser family and are not independent parser engines.",
        "The public parser targets TOML 1.1 and accepts the TOML 1.0 corpus as a compatible subset; it does not expose strict version selection.",
    ],
};

static TOML_EDIT: AdapterDescriptor = AdapterDescriptor {
    id: "toml_edit",
    display_name: "toml_edit",
    version: Some("0.25.13+spec-1.1.0"),
    upstream: "https://github.com/toml-rs/toml",
    engine_family: "toml-rs",
    availability: Availability::Available,
    seams: TOML_EDIT_SEAMS,
    toml_versions: TOML_1_0_AND_1_1,
    version_mode: VersionMode::CompatibleSubset {
        parser_specification: "1.1.0",
        participation: "TOML 1.0 fixtures are a compatible subset; no strict 1.0 selector",
    },
    included_work: "str::parse::<toml_edit::DocumentMut>: parse and build a format-preserving editable document",
    caveats: &[
        "toml and toml_edit share the toml-rs parser family and are not independent parser engines.",
        "The public parser targets TOML 1.1 and accepts the TOML 1.0 corpus as a compatible subset; it does not expose strict version selection.",
    ],
};

static TAPLO: AdapterDescriptor = AdapterDescriptor {
    id: "taplo",
    display_name: "Taplo",
    version: Some("0.14.0"),
    upstream: "https://github.com/tamasfe/taplo",
    engine_family: "taplo",
    availability: Availability::Available,
    seams: TAPLO_SEAMS,
    toml_versions: TOML_1_0_ONLY,
    version_mode: VersionMode::Declared { version: TomlVersion::V1_0 },
    included_work: "document_pipeline builds Taplo's Rowan syntax tree; semantic_decode parses, builds its DOM, and validates it; format_pipeline calls formatter::format, which parses and formats",
    caveats: &["Taplo 0.14.0 targets TOML 1.0 and is excluded from every TOML 1.1 case."],
};

#[must_use]
pub fn built_in_catalog() -> Vec<Box<dyn Adapter>> {
    [
        (AdapterKind::TomlSmith, &TOMLSMITH),
        (AdapterKind::Toml, &TOML),
        (AdapterKind::TomlEdit, &TOML_EDIT),
        (AdapterKind::Taplo, &TAPLO),
    ]
    .into_iter()
    .map(|(kind, descriptor)| Box::new(BuiltInAdapter { kind, descriptor }) as Box<dyn Adapter>)
    .collect()
}
