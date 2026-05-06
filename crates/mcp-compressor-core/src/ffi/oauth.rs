use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::oauth::{
    clear_oauth_store, list_oauth_stores, oauth_store_root, remember_oauth_store,
    OAuthStoreIndexEntry,
};
use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FfiOAuthStoreEntry {
    pub backend_name: String,
    pub backend_uri: String,
    pub store_dir: PathBuf,
}

pub fn oauth_store_path() -> PathBuf {
    oauth_store_root()
}

pub fn remember_oauth_backend(
    backend_uri: &str,
    backend_name: &str,
    store_dir: PathBuf,
) -> Result<(), Error> {
    remember_oauth_store(backend_uri, backend_name, &store_dir).map_err(Error::Io)
}

pub fn list_oauth_credentials() -> Result<Vec<FfiOAuthStoreEntry>, Error> {
    list_oauth_stores()
        .map(|entries| entries.into_iter().map(Into::into).collect())
        .map_err(Error::Io)
}

pub fn clear_oauth_credentials(target: Option<&str>) -> Result<Vec<PathBuf>, Error> {
    clear_oauth_store(target).map_err(Error::Io)
}

impl From<OAuthStoreIndexEntry> for FfiOAuthStoreEntry {
    fn from(value: OAuthStoreIndexEntry) -> Self {
        Self {
            backend_name: value.name,
            backend_uri: value.uri,
            store_dir: value.store_dir.into(),
        }
    }
}
