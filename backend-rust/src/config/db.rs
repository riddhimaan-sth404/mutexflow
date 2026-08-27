use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Clone)]
pub struct SupabaseClient {
    client: Client,
    pub base_url: String,
    pub service_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LicenseRecord {
    pub id: Option<serde_json::Value>,
    pub key: Option<String>,
    pub status: Option<String>,
    pub machine_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserIntegrationRecord {
    pub machine_id: String,
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub updated_at: Option<String>,
}

impl SupabaseClient {
    pub fn from_env() -> Option<Self> {
        let base_url = env::var("SUPABASE_URL").ok()?;
        let service_key = env::var("SUPABASE_SERVICE_KEY").ok()?;

        if base_url.is_empty() || service_key.is_empty() {
            return None;
        }

        Some(Self {
            client: Client::new(),
            base_url,
            service_key,
        })
    }

    pub async fn get_license(&self, key: &str) -> Result<Option<LicenseRecord>, reqwest::Error> {
        let url = format!("{}/rest/v1/licenses?key=eq.{}&status=eq.active&select=id,key,status,machine_id&limit=1", self.base_url, urlencoding::encode(key));
        let response = self
            .client
            .get(&url)
            .header("apikey", &self.service_key)
            .header("Authorization", format!("Bearer {}", self.service_key))
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let records: Vec<LicenseRecord> = response.json().await.unwrap_or_default();
        Ok(records.into_iter().next())
    }

    pub async fn bind_license_machine(&self, id: &serde_json::Value, machine_id: &str) -> Result<bool, reqwest::Error> {
        let url = match id {
            serde_json::Value::Number(num) => format!("{}/rest/v1/licenses?id=eq.{}", self.base_url, num),
            serde_json::Value::String(s) => format!("{}/rest/v1/licenses?id=eq.{}", self.base_url, s),
            _ => format!("{}/rest/v1/licenses?id=eq.{}", self.base_url, id),
        };

        let response = self
            .client
            .patch(&url)
            .header("apikey", &self.service_key)
            .header("Authorization", format!("Bearer {}", self.service_key))
            .header("Prefer", "return=minimal")
            .json(&serde_json::json!({ "machine_id": machine_id }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    pub async fn upsert_integration(
        &self,
        machine_id: &str,
        provider: &str,
        access_token: &str,
        refresh_token: Option<&str>,
    ) -> Result<bool, reqwest::Error> {
        let url = format!("{}/rest/v1/user_integrations", self.base_url);
        let body = serde_json::json!({
            "machine_id": machine_id,
            "provider": provider,
            "access_token": access_token,
            "refresh_token": refresh_token,
            "updated_at": chrono::Utc::now().to_rfc3339()
        });

        let response = self
            .client
            .post(&url)
            .header("apikey", &self.service_key)
            .header("Authorization", format!("Bearer {}", self.service_key))
            .header("Prefer", "resolution=merge-duplicates")
            .json(&body)
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    pub async fn get_integration(
        &self,
        machine_id: &str,
        provider: &str,
    ) -> Result<Option<UserIntegrationRecord>, reqwest::Error> {
        let url = format!(
            "{}/rest/v1/user_integrations?machine_id=eq.{}&provider=eq.{}&select=machine_id,provider,access_token,refresh_token,updated_at&limit=1",
            self.base_url,
            urlencoding::encode(machine_id),
            urlencoding::encode(provider)
        );

        let response = self
            .client
            .get(&url)
            .header("apikey", &self.service_key)
            .header("Authorization", format!("Bearer {}", self.service_key))
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let records: Vec<UserIntegrationRecord> = response.json().await.unwrap_or_default();
        Ok(records.into_iter().next())
    }

    pub async fn get_all_integrations(
        &self,
        machine_id: &str,
    ) -> Result<Vec<UserIntegrationRecord>, reqwest::Error> {
        let url = format!(
            "{}/rest/v1/user_integrations?machine_id=eq.{}&select=machine_id,provider,access_token,refresh_token,updated_at",
            self.base_url,
            urlencoding::encode(machine_id)
        );

        let response = self
            .client
            .get(&url)
            .header("apikey", &self.service_key)
            .header("Authorization", format!("Bearer {}", self.service_key))
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let records: Vec<UserIntegrationRecord> = response.json().await.unwrap_or_default();
        Ok(records)
    }
}
