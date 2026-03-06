use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonSchemaType {
    Single(String),
    Union(Vec<String>),
}

impl JsonSchemaType {
    fn contains(&self, expected: &str) -> bool {
        match self {
            Self::Single(kind) => kind == expected,
            Self::Union(kinds) => kinds.iter().any(|kind| kind == expected),
        }
    }
}

fn schema_declares_array(schema: &serde_json::Map<String, Value>) -> bool {
    schema
        .get("type")
        .cloned()
        .and_then(|value| serde_json::from_value::<JsonSchemaType>(value).ok())
        .is_some_and(|kind| kind.contains("array"))
}

pub(super) fn normalize_responses_tool_schema(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for child in map.values_mut() {
                normalize_responses_tool_schema(child);
            }
            if schema_declares_array(map) && !map.contains_key("items") {
                map.insert("items".to_string(), serde_json::json!({}));
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_responses_tool_schema(item);
            }
        }
        _ => {}
    }
}
