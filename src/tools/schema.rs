use serde_json::{json, Value};

/// Generate a JSON schema from a Rust type name and description.
///
/// This is a basic implementation. For more advanced schema generation,
/// consider using libraries like `schemars` or `jsonschema`.
pub fn generate_schema_from_type(type_name: &str, description: &str) -> Value {
    match type_name {
        "String" | "str" => json!({
            "type": "string",
            "description": description
        }),
        "i32" | "i64" | "u32" | "u64" | "isize" | "usize" => json!({
            "type": "integer",
            "description": description
        }),
        "f32" | "f64" => json!({
            "type": "number",
            "description": description
        }),
        "bool" => json!({
            "type": "boolean",
            "description": description
        }),
        _ => json!({
            "type": "string",
            "description": format!("{} ({})", description, type_name)
        }),
    }
}

/// Generate a JSON schema for a struct field.
pub fn generate_field_schema(
    field_name: &str,
    field_type: &str,
    description: Option<&str>,
    required: bool,
) -> Value {
    let desc = description.unwrap_or(field_name);
    let mut schema = generate_schema_from_type(field_type, desc);

    if let Some(obj) = schema.as_object_mut() {
        obj.insert("description".to_string(), json!(desc));
    }

    if !required {
        // For optional fields, we wrap in a union with null
        schema = json!({
            "oneOf": [
                schema,
                {"type": "null"}
            ]
        });
    }

    schema
}

/// Generate a complete JSON schema for tool parameters from field definitions.
pub fn generate_parameters_schema(
    properties: Vec<(&str, &str, Option<&str>, bool)>, // (name, type, description, required)
) -> Value {
    let mut schema_properties = serde_json::Map::new();
    let mut required = Vec::new();

    for (name, field_type, description, is_required) in properties {
        let field_schema = generate_field_schema(name, field_type, description, is_required);
        schema_properties.insert(name.to_string(), field_schema);

        if is_required {
            required.push(name.to_string());
        }
    }

    json!({
        "type": "object",
        "properties": schema_properties,
        "required": required
    })
}

/// Helper to extract type name from a type string (handles Option, Vec, etc.)
pub fn extract_base_type(type_str: &str) -> (&str, bool) {
    let type_str = type_str.trim();

    // Handle Option<T>
    if let Some(inner) = type_str
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return (inner.trim(), false);
    }

    // Handle Vec<T>
    if let Some(inner) = type_str
        .strip_prefix("Vec<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return (inner.trim(), true); // true indicates array
    }

    (type_str, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_schema_from_type() {
        let schema = generate_schema_from_type("String", "A string value");
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["description"], "A string value");
    }

    #[test]
    fn test_generate_parameters_schema() {
        let schema = generate_parameters_schema(vec![
            ("query", "String", Some("Search query"), true),
            ("limit", "u32", Some("Result limit"), false),
        ]);

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["required"].is_array());
    }

    #[test]
    fn test_extract_base_type() {
        assert_eq!(extract_base_type("String"), ("String", false));
        assert_eq!(extract_base_type("Option<String>"), ("String", false));
        assert_eq!(extract_base_type("Vec<String>"), ("String", true));
    }
}
