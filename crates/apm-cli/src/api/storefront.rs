use anyhow::Result;
use reqwest::{Method, Response, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct StorefrontHttpClient {
    client: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FeaturedResponse {
    pub sections: Vec<StorefrontSection>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExploreResponse {
    pub categories: Vec<StorefrontSection>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CompareResponse {
    pub left: StorefrontPlugin,
    pub right: StorefrontPlugin,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StorefrontSection {
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub plugins: Vec<StorefrontPlugin>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StorefrontPlugin {
    pub slug: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub category: String,
    pub description: String,
    pub tags: Vec<String>,
    pub formats: Vec<String>,
    pub price_cents: i64,
    pub currency: String,
    pub is_paid: bool,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    message: String,
}

impl StorefrontHttpClient {
    pub fn from_env() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: std::env::var("APM_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
        }
    }

    pub async fn featured(&self) -> Result<FeaturedResponse> {
        let response = self.send(Method::GET, "/commerce/featured", None).await?;
        decode_json(response, "featured request failed").await
    }

    pub async fn explore(&self) -> Result<ExploreResponse> {
        let response = self.send(Method::GET, "/commerce/explore", None).await?;
        decode_json(response, "explore request failed").await
    }

    pub async fn compare(&self, left_slug: &str, right_slug: &str) -> Result<CompareResponse> {
        let response = self
            .send(
                Method::POST,
                "/commerce/compare",
                Some(serde_json::json!({
                    "left_slug": left_slug,
                    "right_slug": right_slug,
                })),
            )
            .await?;
        decode_json(response, "compare request failed").await
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Response> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path));

        if let Some(body) = body {
            request = request.json(&body);
        }

        Ok(request.send().await?)
    }
}

async fn decode_json<T>(response: Response, context: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let status = response.status();
    if status.is_success() {
        return Ok(response.json().await?);
    }

    let body = response.text().await.unwrap_or_default();
    let detail = parse_api_error(status, &body).unwrap_or_else(|| body.trim().to_string());
    anyhow::bail!("{context}: {detail}");
}

fn parse_api_error(status: StatusCode, body: &str) -> Option<String> {
    let parsed: ApiErrorEnvelope = serde_json::from_str(body).ok()?;
    let status_label = match status {
        StatusCode::NOT_FOUND => "not found",
        StatusCode::BAD_REQUEST => "invalid request",
        StatusCode::UNAUTHORIZED => "authentication required",
        _ => "request failed",
    };
    Some(format!("{status_label}: {}", parsed.error.message))
}
