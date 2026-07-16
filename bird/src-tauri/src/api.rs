//! Typed HTTP client for the Nest REST API.
//!
//! Uses `reqwest` and the shared DTOs from `nest-shared` so the Bird and Nest
//! stay in sync.

use nest_shared::api::{
    AuthResponse, ClutchSummary, CompareRequest, CompareResponse, LoginRequest,
    RegisterBirdRequest, RegisterBirdResponse, RegisterFlockRequest, Resolution, ResolveRequest,
    ResolveResponse,
};
use nest_shared::domain::{Bird, Egg};
use reqwest::header::AUTHORIZATION;
use serde::Serialize;

use crate::error::{BirdError, BirdResult};

/// HTTP client for talking to the Nest.
#[derive(Debug, Clone)]
pub struct NestClient {
    base_url: String,
    client: reqwest::Client,
    token: Option<String>,
}

impl NestClient {
    /// Create a new client pointing at `base_url`.
    pub fn new(base_url: &str, token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            token,
        }
    }

    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    fn auth_request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> BirdResult<reqwest::RequestBuilder> {
        let token = self.token.as_ref().ok_or(BirdError::NotAuthenticated)?;
        Ok(self
            .client
            .request(method, format!("{}{path}", self.base_url))
            .header(AUTHORIZATION, format!("Bearer {token}")))
    }

    /// `POST /api/flock/register`
    pub async fn register_flock(&self, username: &str, password: &str) -> BirdResult<AuthResponse> {
        self.post(
            "/api/flock/register",
            &RegisterFlockRequest {
                username: username.to_string(),
                password: password.to_string(),
            },
        )
        .await
    }

    /// `POST /api/flock/login`
    pub async fn login(&self, username: &str, password: &str) -> BirdResult<AuthResponse> {
        self.post(
            "/api/flock/login",
            &LoginRequest {
                username: username.to_string(),
                password: password.to_string(),
            },
        )
        .await
    }

    /// `POST /api/birds`
    pub async fn register_bird(
        &self,
        name: &str,
        platform: &str,
    ) -> BirdResult<RegisterBirdResponse> {
        self.post(
            "/api/birds",
            &RegisterBirdRequest {
                name: name.to_string(),
                platform: platform.to_string(),
            },
        )
        .await
    }

    /// `GET /api/birds`
    pub async fn list_birds(&self) -> BirdResult<Vec<Bird>> {
        self.get("/api/birds").await
    }

    /// `GET /api/clutches`
    pub async fn list_clutches(&self) -> BirdResult<Vec<ClutchSummary>> {
        self.get("/api/clutches").await
    }

    /// `GET /api/clutches/{game_id}/eggs`
    pub async fn list_eggs(&self, game_id: &str) -> BirdResult<Vec<Egg>> {
        self.get(&format!("/api/clutches/{game_id}/eggs")).await
    }

    /// `POST /api/clutches/{game_id}/compare`
    pub async fn compare(
        &self,
        game_id: &str,
        local_hash: &str,
        local_modified_at: i64,
    ) -> BirdResult<CompareResponse> {
        self.post(
            &format!("/api/clutches/{game_id}/compare"),
            &CompareRequest {
                local_hash: local_hash.to_string(),
                local_modified_at,
                bird_id: None,
            },
        )
        .await
    }

    /// `POST /api/clutches/{game_id}/resolve`
    pub async fn resolve(
        &self,
        game_id: &str,
        resolution: Resolution,
        local_hash: Option<&str>,
        local_modified_at: Option<i64>,
    ) -> BirdResult<ResolveResponse> {
        self.post(
            &format!("/api/clutches/{game_id}/resolve"),
            &ResolveRequest {
                resolution,
                local_hash: local_hash.map(String::from),
                local_modified_at,
                egg_id: None,
                bird_id: None,
            },
        )
        .await
    }

    /// `GET /api/clutches/{game_id}/hatch/{egg_id}`
    pub async fn hatch(&self, game_id: &str, egg_id: uuid::Uuid) -> BirdResult<Vec<u8>> {
        let req = self.auth_request(
            reqwest::Method::GET,
            &format!("/api/clutches/{game_id}/hatch/{egg_id}"),
        )?;
        let resp = req.send().await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// `POST /api/clutches/{game_id}/lay`
    pub async fn lay(
        &self,
        game_id: &str,
        source_bird_id: uuid::Uuid,
        zip_bytes: Vec<u8>,
        file_hash: &str,
    ) -> BirdResult<Egg> {
        let token = self.token.as_ref().ok_or(BirdError::NotAuthenticated)?;
        let file_part = reqwest::multipart::Part::bytes(zip_bytes)
            .file_name("egg.zip")
            .mime_str("application/zip")?;
        let form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("file_hash", file_hash.to_string())
            .text("source_bird_id", source_bird_id.to_string());

        let url = format!("{}/api/clutches/{}/lay", self.base_url, game_id);
        let resp = self
            .client
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .multipart(form)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json().await?)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> BirdResult<T> {
        let req = self.auth_request(reqwest::Method::GET, path)?;
        let resp = req.send().await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json().await?)
    }

    async fn post<T: serde::de::DeserializeOwned, B: Serialize + Sync + Send>(
        &self,
        path: &str,
        body: &B,
    ) -> BirdResult<T> {
        let req = self.auth_request(reqwest::Method::POST, path)?.json(body);
        let resp = req.send().await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json().await?)
    }

    async fn check_status(resp: reqwest::Response) -> BirdResult<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }

        let text = resp.text().await.unwrap_or_default();
        let message = parse_nest_error(&text)
            .or_else(|| parse_nest_error_legacy(&text))
            .unwrap_or(text);
        Err(BirdError::Nest {
            status: status.as_u16(),
            message,
        })
    }
}

fn parse_nest_error(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .map(std::string::String::from)
}

fn parse_nest_error_legacy(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value
        .get("message")
        .and_then(|m| m.as_str())
        .map(String::from)
}
