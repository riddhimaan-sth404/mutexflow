use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct VerifyLicenseRequest {
    #[serde(rename = "licenseKey")]
    pub license_key: String,
    #[serde(rename = "machineId")]
    pub machine_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyLicenseResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessStandardRequest {
    pub prompt: String,
    #[serde(rename = "licenseKey")]
    pub license_key: String,
    #[serde(rename = "machineId")]
    pub machine_id: Option<String>,
    #[serde(rename = "taskComplexity")]
    pub task_complexity: Option<String>,
    #[serde(rename = "workflowType")]
    pub workflow_type: Option<String>,
    #[serde(rename = "targetIntegrations")]
    pub target_integrations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessStandardResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct DispatchIntegrationsRequest {
    pub result: String,
    pub prompt: Option<String>,
    #[serde(rename = "licenseKey")]
    pub license_key: Option<String>,
    #[serde(rename = "machineId")]
    pub machine_id: Option<String>,
    #[serde(rename = "workflowType")]
    pub workflow_type: Option<String>,
    #[serde(rename = "taskComplexity")]
    pub task_complexity: Option<String>,
    #[serde(rename = "targetIntegrations")]
    pub target_integrations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DispatchIntegrationsResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationStatusItem {
    pub provider: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationsStatusResponse {
    pub ok: bool,
    pub integrations: Vec<IntegrationStatusItem>,
}
