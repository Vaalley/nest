//! Tauri-managed application state for the Bird client.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::NestClient;
use crate::config::{AppConfig, ConfigStore};
use crate::error::{BirdError, BirdResult};
use crate::forage::ForagingEngine;
use crate::sync::FlightHome;

/// Shared application state injected into Tauri commands.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    config_store: ConfigStore,
    forager: ForagingEngine,
    client: Arc<RwLock<Option<NestClient>>>,
    flight: FlightHome,
}

impl AppState {
    /// Load the config store and forager, optionally restoring an authenticated
    /// client from saved credentials.
    pub async fn load() -> BirdResult<Self> {
        let config_store = ConfigStore::new()?;
        let config = config_store.load()?;
        let forager = ForagingEngine::new(ConfigStore::dir()?.join("cache"));

        let client = if let Some(ref username) = config.flock_username {
            if let Some(token) = crate::storage::get_token(username)? {
                Some(NestClient::new(&config.nest_url, Some(token)))
            } else {
                None
            }
        } else {
            None
        };

        let client = Arc::new(RwLock::new(client));
        let flight = FlightHome::new(config_store.clone(), forager.clone(), client.clone())?;

        Ok(Self {
            inner: Arc::new(Inner {
                config_store,
                forager,
                client,
                flight,
            }),
        })
    }

    pub fn config(&self) -> BirdResult<AppConfig> {
        self.inner.config_store.load()
    }

    pub async fn set_config(&self, config: AppConfig) -> BirdResult<()> {
        self.inner.config_store.save(&config)?;
        Ok(())
    }

    pub fn forager(&self) -> &ForagingEngine {
        &self.inner.forager
    }

    pub fn flight(&self) -> &FlightHome {
        &self.inner.flight
    }

    /// Return the currently authenticated Nest client, if any.
    pub async fn client(&self) -> Option<NestClient> {
        self.inner.client.read().await.clone()
    }

    /// Set (or clear) the authenticated Nest client.
    pub async fn set_client(&self, client: Option<NestClient>) {
        *self.inner.client.write().await = client;
    }

    /// Return the current Nest client or fail with [`BirdError::NotAuthenticated`].
    pub async fn require_client(&self) -> BirdResult<NestClient> {
        self.client().await.ok_or(BirdError::NotAuthenticated)
    }

    /// Persist a username/token pair and update the in-memory client.
    pub async fn authenticate(
        &self,
        username: String,
        token: String,
        bird_id: Option<uuid::Uuid>,
    ) -> BirdResult<()> {
        let mut config = self.config()?;
        config.flock_username = Some(username.clone());
        if let Some(id) = bird_id {
            config.bird_id = Some(id);
        }
        self.set_config(config).await?;
        crate::storage::set_token(&username, &token)?;
        self.set_client(Some(NestClient::new(&self.config()?.nest_url, Some(token))))
            .await;
        Ok(())
    }

    /// Clear all authentication state.
    pub async fn logout(&self) -> BirdResult<()> {
        let config = self.config()?;
        if let Some(username) = config.flock_username {
            crate::storage::delete_token(&username)?;
        }
        let new_config = AppConfig {
            nest_url: config.nest_url,
            bird_name: config.bird_name,
            platform: config.platform,
            flock_username: None,
            bird_id: None,
        };
        self.set_config(new_config).await?;
        self.set_client(None).await;
        Ok(())
    }

    /// Start the background agent and sync engine. Called once from Tauri's
    /// setup hook.
    pub async fn start_background(&self, app_handle: tauri::AppHandle) -> BirdResult<()> {
        self.flight().start(app_handle).await
    }
}
