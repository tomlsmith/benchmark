#![forbid(unsafe_code)]

mod adapter;
mod corpus;
mod correctness;
mod environment;
mod external_process;
mod verify;
mod version;

pub use adapter::{
    Adapter, AdapterDescriptor, AdapterError, AdapterOutput, Availability, Operation,
    SeamDescriptor, VersionMode, built_in_catalog,
};
pub use corpus::{
    CorpusError, Fixture, FixtureClass, FixtureCorpus, FixtureManifest, FixtureManifestEntry,
    FixturePurpose, FormatState, SourceKind, SyntaxProfile, WorkloadShape, check_generated_corpus,
    generate_corpus,
};
pub use correctness::{CanonicalFloat, CanonicalValue, semantic_digest};
pub use environment::{
    BenchmarkSettings, BuildMetadata, CargoProfileMetadata, EnvironmentError, EnvironmentReport,
    GitMetadata, PowerMetadata, RuntimeMetadata, ToolMetadata,
};
pub use external_process::{
    DPRINT_TOML_PLUGIN_URL, GO_BINARY_ENV, OptionalToolAvailability, PRETTIER_PLUGIN_ENV,
    PeakMemoryProcessOutput, ProductDescriptor, ProductId, ProductOperation, ProductProcessError,
    ProductProcessOutput, ProductRunner, ProductStatus, ProductVersionSupport, TIME_BINARY_ENV,
    TOMBI_BINARY_ENV, TOMBI_REQUIRED_VERSION, product_catalog, product_status, product_statuses,
};
pub use verify::{
    FormatterInvariantCase, ProductVerificationCase, ProductVerificationReport,
    SemanticDigestMember, SemanticEquivalenceCase, VerificationCase, VerificationReport,
    verify_corpus, verify_corpus_with_product_filter, verify_products, verify_products_matching,
};
pub use version::TomlVersion;
