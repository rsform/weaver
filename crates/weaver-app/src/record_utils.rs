use dioxus::prelude::*;
use jacquard::bytes::Bytes;
use jacquard::common::{Data, IntoStatic};
use jacquard::smol_str::{SmolStr, format_smolstr};
use jacquard::types::LexiconStringType;
use jacquard::types::string::AtprotoStr;
use jacquard_lexicon::validation::{
    ConstraintError, StructuralError, ValidationError, ValidationResult,
};

// ============================================================================
// Validation Helper Functions
// ============================================================================

/// Parse UI path into segments (fields and indices only)
fn parse_ui_path(ui_path: &str) -> Vec<UiPathSegment> {
    if ui_path.is_empty() {
        return vec![];
    }

    let mut segments = Vec::new();
    let mut current = String::new();

    for ch in ui_path.chars() {
        match ch {
            '.' => {
                if !current.is_empty() {
                    segments.push(UiPathSegment::Field(current.clone()));
                    current.clear();
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(UiPathSegment::Field(current.clone()));
                    current.clear();
                }
            }
            ']' => {
                if !current.is_empty() {
                    if let Ok(idx) = current.parse::<usize>() {
                        segments.push(UiPathSegment::Index(idx));
                    }
                    current.clear();
                }
            }
            c => current.push(c),
        }
    }

    if !current.is_empty() {
        segments.push(UiPathSegment::Field(current));
    }

    segments
}

#[derive(Debug, PartialEq)]
enum UiPathSegment {
    Field(String),
    Index(usize),
}

/// Get all validation errors at exactly this path (not children)
pub fn get_errors_at_exact_path(
    validation_result: &Option<ValidationResult>,
    ui_path: &str,
) -> Vec<String> {
    use jacquard_lexicon::validation::PathSegment;

    if let Some(result) = validation_result {
        let ui_segments = parse_ui_path(ui_path);

        result
            .all_errors()
            .filter_map(|err| {
                let validation_path = match &err {
                    ValidationError::Structural(s) => match s {
                        StructuralError::TypeMismatch { path, .. } => Some(path),
                        StructuralError::MissingRequiredField { path, .. } => Some(path),
                        StructuralError::MissingUnionDiscriminator { path } => Some(path),
                        StructuralError::UnionNoMatch { path, .. } => Some(path),
                        StructuralError::UnresolvedRef { path, .. } => Some(path),
                        StructuralError::RefCycle { path, .. } => Some(path),
                        StructuralError::MaxDepthExceeded { path, .. } => Some(path),
                    },
                    ValidationError::Constraint(c) => match c {
                        ConstraintError::MaxLength { path, .. } => Some(path),
                        ConstraintError::MaxGraphemes { path, .. } => Some(path),
                        ConstraintError::MinLength { path, .. } => Some(path),
                        ConstraintError::MinGraphemes { path, .. } => Some(path),
                        ConstraintError::Maximum { path, .. } => Some(path),
                        ConstraintError::Minimum { path, .. } => Some(path),
                    },
                };

                if let Some(path) = validation_path {
                    // Convert validation path to UI segments
                    let validation_ui_segments: Vec<_> = path
                        .segments()
                        .iter()
                        .filter_map(|seg| match seg {
                            PathSegment::Field(name) => {
                                Some(UiPathSegment::Field(name.to_string()))
                            }
                            PathSegment::Index(idx) => Some(UiPathSegment::Index(*idx)),
                            PathSegment::UnionVariant(_) => None,
                        })
                        .collect();

                    // Exact match only
                    if validation_ui_segments == ui_segments {
                        return Some(err.to_string());
                    }
                }
                None
            })
            .collect()
    } else {
        Vec::new()
    }
}

// ============================================================================
// Pretty Editor: Helper Functions
// ============================================================================

/// Infer Data type from text input
pub fn infer_data_from_text(text: &str) -> Result<Data<'static>, String> {
    let trimmed = text.trim();

    if trimmed == "true" || trimmed == "false" {
        Ok(Data::Boolean(trimmed == "true"))
    } else if trimmed == "{}" {
        use jacquard::types::value::Object;
        use std::collections::BTreeMap;
        Ok(Data::Object(Object(BTreeMap::new())))
    } else if trimmed == "[]" {
        use jacquard::types::value::Array;
        Ok(Data::Array(Array(Vec::new())))
    } else if trimmed == "null" {
        Ok(Data::Null)
    } else if let Ok(num) = trimmed.parse::<i64>() {
        Ok(Data::Integer(num))
    } else {
        // Smart string parsing
        use jacquard::types::value::parsing;
        Ok(Data::String(parsing::parse_string(trimmed).into_static()))
    }
}

/// Parse text as specific AtprotoStr type, preserving type information
pub fn try_parse_as_type(
    text: &str,
    string_type: LexiconStringType,
) -> Result<AtprotoStr<'static>, String> {
    use jacquard::types::string::*;
    use std::str::FromStr;

    match string_type {
        LexiconStringType::Datetime => Datetime::from_str(text)
            .map(AtprotoStr::Datetime)
            .map_err(|e| format_smolstr!("Invalid datetime: {}", e).to_string()),
        LexiconStringType::Did => Did::new(text)
            .map(|v| AtprotoStr::Did(v.into_static()))
            .map_err(|e| format_smolstr!("Invalid DID: {}", e).to_string()),
        LexiconStringType::Handle => Handle::new(text)
            .map(|v| AtprotoStr::Handle(v.into_static()))
            .map_err(|e| format_smolstr!("Invalid handle: {}", e).to_string()),
        LexiconStringType::AtUri => AtUri::new(text)
            .map(|v| AtprotoStr::AtUri(v.into_static()))
            .map_err(|e| format_smolstr!("Invalid AT-URI: {}", e).to_string()),
        LexiconStringType::AtIdentifier => AtIdentifier::new(text)
            .map(|v| AtprotoStr::AtIdentifier(v.into_static()))
            .map_err(|e| format_smolstr!("Invalid identifier: {}", e).to_string()),
        LexiconStringType::Nsid => Nsid::new(text)
            .map(|v| AtprotoStr::Nsid(v.into_static()))
            .map_err(|e| format_smolstr!("Invalid NSID: {}", e).to_string()),
        LexiconStringType::Tid => Tid::new(text)
            .map(|v| AtprotoStr::Tid(v.into_static()))
            .map_err(|e| format_smolstr!("Invalid TID: {}", e).to_string()),
        LexiconStringType::RecordKey => Rkey::new(text)
            .map(|rk| AtprotoStr::RecordKey(RecordKey::from(rk)))
            .map_err(|e| format_smolstr!("Invalid record key: {}", e).to_string()),
        LexiconStringType::Cid => Cid::new(text.as_bytes())
            .map(|v| AtprotoStr::Cid(v.into_static()))
            .map_err(|_| SmolStr::new_inline("Invalid CID").to_string()),
        LexiconStringType::Language => Language::new(text)
            .map(AtprotoStr::Language)
            .map_err(|e| format_smolstr!("Invalid language: {}", e).to_string()),
        LexiconStringType::Uri(_) => Uri::new(text)
            .map(|u| AtprotoStr::Uri(u.into_static()))
            .map_err(|e| format_smolstr!("Invalid URI: {}", e).to_string()),
        LexiconStringType::String => {
            // Plain strings: use smart inference
            use jacquard::types::value::parsing;
            Ok(parsing::parse_string(text).into_static())
        }
    }
}

/// Create default value for new array item by cloning structure of existing items
pub fn create_array_item_default(arr: &jacquard::types::value::Array) -> Data<'static> {
    if let Some(existing) = arr.0.first() {
        clone_structure(existing)
    } else {
        // Empty array, default to null (user can change type)
        Data::Null
    }
}

/// Clone structure of Data, setting sensible defaults for leaf values
pub fn clone_structure(data: &Data) -> Data<'static> {
    use jacquard::types::string::*;
    use jacquard::types::value::{Array, Object};
    use jacquard::types::{LexiconStringType, blob::*};
    use std::collections::BTreeMap;

    match data {
        Data::Object(obj) => {
            let mut new_obj = BTreeMap::new();
            for (key, value) in obj.0.iter() {
                new_obj.insert(key.clone(), clone_structure(value));
            }
            Data::Object(Object(new_obj))
        }
        Data::Array(_) => Data::Array(Array(Vec::new())),
        Data::String(s) => match s.string_type() {
            LexiconStringType::Datetime => {
                // Sensible default: now
                Data::String(AtprotoStr::Datetime(Datetime::now()))
            }
            LexiconStringType::Tid => Data::String(AtprotoStr::Tid(Tid::now_0())),
            _ => {
                // Empty string, type inference will handle it
                Data::String(AtprotoStr::String("".into()))
            }
        },
        Data::Integer(_) => Data::Integer(0),
        Data::Boolean(_) => Data::Boolean(false),
        Data::Blob(blob) => {
            // Placeholder blob
            Data::Blob(
                Blob {
                    r#ref: CidLink::str(
                        "bafkreiaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ),
                    mime_type: blob.mime_type.clone(),
                    size: 0,
                }
                .into_static(),
            )
        }
        Data::CidLink(_) => Data::CidLink(Cid::str(
            "bafkreiaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )),
        Data::Bytes(_) => Data::Bytes(Bytes::new()),
        Data::Null => Data::Null,
    }
}

/// Get expected string format from schema by navigating path
pub fn get_expected_string_format(
    root_data: &Data<'_>,
    current_path: &str,
) -> Option<jacquard_lexicon::lexicon::LexStringFormat> {
    use jacquard_lexicon::lexicon::*;

    // Get root type discriminator
    let root_nsid = root_data.type_discriminator()?;

    // Look up schema in global registry
    let validator = jacquard_lexicon::validation::SchemaValidator::global();
    let registry = validator.registry();
    let schema = registry.get(root_nsid)?;

    // Navigate to the property at this path
    let segments: Vec<&str> = if current_path.is_empty() {
        vec![]
    } else {
        current_path.split('.').collect()
    };

    // Start with the record's main object definition
    let main_obj = match schema.defs.get("main")? {
        LexUserType::Record(rec) => match &rec.record {
            LexRecordRecord::Object(obj) => obj,
        },
        _ => return None,
    };

    // Track current position in schema
    enum SchemaType<'a> {
        ObjectProp(&'a LexObjectProperty<'a>),
        ArrayItem(&'a LexArrayItem<'a>),
    }

    let mut current_type: Option<SchemaType> = None;
    let mut current_obj = Some(main_obj);

    for segment in segments {
        // Handle array indices - strip them to get field name
        let field_name = segment.trim_end_matches(|c: char| c.is_numeric() || c == '[' || c == ']');

        if field_name.is_empty() {
            continue; // Pure array index like [0], skip
        }

        if let Some(obj) = current_obj.take() {
            if let Some(prop) = obj.properties.get(field_name) {
                current_type = Some(SchemaType::ObjectProp(prop));
            }
        }

        // Process current type
        match current_type {
            Some(SchemaType::ObjectProp(LexObjectProperty::Array(arr))) => {
                // Array - unwrap to item type
                current_type = Some(SchemaType::ArrayItem(&arr.items));
            }
            Some(SchemaType::ObjectProp(LexObjectProperty::Object(obj))) => {
                // Nested object - descend into it
                current_obj = Some(obj);
                current_type = None;
            }
            Some(SchemaType::ArrayItem(LexArrayItem::Object(obj))) => {
                // Array of objects - descend into object
                current_obj = Some(obj);
                current_type = None;
            }
            _ => {}
        }
    }

    // Check if final type is a string with format
    match current_type? {
        SchemaType::ObjectProp(LexObjectProperty::String(lex_string)) => lex_string.format,
        SchemaType::ArrayItem(LexArrayItem::String(lex_string)) => lex_string.format,
        _ => None,
    }
}

pub fn get_hex_rep(byte_array: &mut [u8]) -> String {
    let build_string_vec: Vec<String> = byte_array
        .chunks(2)
        .enumerate()
        .map(|(i, c)| {
            let sep = if i % 16 == 0 && i > 0 {
                "\n"
            } else if i == 0 {
                ""
            } else {
                " "
            };
            if c.len() == 2 {
                format!("{}{:02x}{:02x}", sep, c[0], c[1])
            } else {
                format!("{}{:02x}", sep, c[0])
            }
        })
        .collect();
    build_string_vec.join("")
}
