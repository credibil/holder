#![allow(missing_docs)]
mod issuer_client;
mod store;
mod verifier_client;

use std::collections::HashMap;
use std::str;
use std::sync::Arc;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use credibil_holder::provider::{
    Algorithm, DidResolver, Document, HolderProvider, Result, Signer, StateStore,
};
use credibil_holder::test_utils::store::keystore::HolderKeystore;
use futures::lock::Mutex;
use http::header::ACCEPT;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tauri_plugin_http::reqwest;

#[derive(Clone, Debug)]
pub struct Provider {
    app_handle: tauri::AppHandle,
    state_store: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl Provider {
    #[must_use]
    pub fn new(
        app_handle: &tauri::AppHandle, state_store: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    ) -> Self {
        Self {
            app_handle: app_handle.clone(),
            state_store,
        }
    }
}

impl HolderProvider for Provider {}

impl StateStore for Provider {
    async fn put(&self, key: &str, state: impl Serialize + Send, _: DateTime<Utc>) -> Result<()> {
        let state = serde_json::to_vec(&state)?;
        self.state_store.lock().await.insert(key.to_string(), state);
        Ok(())
    }

    async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<T> {
        let Some(state) = self.state_store.lock().await.get(key).cloned() else {
            return Err(anyhow!("state not found for key: {key}"));
        };
        Ok(serde_json::from_slice(&state)?)
    }

    async fn purge(&self, key: &str) -> Result<()> {
        self.state_store.lock().await.remove(key);
        Ok(())
    }
}

impl DidResolver for Provider {
    async fn resolve(&self, url: &str) -> anyhow::Result<Document> {
        let client = reqwest::Client::new();
        let result = client.get(url).header(ACCEPT, "application/json").send().await?;
        let doc = match result.json::<Document>().await {
            Ok(doc) => doc,
            Err(e) => {
                log::error!("Error resolving DID document: {}", e);
                return Err(e.into());
            }
        };
        Ok(doc)
    }
}

impl Signer for Provider {
    async fn try_sign(&self, msg: &[u8]) -> Result<Vec<u8>> {
        HolderKeystore::try_sign(msg)
    }

    async fn verifying_key(&self) -> Result<Vec<u8>> {
        HolderKeystore::public_key()
    }

    fn algorithm(&self) -> Algorithm {
        HolderKeystore::algorithm()
    }

    async fn verification_method(&self) -> Result<String> {
        Ok(HolderKeystore::verification_method())
    }
}
