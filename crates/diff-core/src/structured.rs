//! Typed JSON, YAML and XML comparison.
//!
//! Canonical output is always JSON: YAML maps/sequences become their JSON data
//! equivalents; XML becomes an ordered semantic node tree. This representation
//! is for inspection, never an implicit replacement for a source document.

use num_bigint::BigInt;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StructuredFormat {
    #[default]
    Json,
    Yaml,
    Xml,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct StructuredOptions {
    pub format: StructuredFormat,
    pub ignore_array_order: bool,
    pub ignore_case: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StructureChangeKind {
    Added,
    Removed,
    Changed,
    TypeChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureChange {
    /// RFC 6901 JSON Pointer; the empty string addresses the document root.
    pub path: String,
    pub kind: StructureChangeKind,
    pub left: Option<String>,
    pub right: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredResult {
    pub equal: bool,
    pub changes: Vec<StructureChange>,
    pub left_canonical: String,
    pub right_canonical: String,
}

// serde_json::Value normally retains only the last value of a repeated key.
// Validate uniqueness in a separate visitor before constructing Value. This
// also preserves arbitrary-precision numbers through serde_json's own Number
// deserializer rather than converting them to f64 in a custom value visitor.
struct UniqueJson;

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("JSON with unique object keys")
            }
            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                while seq.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut keys = HashSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate object key {key:?}"
                        )));
                    }
                    map.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct RawObject<'a>(Vec<(String, &'a serde_json::value::RawValue)>);

impl<'de> Deserialize<'de> for RawObject<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RawObjectVisitor;
        impl<'de> Visitor<'de> for RawObjectVisitor {
            type Value = RawObject<'de>;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some(entry) =
                    map.next_entry::<String, &'de serde_json::value::RawValue>()?
                {
                    entries.push(entry);
                }
                Ok(RawObject(entries))
            }
        }
        deserializer.deserialize_map(RawObjectVisitor)
    }
}

fn exact_json(raw: &serde_json::value::RawValue) -> Result<Value, String> {
    let source = raw.get();
    // Construct objects directly: serde_json's arbitrary-precision/raw-value
    // Value visitor treats its private sentinel keys as tagged scalar values.
    // A user's perfectly valid key must never disappear because it has that
    // spelling. The typed object visitor and raw numeric tokens avoid this.
    match source.as_bytes().first() {
        Some(b'{') => {
            let entries: RawObject<'_> = serde_json::from_str(source).map_err(|e| e.to_string())?;
            entries
                .0
                .into_iter()
                .map(|(key, raw)| exact_json(raw).map(|value| (key, value)))
                .collect::<Result<Map<_, _>, _>>()
                .map(Value::Object)
        }
        Some(b'[') => {
            let entries: Vec<&serde_json::value::RawValue> =
                serde_json::from_str(source).map_err(|e| e.to_string())?;
            entries
                .into_iter()
                .map(exact_json)
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        Some(b'"') => serde_json::from_str::<String>(source)
            .map(Value::String)
            .map_err(|e| e.to_string()),
        Some(b'n') => Ok(Value::Null),
        Some(b't') => Ok(Value::Bool(true)),
        Some(b'f') => Ok(Value::Bool(false)),
        _ => source
            .parse::<serde_json::Number>()
            .map(Value::Number)
            .map_err(|e| e.to_string()),
    }
}

fn parse_json(input: &str) -> Result<Value, String> {
    // The first pass retains whole-document line/column errors, rejects
    // duplicates and applies the parser's recursion protections.
    serde_json::from_str::<UniqueJson>(input).map_err(|e| e.to_string())?;
    let raw =
        serde_json::from_str::<&serde_json::value::RawValue>(input).map_err(|e| e.to_string())?;
    exact_json(raw)
}

fn pointer_child(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn convert_yaml(value: serde_yaml_ng::Value, path: &str) -> Result<Value, String> {
    use serde_yaml_ng::Value as Yaml;
    match value {
        Yaml::Null => Ok(Value::Null),
        Yaml::Bool(value) => Ok(Value::Bool(value)),
        Yaml::Number(value) => {
            let number = value
                .to_string()
                .parse::<serde_json::Number>()
                .map_err(|_| {
                    format!(
                        "non-finite YAML number at {} is not supported",
                        if path.is_empty() { "/" } else { path }
                    )
                })?;
            Ok(Value::Number(number))
        }
        Yaml::String(value) => Ok(Value::String(value)),
        Yaml::Sequence(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| convert_yaml(value, &pointer_child(path, &index.to_string())))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Yaml::Mapping(values) => {
            let mut result = Map::new();
            for (key, value) in values {
                let Yaml::String(key) = key else {
                    return Err(format!("YAML mapping at {} has a non-string key; quote scalar keys or use text comparison", if path.is_empty() { "/" } else { path }));
                };
                let child = pointer_child(path, &key);
                result.insert(key, convert_yaml(value, &child)?);
            }
            Ok(Value::Object(result))
        }
        Yaml::Tagged(value) => Err(format!(
            "custom YAML tag {} at {} is not supported; use text comparison",
            value.tag,
            if path.is_empty() { "/" } else { path }
        )),
    }
}

/// Recognize the finite numeric scalar forms supported by our YAML loader,
/// retaining their exact decimal value instead of resolving through f64.
fn exact_yaml_numeric_token(
    raw: &str,
    explicitly_numeric: bool,
) -> Option<Result<serde_json::Number, String>> {
    let (negative, unsigned) = raw.strip_prefix('-').map_or_else(
        || (false, raw.strip_prefix('+').unwrap_or(raw)),
        |value| (true, value),
    );
    if matches!(unsigned.to_ascii_lowercase().as_str(), ".inf" | ".nan") {
        return Some(Err("non-finite YAML numbers are not supported".into()));
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = unsigned.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|value| value.is_digit(radix)) {
                return None;
            }
            let number = BigInt::parse_bytes(digits.as_bytes(), radix)?;
            let token = if negative {
                format!("-{number}")
            } else {
                number.to_string()
            };
            return Some(
                token
                    .parse::<serde_json::Number>()
                    .map_err(|e| e.to_string()),
            );
        }
    }
    let (mantissa, exponent) = unsigned.find(['e', 'E']).map_or((unsigned, None), |index| {
        (&unsigned[..index], Some(&unsigned[index + 1..]))
    });
    if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if digits.is_empty() || !digits.bytes().all(|value| value.is_ascii_digit()) {
            return None;
        }
    }
    let (whole, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, None), |(a, b)| (a, Some(b)));
    if !whole.bytes().all(|value| value.is_ascii_digit())
        || fraction.is_some_and(|value| !value.bytes().all(|byte| byte.is_ascii_digit()))
        || whole.is_empty() && fraction.unwrap_or_default().is_empty()
    {
        return None;
    }
    // The existing YAML 1.2 loader resolves plain leading-zero integer-looking
    // scalars as strings, so these must not be reclassified as numbers here.
    if !explicitly_numeric
        && fraction.is_none()
        && exponent.is_none()
        && whole.len() > 1
        && whole.starts_with('0')
    {
        return None;
    }
    let integer = whole.trim_start_matches('0');
    let mut token = if negative {
        "-".to_owned()
    } else {
        String::new()
    };
    token.push_str(if integer.is_empty() { "0" } else { integer });
    if let Some(fraction) = fraction {
        token.push('.');
        token.push_str(if fraction.is_empty() { "0" } else { fraction });
    }
    if let Some(exponent) = exponent {
        token.push('e');
        token.push_str(exponent);
    }
    Some(
        token
            .parse::<serde_json::Number>()
            .map_err(|e| e.to_string()),
    )
}

/// serde_yaml_ng stores decimal values as f64. Validate the actual scalar
/// tokens before it can round two different documents to the same value. This
/// is a token pass, not a text regex: comments, quoted strings, block strings,
/// and aliases retain their real YAML meaning and source locations.
fn validate_yaml_numeric_precision(input: &str) -> Result<(), String> {
    use yaml_rust2::parser::{Event, Parser};
    use yaml_rust2::scanner::TScalarStyle;
    let mut parser = Parser::new_from_str(input);
    loop {
        let (event, mark) = parser.next_token().map_err(|e| e.to_string())?;
        match event {
            Event::StreamEnd => break,
            Event::Scalar(value, style, _, tag) => {
                let full_tag = tag
                    .as_ref()
                    .map(|tag| format!("{}{}", tag.handle, tag.suffix));
                let numeric_tag = match full_tag.as_deref() {
                    Some("tag:yaml.org,2002:int") => Some("int"),
                    Some("tag:yaml.org,2002:float") => Some("float"),
                    _ => None,
                };
                if numeric_tag.is_none() && (tag.is_some() || style != TScalarStyle::Plain) {
                    continue;
                }
                let Some(expected) = exact_yaml_numeric_token(&value, numeric_tag.is_some()) else {
                    continue;
                };
                let expected = expected.and_then(normalize_number).map_err(|error| {
                    format!("{error} at line {} column {}", mark.line(), mark.col() + 1)
                })?;
                let expression = if let Some(tag) = numeric_tag {
                    format!(
                        "!!{tag} {}",
                        serde_json::to_string(&value).map_err(|e| e.to_string())?
                    )
                } else {
                    value
                };
                let actual = match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&expression) {
                    Ok(serde_yaml_ng::Value::Number(number)) => number
                        .to_string()
                        .parse::<serde_json::Number>()
                        .ok()
                        .and_then(|number| normalize_number(number).ok()),
                    _ => None,
                };
                if actual.as_ref() != Some(&expected) {
                    return Err(format!("YAML numeric scalar at line {} column {} would lose precision or change type. Use text comparison for this document.", mark.line(), mark.col() + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_yaml(input: &str) -> Result<Value, String> {
    validate_yaml_numeric_precision(input)?;
    // Mapping's Deserialize rejects duplicate keys before aliases/merges can
    // erase evidence. apply_merge implements YAML merge precedence explicitly.
    let mut value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(input).map_err(|e| e.to_string())?;
    value
        .apply_merge()
        .map_err(|e| format!("invalid YAML merge: {e}"))?;
    convert_yaml(value, "")
}

fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn normalized_string(value: &str, ignore_case: bool) -> Value {
    Value::String(if ignore_case {
        value.to_lowercase()
    } else {
        value.to_owned()
    })
}

fn expanded_name(namespace: Option<&str>, local: &str) -> String {
    namespace.map_or_else(|| local.to_owned(), |uri| format!("{{{uri}}}{local}"))
}

fn xml_node(node: roxmltree::Node<'_, '_>, ignore_case: bool) -> Option<Value> {
    match node.node_type() {
        roxmltree::NodeType::Element => {
            let attributes = node
                .attributes()
                .map(|attribute| {
                    (
                        expanded_name(attribute.namespace(), attribute.name()),
                        normalized_string(attribute.value(), ignore_case),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect::<Map<_, _>>();
            Some(object([
                ("kind", Value::String("element".into())),
                (
                    "name",
                    Value::String(expanded_name(
                        node.tag_name().namespace(),
                        node.tag_name().name(),
                    )),
                ),
                ("attributes", Value::Object(attributes)),
                (
                    "children",
                    Value::Array(
                        node.children()
                            .filter_map(|child| xml_node(child, ignore_case))
                            .collect(),
                    ),
                ),
            ]))
        }
        roxmltree::NodeType::Text => Some(object([
            ("kind", Value::String("text".into())),
            (
                "value",
                normalized_string(node.text().unwrap_or_default(), ignore_case),
            ),
        ])),
        roxmltree::NodeType::Comment => Some(object([
            ("kind", Value::String("comment".into())),
            (
                "value",
                normalized_string(node.text().unwrap_or_default(), ignore_case),
            ),
        ])),
        roxmltree::NodeType::PI => node.pi().map(|pi| {
            object([
                ("kind", Value::String("processingInstruction".into())),
                ("target", Value::String(pi.target.to_owned())),
                (
                    "value",
                    normalized_string(pi.value.unwrap_or_default(), ignore_case),
                ),
            ])
        }),
        roxmltree::NodeType::Root => None,
    }
}

fn parse_xml(input: &str, ignore_case: bool) -> Result<Value, String> {
    // No DTD expansion and no resolver: parsing never makes filesystem or
    // network requests. Standard XML character references remain supported.
    let document = roxmltree::Document::parse_with_options(
        input,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            ..roxmltree::ParsingOptions::default()
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(object([(
        "$xml",
        Value::Array(
            document
                .root()
                .children()
                .filter_map(|node| xml_node(node, ignore_case))
                .collect(),
        ),
    )]))
}

/// Canonical exact decimal arithmetic: coefficient × 10^exponent. Only the
/// exponent is a big integer; a huge exponent never expands to that many zeros.
/// This keeps 1, 1.0 and 1e0 equal without rounding large integers or decimals.
fn normalize_number(value: serde_json::Number) -> Result<serde_json::Number, String> {
    let token = value.to_string();
    let (negative, unsigned) = token
        .strip_prefix('-')
        .map_or((false, token.as_str()), |s| (true, s));
    let (mantissa, raw_exponent) = match unsigned.find(['e', 'E']) {
        Some(index) => (&unsigned[..index], &unsigned[index + 1..]),
        None => (unsigned, "0"),
    };
    let fractional = mantissa
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(serde_json::Number::from(0));
    }
    let coefficient = digits.trim_end_matches('0');
    let trailing = digits.len() - coefficient.len();
    let exponent = raw_exponent
        .parse::<BigInt>()
        .map_err(|e| format!("invalid numeric exponent: {e}"))?
        - BigInt::from(fractional)
        + BigInt::from(trailing);
    let exponent_text = exponent.to_string();
    // Keep ordinary configuration values readable. At most six presentation
    // zeros are added; larger magnitudes use compact scientific notation.
    let body = match exponent_text
        .parse::<i32>()
        .ok()
        .filter(|value| (-6..=6).contains(value))
    {
        Some(value) if value >= 0 => format!("{coefficient}{}", "0".repeat(value as usize)),
        Some(value) => {
            let scale = (-value) as usize;
            if scale < coefficient.len() {
                let point = coefficient.len() - scale;
                format!("{}.{}", &coefficient[..point], &coefficient[point..])
            } else {
                format!("0.{}{coefficient}", "0".repeat(scale - coefficient.len()))
            }
        }
        None => format!("{coefficient}e{exponent_text}"),
    };
    let canonical = if negative { format!("-{body}") } else { body };
    canonical
        .parse::<serde_json::Number>()
        .map_err(|e| e.to_string())
}

/// Sort JSON/YAML arrays as multisets only when requested. Object keys and XML
/// element/attribute names always remain case-sensitive to avoid collisions.
fn normalize_data(value: Value, options: &StructuredOptions) -> Result<Value, String> {
    match value {
        Value::Number(value) => normalize_number(value).map(Value::Number),
        Value::String(value) => Ok(normalized_string(&value, options.ignore_case)),
        Value::Array(values) => {
            let mut values = values
                .into_iter()
                .map(|value| normalize_data(value, options))
                .collect::<Result<Vec<_>, _>>()?;
            if options.ignore_array_order {
                let mut sortable = values
                    .into_iter()
                    .map(|value| {
                        serde_json::to_string(&value)
                            .map(|key| (key, value))
                            .map_err(|e| e.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                sortable.sort_by(|(a, _), (b, _)| a.cmp(b));
                values = sortable.into_iter().map(|(_, value)| value).collect();
            }
            Ok(Value::Array(values))
        }
        Value::Object(values) => values
            .into_iter()
            .map(|(key, value)| normalize_data(value, options).map(|value| (key, value)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(|values| Value::Object(values.into_iter().collect())),
        value => Ok(value),
    }
}

fn parse(input: &str, side: &str, options: &StructuredOptions) -> Result<Value, String> {
    let result = match options.format {
        StructuredFormat::Json => {
            parse_json(input).and_then(|value| normalize_data(value, options))
        }
        StructuredFormat::Yaml => {
            parse_yaml(input).and_then(|value| normalize_data(value, options))
        }
        StructuredFormat::Xml => parse_xml(input, options.ignore_case),
    };
    result.map_err(|error| format!("{side} {:?}: {error}", options.format))
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn value_string(value: &Value) -> String {
    // Value serializes infallibly; arbitrary-precision JSON numbers are kept as
    // numeric tokens rather than floating point conversions or quoted strings.
    value.to_string()
}

fn collect_changes(
    left: Option<&Value>,
    right: Option<&Value>,
    path: &str,
    changes: &mut Vec<StructureChange>,
) {
    match (left, right) {
        (Some(left), Some(right)) if left == right => {}
        (Some(Value::Object(left)), Some(Value::Object(right))) => {
            let keys = left.keys().chain(right.keys()).collect::<BTreeSet<_>>();
            for key in keys {
                collect_changes(
                    left.get(key),
                    right.get(key),
                    &pointer_child(path, key),
                    changes,
                );
            }
        }
        (Some(Value::Array(left)), Some(Value::Array(right))) => {
            for index in 0..left.len().max(right.len()) {
                collect_changes(
                    left.get(index),
                    right.get(index),
                    &pointer_child(path, &index.to_string()),
                    changes,
                );
            }
        }
        (left, right) => {
            let kind = match (left, right) {
                (None, Some(_)) => StructureChangeKind::Added,
                (Some(_), None) => StructureChangeKind::Removed,
                (Some(left), Some(right)) if type_name(left) != type_name(right) => {
                    StructureChangeKind::TypeChanged
                }
                _ => StructureChangeKind::Changed,
            };
            changes.push(StructureChange {
                path: path.to_owned(),
                kind,
                left: left.map(value_string),
                right: right.map(value_string),
            });
        }
    }
}

pub fn compare_structured(
    left: &str,
    right: &str,
    options: &StructuredOptions,
) -> Result<StructuredResult, String> {
    let left = parse(left, "Left", options)?;
    let right = parse(right, "Right", options)?;
    let mut changes = Vec::new();
    collect_changes(Some(&left), Some(&right), "", &mut changes);
    Ok(StructuredResult {
        equal: changes.is_empty(),
        changes,
        left_canonical: serde_json::to_string_pretty(&left).map_err(|e| e.to_string())?,
        right_canonical: serde_json::to_string_pretty(&right).map_err(|e| e.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(format: StructuredFormat) -> StructuredOptions {
        StructuredOptions {
            format,
            ..StructuredOptions::default()
        }
    }

    #[test]
    fn json_order_types_big_numbers_and_pointer_escaping() {
        let opts = options(StructuredFormat::Json);
        assert!(
            compare_structured(r#"{"b":2,"a":1}"#, r#"{"a":1,"b":2}"#, &opts)
                .unwrap()
                .equal
        );
        let result = compare_structured(r#"{"a/b~c":1}"#, r#"{"a/b~c":"1"}"#, &opts).unwrap();
        assert_eq!(result.changes[0].path, "/a~1b~0c");
        assert_eq!(result.changes[0].kind, StructureChangeKind::TypeChanged);
        let result = compare_structured("9007199254740992", "9007199254740993", &opts).unwrap();
        assert!(!result.equal);
        assert_eq!(result.right_canonical, "9007199254740993");
        let result = compare_structured(
            "123456789012345678901234567890",
            "123456789012345678901234567891",
            &opts,
        )
        .unwrap();
        assert!(!result.equal);
    }

    #[test]
    fn numbers_use_exact_decimal_equality_without_expanding_exponents() {
        let opts = options(StructuredFormat::Json);
        for (left, right) in [
            ("1", "1.0"),
            ("1.00", "1e0"),
            ("-0", "0.0"),
            ("0.125", "125e-3"),
            ("1000", "1e+03"),
        ] {
            assert!(
                compare_structured(left, right, &opts).unwrap().equal,
                "{left} vs {right}"
            );
        }
        assert!(
            !compare_structured("0.123456789012345678901", "0.123456789012345678902", &opts)
                .unwrap()
                .equal
        );
        let result = compare_structured(
            "1e9999999999999999999999999999999999999999",
            "10e9999999999999999999999999999999999999998",
            &opts,
        )
        .unwrap();
        assert!(result.equal);
        assert!(result.left_canonical.len() < 64);
        let result = compare_structured(
            "1e-9999999999999999999999999999999999999998",
            "10e-9999999999999999999999999999999999999999",
            &opts,
        )
        .unwrap();
        assert!(result.equal);
        assert!(result.left_canonical.len() < 64);
        let mut unordered = opts;
        unordered.ignore_array_order = true;
        assert!(
            compare_structured("[1.0, 2]", "[2, 1e0]", &unordered)
                .unwrap()
                .equal
        );
    }

    #[test]
    fn json_parser_private_key_spellings_remain_user_data() {
        let opts = options(StructuredFormat::Json);
        let result =
            compare_structured(r#"{"$serde_json::private::Number":"123"}"#, "123", &opts).unwrap();
        assert!(!result.equal);
        assert_eq!(result.changes[0].kind, StructureChangeKind::TypeChanged);
        assert!(result
            .left_canonical
            .contains("$serde_json::private::Number"));
        let result = compare_structured(
            r#"{"$serde_json::private::RawValue":"{\"x\":1}"}"#,
            r#"{"x":1}"#,
            &opts,
        )
        .unwrap();
        assert!(!result.equal);
        assert!(result
            .left_canonical
            .contains("$serde_json::private::RawValue"));
    }

    #[test]
    fn duplicates_are_errors_with_the_affected_side() {
        let result = compare_structured(
            "{}",
            "{\n\"x\": 1,\n\"x\": 2\n}",
            &options(StructuredFormat::Json),
        );
        let error = result.unwrap_err();
        assert!(error.contains("Right"), "{error}");
        assert!(error.contains("duplicate"), "{error}");
        assert!(error.contains("line 3"), "{error}");
        let result = compare_structured("a: 1\na: 2", "a: 2", &options(StructuredFormat::Yaml));
        assert!(result.unwrap_err().contains("duplicate"));
    }

    #[test]
    fn case_rules_do_not_collapse_object_keys() {
        let mut opts = options(StructuredFormat::Json);
        opts.ignore_case = true;
        assert!(
            compare_structured(r#"{"name":"JOJO"}"#, r#"{"name":"jojo"}"#, &opts)
                .unwrap()
                .equal
        );
        assert!(
            !compare_structured(r#"{"A":1}"#, r#"{"a":1}"#, &opts)
                .unwrap()
                .equal
        );
        assert!(
            !compare_structured(r#"{"A":1,"a":2}"#, r#"{"A":2,"a":1}"#, &opts)
                .unwrap()
                .equal
        );
    }

    #[test]
    fn arrays_are_multisets_only_by_explicit_option() {
        let mut opts = options(StructuredFormat::Json);
        assert!(
            !compare_structured("[1,2,1]", "[1,1,2]", &opts)
                .unwrap()
                .equal
        );
        opts.ignore_array_order = true;
        assert!(
            compare_structured("[1,2,1]", "[1,1,2]", &opts)
                .unwrap()
                .equal
        );
        assert!(!compare_structured("[1,2,1]", "[1,2]", &opts).unwrap().equal);
    }

    #[test]
    fn yaml_alias_merge_and_explicit_unsupported_values() {
        let opts = options(StructuredFormat::Yaml);
        assert!(
            compare_structured(
                "default: &d {port: 80}\nserver: {<<: *d, host: example}",
                "server: {host: example, port: 80}\ndefault: {port: 80}",
                &opts
            )
            .unwrap()
            .equal
        );
        assert!(
            !compare_structured("port: 80", "port: '80'", &opts)
                .unwrap()
                .equal
        );
        assert!(compare_structured("? [a,b]\n: c", "{}", &opts)
            .unwrap_err()
            .contains("non-string key"));
        assert!(compare_structured("value: !custom hello", "{}", &opts)
            .unwrap_err()
            .contains("custom YAML tag"));
        assert!(compare_structured("value: .inf", "{}", &opts)
            .unwrap_err()
            .contains("non-finite"));
    }

    #[test]
    fn yaml_never_reports_rounded_numeric_values_equal() {
        let opts = options(StructuredFormat::Yaml);
        for source in [
            "value: 0.123456789012345678901",
            "value: 9007199254740993.0",
            "value: 1e99999999999999999999999999",
            "value: !!float '0.123456789012345678901'",
            "value: 1e-400",
        ] {
            let error = compare_structured(source, source, &opts).unwrap_err();
            assert!(error.contains("lose precision"), "{source}: {error}");
            assert!(error.contains("line 1 column"), "{error}");
        }
        assert!(
            compare_structured("value: 0.1", "value: 1e-1", &opts)
                .unwrap()
                .equal
        );
        assert!(
            compare_structured("value: -0.125", "value: -125e-3", &opts)
                .unwrap()
                .equal
        );
        assert!(
            compare_structured("value: .5", "value: 0.5", &opts)
                .unwrap()
                .equal
        );
        assert!(
            compare_structured("value: 0xFF", "value: 255", &opts)
                .unwrap()
                .equal
        );
        assert!(
            compare_structured(
                "value: '0.123456789012345678901'",
                "value: !!str 0.123456789012345678901",
                &opts
            )
            .unwrap()
            .equal
        );
        assert!(compare_structured(
            "value: |\n  0.123456789012345678901\n",
            "value: \"0.123456789012345678901\\n\"",
            &opts
        )
        .is_ok());
        assert!(
            !compare_structured("value: 01", "value: 1", &opts)
                .unwrap()
                .equal
        );
    }

    #[test]
    fn xml_attributes_and_prefixes_are_semantic_but_children_keep_order() {
        let mut opts = options(StructuredFormat::Xml);
        assert!(
            compare_structured(
                "<p:r xmlns:p='urn:x' b='2' a='1'/>",
                "<q:r a='1' xmlns:q='urn:x' b='2'></q:r>",
                &opts
            )
            .unwrap()
            .equal
        );
        assert!(
            !compare_structured("<r><a/><b/></r>", "<r><b/><a/></r>", &opts)
                .unwrap()
                .equal
        );
        opts.ignore_array_order = true;
        assert!(
            !compare_structured("<r><a/><b/></r>", "<r><b/><a/></r>", &opts)
                .unwrap()
                .equal
        );
        assert!(
            !compare_structured("<r>before<b/>after</r>", "<r>after<b/>before</r>", &opts)
                .unwrap()
                .equal
        );
        assert!(!compare_structured("<r> </r>", "<r/>", &opts).unwrap().equal);
        assert!(
            compare_structured("<r>&#65;&amp;</r>", "<r>A&amp;</r>", &opts)
                .unwrap()
                .equal
        );
    }

    #[test]
    fn xml_case_rules_keep_names_and_dtd_is_not_loaded() {
        let mut opts = options(StructuredFormat::Xml);
        opts.ignore_case = true;
        assert!(
            compare_structured("<r a='VALUE'>TEXT</r>", "<r a='value'>text</r>", &opts)
                .unwrap()
                .equal
        );
        assert!(!compare_structured("<R/>", "<r/>", &opts).unwrap().equal);
        assert!(compare_structured(
            "<!DOCTYPE r [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><r>&x;</r>",
            "<r/>",
            &opts
        )
        .is_err());
        assert!(compare_structured("<r a='1' a='2'/>", "<r/>", &opts).is_err());
    }
}
