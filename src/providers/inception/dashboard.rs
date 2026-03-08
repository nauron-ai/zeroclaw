use serde::Serialize;

use super::DASHBOARD_MODEL_OPTIONS;

const EMPTY_DASHBOARD_OPTIONS: &[&str] = &[];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DashboardFieldInputType {
    Secret,
    Select,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DashboardField {
    key: &'static str,
    label: &'static str,
    required: bool,
    has_value: bool,
    input_type: DashboardFieldInputType,
    options: &'static [&'static str],
    #[serde(skip_serializing_if = "Option::is_none")]
    masked_value: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_value: Option<String>,
}

impl DashboardField {
    fn secret(masked_secret: &'static str, has_value: bool) -> Self {
        Self {
            key: "api_key",
            label: "API Key",
            required: true,
            has_value,
            input_type: DashboardFieldInputType::Secret,
            options: EMPTY_DASHBOARD_OPTIONS,
            masked_value: has_value.then_some(masked_secret),
            current_value: None,
        }
    }

    fn default_model(current_value: Option<&str>) -> Self {
        Self {
            key: "default_model",
            label: "Default Model",
            required: false,
            has_value: current_value.is_some(),
            input_type: DashboardFieldInputType::Select,
            options: DASHBOARD_MODEL_OPTIONS,
            masked_value: None,
            current_value: Some(current_value.unwrap_or_default().to_string()),
        }
    }
}

pub(crate) fn dashboard_fields(
    has_key: bool,
    current_default_model: Option<&str>,
    masked_secret: &'static str,
) -> Vec<DashboardField> {
    vec![
        DashboardField::secret(masked_secret, has_key),
        DashboardField::default_model(current_default_model),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::inception::DEFAULT_MODEL_ID;

    #[test]
    fn dashboard_fields_use_typed_contract() {
        let fields = dashboard_fields(true, Some(DEFAULT_MODEL_ID), "***MASKED***");

        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].input_type, DashboardFieldInputType::Secret);
        assert_eq!(fields[1].input_type, DashboardFieldInputType::Select);
        assert_eq!(fields[1].options, DASHBOARD_MODEL_OPTIONS);
    }
}
