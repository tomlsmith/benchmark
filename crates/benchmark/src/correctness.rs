use std::{collections::BTreeMap, fmt::Write as _};

use sha2::{Digest, Sha256};

use crate::{AdapterError, TomlVersion};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalFloat {
    Finite(u64),
    PositiveInfinity,
    NegativeInfinity,
    NotANumber,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalValue {
    String(String),
    Integer(i64),
    Float(CanonicalFloat),
    Boolean(bool),
    DateTime(String),
    Array(Vec<Self>),
    Table(BTreeMap<String, Self>),
}

#[must_use]
pub fn semantic_digest(value: &CanonicalValue) -> String {
    let mut hasher = Sha256::new();
    hash_value(&mut hasher, value);
    format!("{:x}", hasher.finalize())
}

fn hash_value(hasher: &mut Sha256, value: &CanonicalValue) {
    match value {
        CanonicalValue::String(value) => {
            hasher.update([b's']);
            hash_bytes(hasher, value.as_bytes());
        }
        CanonicalValue::Integer(value) => {
            hasher.update([b'i']);
            hasher.update(value.to_be_bytes());
        }
        CanonicalValue::Float(value) => {
            hasher.update([b'f']);
            match value {
                CanonicalFloat::Finite(bits) => {
                    hasher.update([0]);
                    hasher.update(bits.to_be_bytes());
                }
                CanonicalFloat::PositiveInfinity => hasher.update([1]),
                CanonicalFloat::NegativeInfinity => hasher.update([2]),
                CanonicalFloat::NotANumber => hasher.update([3]),
            }
        }
        CanonicalValue::Boolean(value) => {
            hasher.update([b'b', u8::from(*value)]);
        }
        CanonicalValue::DateTime(value) => {
            hasher.update([b'd']);
            hash_bytes(hasher, value.as_bytes());
        }
        CanonicalValue::Array(values) => {
            hasher.update([b'a']);
            hash_len(hasher, values.len());
            for value in values {
                hash_value(hasher, value);
            }
        }
        CanonicalValue::Table(entries) => {
            hasher.update([b't']);
            hash_len(hasher, entries.len());
            for (key, value) in entries {
                hash_bytes(hasher, key.as_bytes());
                hash_value(hasher, value);
            }
        }
    }
}

fn hash_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hash_len(hasher, bytes.len());
    hasher.update(bytes);
}

fn hash_len(hasher: &mut Sha256, len: usize) {
    hasher.update(u64::try_from(len).unwrap_or(u64::MAX).to_be_bytes());
}

const fn canonical_float(value: f64) -> CanonicalFloat {
    if value.is_nan() {
        CanonicalFloat::NotANumber
    } else if value == f64::INFINITY {
        CanonicalFloat::PositiveInfinity
    } else if value == f64::NEG_INFINITY {
        CanonicalFloat::NegativeInfinity
    } else {
        CanonicalFloat::Finite(value.to_bits())
    }
}

pub(crate) fn canonical_tomlsmith(
    source: &str,
    version: TomlVersion,
) -> Result<CanonicalValue, AdapterError> {
    let document = tomlsmith::Document::parse_as(source, tomlsmith_version(version));
    if !document.diagnostics().is_empty() {
        return Err(AdapterError::Rejected {
            adapter: "tomlsmith",
            message: format!("{} diagnostic(s)", document.diagnostics().len()),
        });
    }
    canonical_tomlsmith_table(document.semantics().root())
}

fn canonical_tomlsmith_table(
    table: &tomlsmith::SemanticTable,
) -> Result<CanonicalValue, AdapterError> {
    let mut canonical = BTreeMap::new();
    for (key, value) in table.entries() {
        let value = canonical_tomlsmith_value(value)?;
        if canonical.insert(key.to_string(), value).is_some() {
            return Err(canonical_rejection("tomlsmith", "duplicate semantic table key"));
        }
    }
    Ok(CanonicalValue::Table(canonical))
}

fn canonical_tomlsmith_value(
    value: &tomlsmith::SemanticValue,
) -> Result<CanonicalValue, AdapterError> {
    match value {
        tomlsmith::SemanticValue::String(value) => Ok(CanonicalValue::String(value.to_string())),
        tomlsmith::SemanticValue::Integer(value) => Ok(CanonicalValue::Integer(*value)),
        tomlsmith::SemanticValue::Float(value) => {
            Ok(CanonicalValue::Float(canonical_float(*value)))
        }
        tomlsmith::SemanticValue::Boolean(value) => Ok(CanonicalValue::Boolean(*value)),
        tomlsmith::SemanticValue::DateTime(value) => {
            Ok(CanonicalValue::DateTime(canonical_datetime(value.canonical(), "tomlsmith")?))
        }
        tomlsmith::SemanticValue::Array(values) => values
            .iter()
            .map(canonical_tomlsmith_value)
            .collect::<Result<Vec<_>, _>>()
            .map(CanonicalValue::Array),
        tomlsmith::SemanticValue::InlineTable(entries) => {
            let mut table = BTreeMap::new();
            for (key, value) in entries.iter() {
                let segments = key.segments().map(str::to_owned).collect::<Vec<_>>();
                insert_path(&mut table, &segments, canonical_tomlsmith_value(value)?, "tomlsmith")?;
            }
            Ok(CanonicalValue::Table(table))
        }
        tomlsmith::SemanticValue::Table(table) => canonical_tomlsmith_table(table),
        tomlsmith::SemanticValue::Invalid(_) => {
            Err(canonical_rejection("tomlsmith", "invalid semantic value"))
        }
    }
}

fn insert_path(
    table: &mut BTreeMap<String, CanonicalValue>,
    path: &[String],
    value: CanonicalValue,
    adapter: &'static str,
) -> Result<(), AdapterError> {
    let Some((head, tail)) = path.split_first() else {
        return Err(canonical_rejection(adapter, "empty semantic key path"));
    };
    if tail.is_empty() {
        if table.insert(head.clone(), value).is_some() {
            return Err(canonical_rejection(adapter, "duplicate semantic key path"));
        }
        return Ok(());
    }
    let entry = table.entry(head.clone()).or_insert_with(|| CanonicalValue::Table(BTreeMap::new()));
    let CanonicalValue::Table(child) = entry else {
        return Err(canonical_rejection(adapter, "conflicting semantic key path"));
    };
    insert_path(child, tail, value, adapter)
}

pub(crate) fn canonical_toml(source: &str) -> Result<CanonicalValue, AdapterError> {
    let value = toml::from_str::<toml::Value>(source)
        .map_err(|error| AdapterError::Rejected { adapter: "toml", message: error.to_string() })?;
    canonical_toml_value(&value)
}

fn canonical_toml_value(value: &toml::Value) -> Result<CanonicalValue, AdapterError> {
    match value {
        toml::Value::String(value) => Ok(CanonicalValue::String(value.clone())),
        toml::Value::Integer(value) => Ok(CanonicalValue::Integer(*value)),
        toml::Value::Float(value) => Ok(CanonicalValue::Float(canonical_float(*value))),
        toml::Value::Boolean(value) => Ok(CanonicalValue::Boolean(*value)),
        toml::Value::Datetime(value) => {
            Ok(CanonicalValue::DateTime(canonical_datetime(&value.to_string(), "toml")?))
        }
        toml::Value::Array(values) => values
            .iter()
            .map(canonical_toml_value)
            .collect::<Result<Vec<_>, _>>()
            .map(CanonicalValue::Array),
        toml::Value::Table(entries) => entries
            .iter()
            .map(|(key, value)| Ok((key.clone(), canonical_toml_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, AdapterError>>()
            .map(CanonicalValue::Table),
    }
}

pub(crate) fn canonical_taplo(source: &str) -> Result<CanonicalValue, AdapterError> {
    let parsed = taplo::parser::parse(source);
    if !parsed.errors.is_empty() {
        return Err(AdapterError::Rejected {
            adapter: "taplo",
            message: format!("{} syntax diagnostic(s)", parsed.errors.len()),
        });
    }
    let dom = parsed.into_dom();
    if let Err(errors) = dom.validate() {
        return Err(AdapterError::Rejected {
            adapter: "taplo",
            message: format!("{} DOM validation error(s)", errors.count()),
        });
    }
    canonical_taplo_node(&dom)
}

fn canonical_taplo_node(node: &taplo::dom::Node) -> Result<CanonicalValue, AdapterError> {
    match node {
        taplo::dom::Node::Table(table) => {
            let entries = table.entries().read();
            entries
                .iter()
                .map(|(key, value)| Ok((key.value().to_owned(), canonical_taplo_node(value)?)))
                .collect::<Result<BTreeMap<_, _>, AdapterError>>()
                .map(CanonicalValue::Table)
        }
        taplo::dom::Node::Array(array) => {
            let items = array.items().read();
            items
                .iter()
                .map(canonical_taplo_node)
                .collect::<Result<Vec<_>, _>>()
                .map(CanonicalValue::Array)
        }
        taplo::dom::Node::Bool(value) => Ok(CanonicalValue::Boolean(value.value())),
        taplo::dom::Node::Str(value) => Ok(CanonicalValue::String(value.value().to_owned())),
        taplo::dom::Node::Integer(value) => {
            let value = value.value();
            let integer = match value {
                taplo::dom::node::IntegerValue::Negative(value) => value,
                taplo::dom::node::IntegerValue::Positive(value) => {
                    i64::try_from(value).map_err(|_| {
                        canonical_rejection("taplo", "positive integer exceeds signed TOML range")
                    })?
                }
            };
            Ok(CanonicalValue::Integer(integer))
        }
        taplo::dom::Node::Float(value) => Ok(CanonicalValue::Float(canonical_float(value.value()))),
        taplo::dom::Node::Date(value) => {
            Ok(CanonicalValue::DateTime(canonical_datetime(&value.value().to_string(), "taplo")?))
        }
        taplo::dom::Node::Invalid(_) => Err(canonical_rejection("taplo", "invalid DOM value")),
    }
}

fn canonical_datetime(text: &str, adapter: &'static str) -> Result<String, AdapterError> {
    let value = text.parse::<toml::value::Datetime>().map_err(|error| AdapterError::Rejected {
        adapter,
        message: format!("failed to canonicalize datetime {text:?}: {error}"),
    })?;
    let mut canonical = String::new();
    if let Some(date) = value.date {
        write!(canonical, "{:04}-{:02}-{:02}", date.year, date.month, date.day)
            .expect("writing to a String cannot fail");
    }
    if let Some(time) = value.time {
        if value.date.is_some() {
            canonical.push('T');
        }
        write!(canonical, "{:02}:{:02}:{:02}", time.hour, time.minute, time.second.unwrap_or(0))
            .expect("writing to a String cannot fail");
        if let Some(nanosecond) = time.nanosecond {
            let fraction = format!("{nanosecond:09}");
            let fraction = fraction.trim_end_matches('0');
            if !fraction.is_empty() {
                canonical.push('.');
                canonical.push_str(fraction);
            }
        }
    }
    if let Some(offset) = value.offset {
        match offset {
            toml::value::Offset::Z | toml::value::Offset::Custom { minutes: 0 } => {
                canonical.push('Z');
            }
            toml::value::Offset::Custom { minutes } => {
                let sign = if minutes < 0 { '-' } else { '+' };
                let absolute = minutes.unsigned_abs();
                canonical.push(sign);
                write!(canonical, "{:02}:{:02}", absolute / 60, absolute % 60)
                    .expect("writing to a String cannot fail");
            }
        }
    }
    Ok(canonical)
}

pub(crate) fn format_tomlsmith(source: &str, version: TomlVersion) -> Result<String, AdapterError> {
    let document = tomlsmith::Document::parse_as(source, tomlsmith_version(version));
    if !document.diagnostics().is_empty() {
        return Err(AdapterError::Rejected {
            adapter: "tomlsmith",
            message: format!("{} diagnostic(s)", document.diagnostics().len()),
        });
    }
    match document.format() {
        tomlsmith::FormatOutcome::Unchanged => Ok(source.to_owned()),
        tomlsmith::FormatOutcome::Changed { text, .. } => Ok(text.to_string()),
        tomlsmith::FormatOutcome::Refused { diagnostics } => Err(AdapterError::Rejected {
            adapter: "tomlsmith",
            message: format!("formatter refused with {} diagnostic(s)", diagnostics.len()),
        }),
    }
}

const fn tomlsmith_version(version: TomlVersion) -> tomlsmith::TomlVersion {
    match version {
        TomlVersion::V1_0 => tomlsmith::TomlVersion::V1_0,
        TomlVersion::V1_1 => tomlsmith::TomlVersion::V1_1,
    }
}

fn canonical_rejection(adapter: &'static str, message: &str) -> AdapterError {
    AdapterError::Rejected { adapter, message: message.to_owned() }
}
