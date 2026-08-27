use serde_json::{Map, Value};

pub(super) fn validate_input(schema: &Value, input: &Value) -> Result<(), String> {
    validate_node(schema, input, "input")
}

fn validate_node(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let Some(schema) = schema.as_object() else {
        return match schema.as_bool() {
            Some(false) => Err(format!("{path} is not allowed")),
            _ => Ok(()),
        };
    };

    if value.is_null() && schema.get("nullable").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }

    if let Some(constant) = schema.get("const")
        && value != constant
    {
        return Err(format!("{path} must equal {}", display_json(constant)));
    }

    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.iter().any(|candidate| candidate == value)
    {
        return Err(format!(
            "{path} must be one of {}",
            display_json(&Value::Array(allowed.clone()))
        ));
    }

    if let Some(expected) = schema.get("type")
        && !matches_type(expected, value)
    {
        return Err(format!(
            "{path} must be {}, got {}",
            display_type(expected),
            value_type(value)
        ));
    }

    match value {
        Value::String(value) => validate_string(schema, value, path)?,
        Value::Number(_) => validate_number(schema, value, path)?,
        Value::Array(values) => validate_array(schema, values, path)?,
        Value::Object(values) => validate_object(schema, values, path)?,
        Value::Null | Value::Bool(_) => {}
    }

    validate_alternatives(schema, value, path)
}

fn validate_string(schema: &Map<String, Value>, value: &str, path: &str) -> Result<(), String> {
    let length = value.chars().count() as u64;
    if let Some(minimum) = unsigned_keyword(schema, "minLength")
        && length < minimum
    {
        return Err(format!(
            "{path} must contain at least {minimum} character(s), got {length}"
        ));
    }
    if let Some(maximum) = unsigned_keyword(schema, "maxLength")
        && length > maximum
    {
        return Err(format!(
            "{path} must contain at most {maximum} character(s), got {length}"
        ));
    }
    Ok(())
}

fn validate_number(schema: &Map<String, Value>, value: &Value, path: &str) -> Result<(), String> {
    let Some(number) = value.as_f64() else {
        return Ok(());
    };
    if let Some(minimum) = number_keyword(schema, "minimum")
        && number < minimum
    {
        return Err(format!("{path} must be at least {minimum}, got {number}"));
    }
    if let Some(maximum) = number_keyword(schema, "maximum")
        && number > maximum
    {
        return Err(format!("{path} must be at most {maximum}, got {number}"));
    }
    if let Some(minimum) = number_keyword(schema, "exclusiveMinimum")
        && number <= minimum
    {
        return Err(format!(
            "{path} must be greater than {minimum}, got {number}"
        ));
    }
    if let Some(maximum) = number_keyword(schema, "exclusiveMaximum")
        && number >= maximum
    {
        return Err(format!("{path} must be less than {maximum}, got {number}"));
    }
    Ok(())
}

fn validate_array(schema: &Map<String, Value>, values: &[Value], path: &str) -> Result<(), String> {
    let length = values.len() as u64;
    if let Some(minimum) = unsigned_keyword(schema, "minItems")
        && length < minimum
    {
        return Err(format!(
            "{path} must contain at least {minimum} item(s), got {length}"
        ));
    }
    if let Some(maximum) = unsigned_keyword(schema, "maxItems")
        && length > maximum
    {
        return Err(format!(
            "{path} must contain at most {maximum} item(s), got {length}"
        ));
    }
    if schema.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
        for (index, value) in values.iter().enumerate() {
            if values[..index].contains(value) {
                return Err(format!("{path} must not contain duplicate items"));
            }
        }
    }
    if let Some(item_schema) = schema.get("items") {
        for (index, value) in values.iter().enumerate() {
            validate_node(item_schema, value, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}

fn validate_object(
    schema: &Map<String, Value>,
    values: &Map<String, Value>,
    path: &str,
) -> Result<(), String> {
    let length = values.len() as u64;
    if let Some(minimum) = unsigned_keyword(schema, "minProperties")
        && length < minimum
    {
        return Err(format!(
            "{path} must contain at least {minimum} field(s), got {length}"
        ));
    }
    if let Some(maximum) = unsigned_keyword(schema, "maxProperties")
        && length > maximum
    {
        return Err(format!(
            "{path} must contain at most {maximum} field(s), got {length}"
        ));
    }

    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for field in required.iter().filter_map(Value::as_str) {
            if !values.contains_key(field) {
                return Err(format!("{path}.{field} is required"));
            }
        }
    }

    let properties = schema.get("properties").and_then(Value::as_object);
    if let Some(properties) = properties {
        for (field, field_schema) in properties {
            if let Some(value) = values.get(field) {
                validate_node(field_schema, value, &format!("{path}.{field}"))?;
            }
        }
    }

    match schema.get("additionalProperties") {
        Some(Value::Bool(false)) if properties.is_some() => {
            if let Some(field) = values
                .keys()
                .find(|field| properties.is_none_or(|properties| !properties.contains_key(*field)))
            {
                return Err(format!("{path}.{field} is not an allowed field"));
            }
        }
        Some(additional_schema @ Value::Object(_)) => {
            for (field, value) in values {
                if properties.is_none_or(|properties| !properties.contains_key(field)) {
                    validate_node(additional_schema, value, &format!("{path}.{field}"))?;
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn validate_alternatives(
    schema: &Map<String, Value>,
    value: &Value,
    path: &str,
) -> Result<(), String> {
    if let Some(branches) = credible_branches(schema, "anyOf", value) {
        let errors = branches
            .iter()
            .filter_map(|branch| validate_node(branch, value, path).err())
            .collect::<Vec<_>>();
        if errors.len() == branches.len() {
            return Err(format!(
                "{path} must match at least one allowed schema: {}",
                errors
                    .first()
                    .map(String::as_str)
                    .unwrap_or("no branch matched")
            ));
        }
    }

    if let Some(branches) = credible_branches(schema, "oneOf", value) {
        let results = branches
            .iter()
            .map(|branch| validate_node(branch, value, path))
            .collect::<Vec<_>>();
        let matches = results.iter().filter(|result| result.is_ok()).count();
        if matches == 0 {
            let detail = results
                .into_iter()
                .find_map(Result::err)
                .unwrap_or_else(|| "no branch matched".to_string());
            return Err(format!("{path} must match one allowed schema: {detail}"));
        }
        if matches > 1 {
            return Err(format!("{path} matches more than one exclusive schema"));
        }
    }

    // KIE's current route documents contain generated `allOf` fragments whose
    // declared type contradicts the surrounding object. They remain visible in
    // kie_models, but enforcing them would reject every valid request.
    Ok(())
}

fn credible_branches<'a>(
    parent: &'a Map<String, Value>,
    keyword: &str,
    value: &Value,
) -> Option<&'a [Value]> {
    let branches = parent.get(keyword)?.as_array()?;
    if branches.is_empty() {
        return None;
    }
    let parent_kind = inferred_type(parent);
    let has_compatible_branch = branches.iter().any(|branch| {
        let Some(branch) = branch.as_object() else {
            return true;
        };
        let branch_kind = inferred_type(branch);
        parent_kind
            .zip(branch_kind)
            .is_none_or(|(parent, branch)| parent == branch)
            && branch
                .get("type")
                .is_none_or(|expected| matches_type(expected, value))
    });
    has_compatible_branch.then_some(branches)
}

fn inferred_type(schema: &Map<String, Value>) -> Option<&str> {
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        return Some(kind);
    }
    if schema.contains_key("properties") || schema.contains_key("required") {
        return Some("object");
    }
    if schema.contains_key("items") {
        return Some("array");
    }
    None
}

fn matches_type(expected: &Value, value: &Value) -> bool {
    match expected {
        Value::String(expected) => matches_single_type(expected, value),
        Value::Array(expected) => expected
            .iter()
            .filter_map(Value::as_str)
            .any(|expected| matches_single_type(expected, value)),
        _ => true,
    }
}

fn matches_single_type(expected: &str, value: &Value) -> bool {
    match expected {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "number" => value.is_number(),
        "integer" => value
            .as_f64()
            .is_some_and(|number| number.is_finite() && number.fract() == 0.0),
        "string" => value.is_string(),
        _ => true,
    }
}

fn unsigned_keyword(schema: &Map<String, Value>, keyword: &str) -> Option<u64> {
    schema.get(keyword).and_then(Value::as_u64)
}

fn number_keyword(schema: &Map<String, Value>, keyword: &str) -> Option<f64> {
    schema.get(keyword).and_then(Value::as_f64)
}

fn display_type(expected: &Value) -> String {
    match expected {
        Value::String(expected) => expected.clone(),
        _ => display_json(expected),
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.as_f64().is_some_and(|value| value.fract() == 0.0) => {
            "integer"
        }
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn display_json(value: &Value) -> String {
    let rendered = value.to_string();
    const MAX_ERROR_VALUE_CHARS: usize = 300;
    if rendered.chars().count() <= MAX_ERROR_VALUE_CHARS {
        rendered
    } else {
        let mut truncated = rendered
            .chars()
            .take(MAX_ERROR_VALUE_CHARS)
            .collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_input;

    #[test]
    fn validates_recursive_constraints() {
        let schema = json!({
            "type": "object",
            "required": ["mode", "shots"],
            "properties": {
                "mode": { "type": "string", "enum": ["std", "pro"] },
                "prompt": { "type": "string", "minLength": 2, "maxLength": 8 },
                "shots": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 2,
                    "items": {
                        "type": "object",
                        "required": ["duration"],
                        "properties": {
                            "duration": { "type": "integer", "minimum": 1, "maximum": 5 }
                        }
                    }
                }
            }
        });

        assert!(
            validate_input(
                &schema,
                &json!({ "mode": "pro", "prompt": "camera", "shots": [{ "duration": 3 }] })
            )
            .is_ok()
        );
        assert_eq!(
            validate_input(&schema, &json!({ "mode": "ultra", "shots": [] })).unwrap_err(),
            "input.mode must be one of [\"std\",\"pro\"]"
        );
        assert_eq!(
            validate_input(&schema, &json!({ "mode": "std", "shots": [{}] })).unwrap_err(),
            "input.shots[0].duration is required"
        );
    }

    #[test]
    fn ignores_contradictory_generated_combinators() {
        let schema = json!({
            "type": "object",
            "properties": { "prompt": { "type": "string" } },
            "allOf": [{ "type": "string" }],
            "anyOf": [{ "type": "string" }, { "type": "string" }]
        });

        assert!(validate_input(&schema, &json!({ "prompt": "valid object" })).is_ok());
    }

    #[test]
    fn enforces_credible_object_alternatives() {
        let schema = json!({
            "type": "object",
            "anyOf": [
                { "type": "object", "required": ["image_url"] },
                { "type": "object", "required": ["task_id"] }
            ]
        });

        assert!(validate_input(&schema, &json!({ "task_id": "task_1" })).is_ok());
        assert!(validate_input(&schema, &json!({})).is_err());
    }
}
