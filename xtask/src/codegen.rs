//! Generates client model types from the OpenAPI document.
//!
//! One pass, two emitters, so TypeScript and Swift cannot disagree about the
//! contract or drift from the Rust that produced it.
//!
//! The supported subset is deliberately narrow — objects, `allOf` composition
//! from `#[serde(flatten)]`, string enums, arrays, `$ref`, and nullable unions.
//! Anything else is an error rather than a guess: emitting `any` for a schema
//! this does not understand would quietly reopen the hole the whole exercise
//! exists to close.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value;

pub struct Schemas(BTreeMap<String, Value>);

pub fn load(document: &Value) -> Result<Schemas, String> {
    let schemas = document
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .ok_or("the document has no components/schemas")?;
    Ok(Schemas(
        schemas
            .iter()
            .map(|(name, schema)| (name.clone(), schema.clone()))
            .collect(),
    ))
}

/// `#/components/schemas/Work` -> `Work`
fn ref_name(schema: &Value) -> Option<&str> {
    schema
        .get("$ref")?
        .as_str()?
        .strip_prefix("#/components/schemas/")
}

fn doc_comment(schema: &Value, indent: &str, marker: &str) -> String {
    match schema.get("description").and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => text
            .lines()
            .map(|line| format!("{indent}{marker} {line}\n"))
            .collect(),
        _ => String::new(),
    }
}

/// Peels nullability off a schema and reports it separately.
///
/// Two forms arrive, depending on what was optional. `Option<String>` becomes
/// `type: ["string", "null"]`, while `Option<SomeStruct>` cannot — a `$ref`
/// carries no type — so it becomes `oneOf: [{type: null}, {$ref}]` instead.
fn split_nullable(schema: &Value) -> (Value, bool) {
    if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
        let is_null = |v: &Value| v.get("type").and_then(Value::as_str) == Some("null");
        let nullable = variants.iter().any(is_null);
        let rest: Vec<&Value> = variants.iter().filter(|v| !is_null(v)).collect();
        // A single variant is that variant: a struct field with a serde
        // default comes out as `oneOf: [{$ref}]` beside its default value.
        if rest.len() == 1 {
            return (rest[0].clone(), nullable);
        }
    }
    match schema.get("type") {
        Some(Value::Array(kinds)) => {
            let nullable = kinds.iter().any(|k| k == "null");
            let remaining: Vec<&Value> = kinds.iter().filter(|k| *k != "null").collect();
            if remaining.len() == 1 {
                let mut narrowed = schema.clone();
                narrowed["type"] = remaining[0].clone();
                (narrowed, nullable)
            } else {
                (schema.clone(), nullable)
            }
        }
        _ => (schema.clone(), false),
    }
}

struct Field {
    name: String,
    schema: Value,
    required: bool,
}

/// Flattens `allOf` composition into one field list, since that is what
/// `#[serde(flatten)]` produces on the wire: a single flat object.
fn fields_of(name: &str, schema: &Value, all: &Schemas) -> Result<Vec<Field>, String> {
    if let Some(parts) = schema.get("allOf").and_then(Value::as_array) {
        let mut out = Vec::new();
        for part in parts {
            let resolved = match ref_name(part) {
                Some(target) => all
                    .0
                    .get(target)
                    .ok_or_else(|| format!("{name}: allOf refers to unknown schema {target}"))?,
                None => part,
            };
            out.extend(fields_of(name, resolved, all)?);
        }
        return Ok(out);
    }

    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };

    Ok(properties
        .iter()
        .map(|(field, field_schema)| Field {
            name: field.clone(),
            schema: field_schema.clone(),
            required: required.contains(&field.as_str()),
        })
        .collect())
}

/// True when a schema says nothing about its shape.
///
/// `serde_json::Value` generates as `{}`, and a documented one as
/// `{"description": "..."}` — metadata but no structure. In OpenAPI an absent
/// type means "any JSON", so both map to the untyped escape hatch. Checked by
/// the presence of structural keys rather than by emptiness, so a description
/// does not change how a field is typed.
fn is_untyped(schema: &Value) -> bool {
    const STRUCTURAL: &[&str] = &[
        "type",
        "$ref",
        "allOf",
        "oneOf",
        "anyOf",
        "enum",
        "items",
        "properties",
        "additionalProperties",
    ];
    match schema.as_object() {
        Some(object) => !STRUCTURAL.iter().any(|key| object.contains_key(*key)),
        None => true,
    }
}

fn string_enum(schema: &Value) -> Option<Vec<String>> {
    if schema.get("type").and_then(Value::as_str) != Some("string") {
        return None;
    }
    Some(
        schema
            .get("enum")?
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    )
}

// --- TypeScript ----------------------------------------------------------

fn ts_type(owner: &str, field: &str, schema: &Value) -> Result<String, String> {
    let (schema, nullable) = split_nullable(schema);
    let base = if let Some(target) = ref_name(&schema) {
        target.to_string()
    } else if is_untyped(&schema) {
        "JsonValue".to_string()
    } else {
        match schema.get("type").and_then(Value::as_str) {
            Some("string") => "string".to_string(),
            Some("integer") | Some("number") => "number".to_string(),
            Some("boolean") => "boolean".to_string(),
            Some("object") => "JsonValue".to_string(),
            Some("array") => {
                let items = schema
                    .get("items")
                    .ok_or_else(|| format!("{owner}.{field}: array without items"))?;
                format!("{}[]", ts_type(owner, field, items)?)
            }
            other => {
                return Err(format!(
                    "{owner}.{field}: unsupported schema type {other:?} — teach codegen.rs about it"
                ))
            }
        }
    };
    Ok(if nullable {
        format!("{base} | null")
    } else {
        base
    })
}

pub fn typescript(all: &Schemas) -> Result<String, String> {
    let mut out = String::new();
    out.push_str(
        "// @generated by `cargo xtask codegen` from docs/openapi.json. Do not edit.\n\
         //\n\
         // Every type here is derived from the Rust handlers. Renaming a field in\n\
         // the backend and regenerating will break this file's consumers, which is\n\
         // the point.\n\n\
         /** Arbitrary JSON, where the server accepts or returns `serde_json::Value`. */\n\
         export type JsonValue = unknown\n",
    );

    for (name, schema) in &all.0 {
        out.push('\n');
        if let Some(variants) = string_enum(schema) {
            out.push_str(&doc_comment(schema, "", "//"));
            let union = variants
                .iter()
                .map(|v| format!("'{v}'"))
                .collect::<Vec<_>>()
                .join(" | ");
            let _ = writeln!(out, "export type {name} = {union}");
            continue;
        }

        let fields = fields_of(name, schema, all)?;
        out.push_str(&doc_comment(schema, "", "//"));
        let _ = writeln!(out, "export interface {name} {{");
        for field in &fields {
            out.push_str(&doc_comment(&field.schema, "  ", "//"));
            let ty = ts_type(name, &field.name, &field.schema)?;
            let optional = if field.required { "" } else { "?" };
            let _ = writeln!(out, "  {}{}: {}", field.name, optional, ty);
        }
        out.push_str("}\n");
    }
    Ok(out)
}

// --- Swift ---------------------------------------------------------------

/// Swift keywords that would otherwise need escaping as identifiers.
const SWIFT_RESERVED: &[&str] = &[
    "public",
    "private",
    "internal",
    "static",
    "case",
    "default",
    "class",
    "struct",
    "enum",
    "protocol",
    "extension",
    "func",
    "let",
    "var",
    "return",
    "where",
    "self",
    "Type",
    "operator",
    "in",
    "for",
    "if",
    "else",
    "switch",
    "import",
    "repeat",
    "continue",
    "break",
];

fn swift_ident(name: &str) -> String {
    if SWIFT_RESERVED.contains(&name) {
        format!("`{name}`")
    } else {
        name.to_string()
    }
}

/// An enum case for a raw value that may not be an identifier: "1.15"
/// becomes `_1_15`.
fn swift_case(variant: &str) -> String {
    let mut name: String = variant
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        name.insert(0, '_');
    }
    swift_ident(&name)
}

fn swift_type(owner: &str, field: &str, schema: &Value) -> Result<String, String> {
    let (schema, nullable) = split_nullable(schema);
    let base = if let Some(target) = ref_name(&schema) {
        target.to_string()
    } else if is_untyped(&schema) {
        "JSONValue".to_string()
    } else {
        match schema.get("type").and_then(Value::as_str) {
            Some("string") => "String".to_string(),
            Some("integer") => "Int".to_string(),
            Some("number") => "Double".to_string(),
            Some("boolean") => "Bool".to_string(),
            Some("object") => "JSONValue".to_string(),
            Some("array") => {
                let items = schema
                    .get("items")
                    .ok_or_else(|| format!("{owner}.{field}: array without items"))?;
                format!("[{}]", swift_type(owner, field, items)?)
            }
            other => {
                return Err(format!(
                    "{owner}.{field}: unsupported schema type {other:?} — teach codegen.rs about it"
                ))
            }
        }
    };
    Ok(if nullable { format!("{base}?") } else { base })
}

pub fn swift(all: &Schemas) -> Result<String, String> {
    let mut out = String::new();
    out.push_str(
        "// @generated by `cargo xtask codegen` from docs/openapi.json. Do not edit.\n\
         //\n\
         // Every type here is derived from the Rust handlers. Renaming a field in\n\
         // the backend and regenerating will break this file's consumers, which is\n\
         // the point.\n\n\
         import Foundation\n\n\
         /// Namespace for the generated API types.\n\
         ///\n\
         /// Nested rather than top-level because the server's vocabulary overlaps\n\
         /// SwiftUI's: bare `Scene` and `Visibility` would be ambiguous in every\n\
         /// view file. `API.Scene` is unambiguous and says where the type came from.\n\
         public enum API {\n",
    );

    for (name, schema) in &all.0 {
        out.push('\n');
        if let Some(variants) = string_enum(schema) {
            out.push_str(&doc_comment(schema, "    ", "///"));
            let _ = writeln!(
                out,
                "    public enum {name}: String, Codable, CaseIterable, Hashable, Sendable {{"
            );
            for variant in &variants {
                let _ = writeln!(out, "        case {} = \"{variant}\"", swift_case(variant));
            }
            out.push_str("    }\n");
            continue;
        }

        let fields = fields_of(name, schema, all)?;
        out.push_str(&doc_comment(schema, "    ", "///"));
        // `Hashable` rather than `Equatable`: SwiftUI navigation values need it,
        // and it implies the other.
        let _ = writeln!(
            out,
            "    public struct {name}: Codable, Hashable, Sendable {{"
        );
        for field in &fields {
            out.push_str(&doc_comment(&field.schema, "        ", "///"));
            let mut ty = swift_type(name, &field.name, &field.schema)?;
            if !field.required && !ty.ends_with('?') {
                ty.push('?');
            }
            let _ = writeln!(
                out,
                "        public var {}: {}",
                swift_ident(&field.name),
                ty
            );
        }

        // An explicit public memberwise init: the implicit one is internal, and
        // these types are used from the UI module.
        if !fields.is_empty() {
            out.push_str("\n        public init(\n");
            for (index, field) in fields.iter().enumerate() {
                let mut ty = swift_type(name, &field.name, &field.schema)?;
                if !field.required && !ty.ends_with('?') {
                    ty.push('?');
                }
                let default = if ty.ends_with('?') { " = nil" } else { "" };
                let comma = if index + 1 == fields.len() { "" } else { "," };
                let _ = writeln!(
                    out,
                    "            {}: {}{}{}",
                    swift_ident(&field.name),
                    ty,
                    default,
                    comma
                );
            }
            out.push_str("        ) {\n");
            for field in &fields {
                let ident = swift_ident(&field.name);
                let _ = writeln!(out, "            self.{ident} = {ident}");
            }
            out.push_str("        }\n");
        }
        out.push_str("    }\n");
    }
    out.push_str("}\n");
    Ok(out)
}
