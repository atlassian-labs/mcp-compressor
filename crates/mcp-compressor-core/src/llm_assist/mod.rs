//! Optional local LLM assistance primitives.
//!
//! This module intentionally exposes a small, reusable core that can be called
//! from different proxy paths without deciding which proxy feature owns the
//! prompt. It provides:
//!
//! - model reference parsing,
//! - lazy Hugging Face GGUF download/cache management,
//! - a prompt-completion trait, and
//! - an ephemeral official `llama-server` runtime for local GGUF inference.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use flate2::read::GzDecoder;
use fs2::FileExt;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};

use crate::Error;

pub const DEFAULT_MODEL_REF: &str = "LiquidAI/LFM2.5-350M-GGUF:Q4_K_M";
pub const INPUT_TOKEN_BUDGET: usize = 32_768;
pub const OUTPUT_TOKEN_BUDGET: usize = 4_096;
pub const DEFAULT_TEMPERATURE: f32 = 0.0;
pub const DEFAULT_TOP_P: f32 = 1.0;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

const DEFAULT_HF_REVISION: &str = "main";
const DEFAULT_CACHE_ENV: &str = "MCP_COMPRESSOR_CACHE_DIR";
const MODEL_MANIFEST_FILE: &str = "manifest.json";
const RUNTIME_MANIFEST_FILE: &str = "manifest.json";
const LLAMA_CPP_RELEASE_TAG: &str = "b9150";
const LLAMA_CPP_RELEASE_BASE: &str = "https://github.com/ggml-org/llama.cpp/releases/download";
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(60);
const SERVER_POLL_INTERVAL: Duration = Duration::from_millis(25);
const SERVER_START_RETRIES: usize = 3;

/// Reference to a GGUF model artifact.
///
/// The compact form is `<org>/<repo>:<quant>`, for example
/// `LiquidAI/LFM2.5-350M-GGUF:Q4_K_M`. A specific filename can be provided as
/// `<org>/<repo>#<filename>`. The filename form is useful for models whose GGUF
/// filenames cannot be derived from the repository name and quantization suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    raw: String,
    repo_id: String,
    quantization: Option<String>,
    filename: String,
    revision: String,
}

impl ModelRef {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, Error> {
        let raw = value.as_ref().trim();
        if raw.is_empty() {
            return Err(Error::LlmAssist(
                "model reference cannot be empty".to_string(),
            ));
        }

        let (repo_part, explicit_filename) = match raw.split_once('#') {
            Some((repo, filename)) if !repo.is_empty() && !filename.is_empty() => {
                (repo, Some(filename.to_string()))
            }
            Some(_) => {
                return Err(Error::LlmAssist(
                    "model reference filename form must be <repo>#<filename>".to_string(),
                ));
            }
            None => (raw, None),
        };

        let (repo_id, quantization) = match repo_part.rsplit_once(':') {
            Some((repo, quant)) if repo.contains('/') && !quant.is_empty() => {
                (repo.to_string(), Some(quant.to_string()))
            }
            _ => (repo_part.to_string(), None),
        };

        if repo_id.split('/').count() != 2 || repo_id.contains("..") {
            return Err(Error::LlmAssist(format!(
                "invalid Hugging Face model repo id: {repo_id}"
            )));
        }

        let filename = match explicit_filename {
            Some(filename) => filename,
            None => derive_gguf_filename(&repo_id, quantization.as_deref())?,
        };
        validate_safe_filename(&filename)?;

        Ok(Self {
            raw: raw.to_string(),
            repo_id,
            quantization,
            filename,
            revision: DEFAULT_HF_REVISION.to_string(),
        })
    }

    pub fn default_model() -> Self {
        Self::parse(DEFAULT_MODEL_REF).expect("default model reference is valid")
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    pub fn quantization(&self) -> Option<&str> {
        self.quantization.as_deref()
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo_id, self.revision, self.filename
        )
    }

    fn cache_key(&self) -> PathBuf {
        let mut path = PathBuf::new();
        for segment in self.repo_id.split('/') {
            path.push(sanitize_path_segment(segment));
        }
        path.push(sanitize_path_segment(&self.revision));
        path.push(&self.filename);
        path
    }
}

impl Default for ModelRef {
    fn default() -> Self {
        Self::default_model()
    }
}

impl fmt::Display for ModelRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.raw)
    }
}

/// Local model cache and lazy downloader.
#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
    client: reqwest::Client,
}

impl ModelStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn default_cache_dir() -> Result<PathBuf, Error> {
        if let Some(path) = std::env::var_os(DEFAULT_CACHE_ENV) {
            return Ok(PathBuf::from(path).join("models"));
        }
        let base = dirs::cache_dir().ok_or_else(|| {
            Error::LlmAssist(
                "could not determine a cache directory for model downloads".to_string(),
            )
        })?;
        Ok(base.join("mcp-compressor").join("models"))
    }

    pub fn default_store() -> Result<Self, Error> {
        Ok(Self::new(Self::default_cache_dir()?))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn model_path(&self, model: &ModelRef) -> PathBuf {
        self.root.join(model.cache_key())
    }

    pub fn is_cached(&self, model: &ModelRef) -> bool {
        self.model_path(model).is_file()
    }

    /// Return the local model path, downloading only when `allow_download` is true.
    pub async fn ensure_model(
        &self,
        model: &ModelRef,
        allow_download: bool,
    ) -> Result<PathBuf, Error> {
        let path = self.model_path(model);
        if path.is_file() {
            return Ok(path);
        }
        if !allow_download {
            return Err(Error::LlmAssist(format!(
                "model {model} is not cached at {}; enable model download or run setup first",
                path.display()
            )));
        }
        self.download_model(model).await
    }

    pub async fn download_model(&self, model: &ModelRef) -> Result<PathBuf, Error> {
        let final_path = self.model_path(model);
        if final_path.is_file() {
            return Ok(final_path);
        }

        let parent = final_path.parent().ok_or_else(|| {
            Error::LlmAssist(format!(
                "invalid model cache path: {}",
                final_path.display()
            ))
        })?;
        fs::create_dir_all(parent)?;
        fs::create_dir_all(&self.root)?;
        let lock_path = self.root.join("download.lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock_file.lock_exclusive()?;
        if final_path.is_file() {
            let _ = lock_file.unlock();
            return Ok(final_path);
        }

        let partial_path = final_path.with_extension("gguf.part");
        let mut file = File::create(&partial_path)?;
        let response = self
            .client
            .get(model.download_url())
            .send()
            .await?
            .error_for_status()?;
        let expected_size = response.content_length();
        let mut stream = response.bytes_stream();
        let mut bytes_written: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            std::io::Write::write_all(&mut file, &chunk)?;
            bytes_written += chunk.len() as u64;
        }
        std::io::Write::flush(&mut file)?;
        drop(file);

        if bytes_written == 0 {
            let _ = std::fs::remove_file(&partial_path);
            return Err(Error::LlmAssist(format!(
                "downloaded model {model} from {} but received an empty response",
                model.download_url()
            )));
        }
        if let Some(expected) = expected_size {
            if expected != bytes_written {
                let _ = std::fs::remove_file(&partial_path);
                return Err(Error::LlmAssist(format!(
                    "downloaded model size mismatch for {model}: expected {expected} bytes, wrote {bytes_written} bytes"
                )));
            }
        }

        fs::rename(&partial_path, &final_path)?;
        let result = self
            .write_manifest(model, &final_path, bytes_written)
            .map(|_| final_path);
        let _ = lock_file.unlock();
        result
    }

    fn write_manifest(&self, model: &ModelRef, path: &Path, size_bytes: u64) -> Result<(), Error> {
        let manifest = ModelManifest {
            model_ref: model.raw().to_string(),
            repo_id: model.repo_id().to_string(),
            revision: model.revision().to_string(),
            filename: model.filename().to_string(),
            download_url: model.download_url(),
            local_path: path.display().to_string(),
            size_bytes,
            downloaded_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let manifest_path = path
            .parent()
            .ok_or_else(|| Error::LlmAssist("model path has no parent directory".to_string()))?
            .join(MODEL_MANIFEST_FILE);
        let json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(manifest_path, json)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    pub model_ref: String,
    pub repo_id: String,
    pub revision: String,
    pub filename: String,
    pub download_url: String,
    pub local_path: String,
    pub size_bytes: u64,
    pub downloaded_at_unix_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct RuntimeStore {
    root: PathBuf,
    client: reqwest::Client,
}

impl RuntimeStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn default_cache_dir() -> Result<PathBuf, Error> {
        let base = if let Some(path) = std::env::var_os(DEFAULT_CACHE_ENV) {
            PathBuf::from(path)
        } else {
            dirs::cache_dir()
                .ok_or_else(|| {
                    Error::LlmAssist(
                        "could not determine a cache directory for llama-server downloads"
                            .to_string(),
                    )
                })?
                .join("mcp-compressor")
        };
        Ok(base.join("llama.cpp"))
    }

    pub fn default_store() -> Result<Self, Error> {
        Ok(Self::new(Self::default_cache_dir()?))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn runtime_path(&self) -> Result<PathBuf, Error> {
        Ok(self.platform_dir()?.join(default_llama_server_binary()))
    }

    pub fn is_installed(&self) -> bool {
        self.runtime_path()
            .map(|path| path.is_file())
            .unwrap_or(false)
    }

    pub async fn ensure_llama_server(
        &self,
        explicit_path: Option<&Path>,
        allow_download: bool,
    ) -> Result<PathBuf, Error> {
        if let Some(path) = explicit_path {
            if path.is_file() {
                return Ok(path.to_path_buf());
            }
            return Err(Error::LlmAssist(format!(
                "configured llama-server path does not exist: {}",
                path.display()
            )));
        }
        if let Ok(path) = self.runtime_path() {
            if path.is_file() {
                return Ok(path);
            }
        }
        if let Some(path) = find_on_path(default_llama_server_binary()) {
            return Ok(path);
        }
        if !allow_download {
            return Err(Error::LlmAssist(
                "llama-server is not installed; enable download or install llama.cpp".to_string(),
            ));
        }
        self.download_llama_server().await
    }

    pub async fn download_llama_server(&self) -> Result<PathBuf, Error> {
        let final_path = self.runtime_path()?;
        if final_path.is_file() {
            return Ok(final_path);
        }
        fs::create_dir_all(self.platform_dir()?)?;
        fs::create_dir_all(&self.root)?;
        let lock_path = self.root.join("download.lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock_file.lock_exclusive()?;
        let result = self.download_llama_server_locked(&final_path).await;
        let _ = lock_file.unlock();
        result
    }

    async fn download_llama_server_locked(&self, final_path: &Path) -> Result<PathBuf, Error> {
        if final_path.is_file() {
            return Ok(final_path.to_path_buf());
        }
        let asset = LlamaCppAsset::for_current_platform()?;
        let mut last_error: Option<String> = None;
        for archive_name in asset.archive_names() {
            let archive_path = self.root.join(&archive_name);
            let download_url = asset.download_url(&archive_name);
            if !archive_path.is_file() {
                match download_file(&self.client, &download_url, &archive_path).await {
                    Ok(()) => {}
                    Err(error) => {
                        last_error = Some(error.to_string());
                        let _ = fs::remove_file(&archive_path);
                        continue;
                    }
                }
            }
            let extract_dir = self.root.join(format!("extract-{}", asset.platform));
            if extract_dir.exists() {
                fs::remove_dir_all(&extract_dir)?;
            }
            fs::create_dir_all(&extract_dir)?;
            if let Err(error) = extract_archive(&archive_path, &extract_dir) {
                last_error = Some(error.to_string());
                let _ = fs::remove_dir_all(&extract_dir);
                continue;
            }
            let extracted = match find_file_named(&extract_dir, default_llama_server_binary()) {
                Some(path) => path,
                None => {
                    last_error = Some(format!(
                        "downloaded {archive_name} but could not find {} inside it",
                        default_llama_server_binary()
                    ));
                    let _ = fs::remove_dir_all(&extract_dir);
                    continue;
                }
            };
            if let Some(parent) = final_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&extracted, final_path)?;
            make_executable(final_path)?;
            let _ = fs::remove_dir_all(&extract_dir);
            let manifest = RuntimeManifest {
                runtime: "llama-server".to_string(),
                source: "llama.cpp".to_string(),
                version: LLAMA_CPP_RELEASE_TAG.to_string(),
                platform: asset.platform.to_string(),
                archive_name,
                download_url,
                local_path: final_path.display().to_string(),
                downloaded_at_unix_seconds: now_unix_seconds(),
            };
            let json = serde_json::to_string_pretty(&manifest)?;
            fs::write(self.platform_dir()?.join(RUNTIME_MANIFEST_FILE), json)?;
            return Ok(final_path.to_path_buf());
        }
        Err(Error::LlmAssist(format!(
            "failed to download llama-server for {} from llama.cpp release {}: {}",
            asset.platform,
            LLAMA_CPP_RELEASE_TAG,
            last_error.unwrap_or_else(|| "no candidate assets were available".to_string())
        )))
    }

    fn platform_dir(&self) -> Result<PathBuf, Error> {
        Ok(self
            .root
            .join(LlamaCppAsset::for_current_platform()?.platform))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeManifest {
    pub runtime: String,
    pub source: String,
    pub version: String,
    pub platform: String,
    pub archive_name: String,
    pub download_url: String,
    pub local_path: String,
    pub downloaded_at_unix_seconds: u64,
}

#[derive(Debug, Clone)]
struct LlamaCppAsset {
    platform: &'static str,
}

impl LlamaCppAsset {
    fn for_current_platform() -> Result<Self, Error> {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let platform = match (os, arch) {
            ("macos", "aarch64") => "macos-arm64",
            ("macos", "x86_64") => "macos-x64",
            ("linux", "x86_64") => "linux-x64",
            ("linux", "aarch64") => "linux-arm64",
            ("windows", "x86_64") => "win-x64",
            _ => {
                return Err(Error::LlmAssist(format!(
                    "managed llama-server download is not available for {os}-{arch}"
                )));
            }
        };
        Ok(Self { platform })
    }

    fn archive_names(&self) -> Vec<String> {
        vec![
            format!("llama-{}-{}.zip", LLAMA_CPP_RELEASE_TAG, self.platform),
            format!("llama-{}-bin-{}.zip", LLAMA_CPP_RELEASE_TAG, self.platform),
            format!("llama-{}-{}.tar.gz", LLAMA_CPP_RELEASE_TAG, self.platform),
        ]
    }

    fn download_url(&self, archive_name: &str) -> String {
        format!(
            "{}/{}/{}",
            LLAMA_CPP_RELEASE_BASE, LLAMA_CPP_RELEASE_TAG, archive_name
        )
    }
}

#[derive(Debug, Clone)]
pub struct PreparedLlmRuntime {
    pub llama_server_path: PathBuf,
    pub model_path: PathBuf,
    pub model: ModelRef,
}

#[derive(Debug, Clone)]
pub struct LlmPreparationManager {
    config: LlmRuntimeConfig,
    state: Arc<Mutex<PreparationState>>,
    notify: Arc<Notify>,
}

#[derive(Debug, Clone)]
enum PreparationState {
    NotStarted,
    Preparing,
    Ready(PreparedLlmRuntime),
    Failed(String),
}

impl LlmPreparationManager {
    pub fn new(config: LlmRuntimeConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(PreparationState::NotStarted)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn start_background(&self) {
        let manager = self.clone();
        tokio::spawn(async move {
            let _ = manager.prepare().await;
        });
    }

    pub async fn prepare(&self) -> Result<PreparedLlmRuntime, Error> {
        {
            let mut state = self.state.lock().await;
            match &*state {
                PreparationState::Ready(prepared) => return Ok(prepared.clone()),
                PreparationState::Failed(message) => return Err(Error::LlmAssist(message.clone())),
                PreparationState::Preparing => {}
                PreparationState::NotStarted => {
                    *state = PreparationState::Preparing;
                    drop(state);
                    let result = self.prepare_inner().await;
                    let mut state = self.state.lock().await;
                    match result {
                        Ok(prepared) => {
                            *state = PreparationState::Ready(prepared.clone());
                            self.notify.notify_waiters();
                            return Ok(prepared);
                        }
                        Err(error) => {
                            let message = error.to_string();
                            *state = PreparationState::Failed(message.clone());
                            self.notify.notify_waiters();
                            return Err(Error::LlmAssist(message));
                        }
                    }
                }
            }
        }

        loop {
            self.notify.notified().await;
            let state = self.state.lock().await;
            match &*state {
                PreparationState::Ready(prepared) => return Ok(prepared.clone()),
                PreparationState::Failed(message) => return Err(Error::LlmAssist(message.clone())),
                PreparationState::Preparing | PreparationState::NotStarted => {}
            }
        }
    }

    pub async fn status(&self) -> String {
        match &*self.state.lock().await {
            PreparationState::NotStarted => "not-started".to_string(),
            PreparationState::Preparing => "preparing".to_string(),
            PreparationState::Ready(prepared) => format!(
                "ready: llama-server={}, model={}",
                prepared.llama_server_path.display(),
                prepared.model_path.display()
            ),
            PreparationState::Failed(message) => format!("failed: {message}"),
        }
    }

    async fn prepare_inner(&self) -> Result<PreparedLlmRuntime, Error> {
        let runtime_store = match &self.config.cache_dir {
            Some(cache_dir) => RuntimeStore::new(cache_dir.join("llama.cpp")),
            None => RuntimeStore::default_store()?,
        };
        let model_store = match &self.config.cache_dir {
            Some(cache_dir) => ModelStore::new(cache_dir.join("models")),
            None => ModelStore::default_store()?,
        };
        let llama_server_path = runtime_store
            .ensure_llama_server(
                self.config.llama_server_path.as_deref(),
                self.config.allow_download,
            )
            .await?;
        let model_path = model_store
            .ensure_model(&self.config.model, self.config.allow_download)
            .await?;
        Ok(PreparedLlmRuntime {
            llama_server_path,
            model_path,
            model: self.config.model.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmInstallStatus {
    pub llama_server_path: Option<PathBuf>,
    pub llama_server_ready: bool,
    pub model_path: PathBuf,
    pub model_ready: bool,
    pub model_ref: String,
}

pub fn install_status(config: &LlmRuntimeConfig) -> Result<LlmInstallStatus, Error> {
    let runtime_store = match &config.cache_dir {
        Some(cache_dir) => RuntimeStore::new(cache_dir.join("llama.cpp")),
        None => RuntimeStore::default_store()?,
    };
    let model_store = match &config.cache_dir {
        Some(cache_dir) => ModelStore::new(cache_dir.join("models")),
        None => ModelStore::default_store()?,
    };
    let llama_server_path = config
        .llama_server_path
        .clone()
        .or_else(|| {
            runtime_store
                .runtime_path()
                .ok()
                .filter(|path| path.is_file())
        })
        .or_else(|| find_on_path(default_llama_server_binary()));
    let model_path = model_store.model_path(&config.model);
    Ok(LlmInstallStatus {
        llama_server_ready: llama_server_path
            .as_ref()
            .map(|path| path.is_file())
            .unwrap_or(false),
        llama_server_path,
        model_ready: model_path.is_file(),
        model_path,
        model_ref: config.model.to_string(),
    })
}

pub async fn pull_llm_assets(config: LlmRuntimeConfig) -> Result<PreparedLlmRuntime, Error> {
    let manager = LlmPreparationManager::new(LlmRuntimeConfig {
        allow_download: true,
        ..config
    });
    manager.prepare().await
}

pub fn remove_managed_llm_assets(cache_dir: Option<PathBuf>) -> Result<(), Error> {
    let base = if let Some(cache_dir) = cache_dir {
        cache_dir
    } else if let Some(path) = std::env::var_os(DEFAULT_CACHE_ENV) {
        PathBuf::from(path)
    } else {
        dirs::cache_dir()
            .ok_or_else(|| Error::LlmAssist("could not determine cache directory".to_string()))?
            .join("mcp-compressor")
    };
    let models = base.join("models");
    let runtime = base.join("llama.cpp");
    if models.exists() {
        fs::remove_dir_all(models)?;
    }
    if runtime.exists() {
        fs::remove_dir_all(runtime)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LlmResponseFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub response_format: LlmResponseFormat,
    pub timeout: Duration,
}

impl LlmRequest {
    pub fn new(system_prompt: impl Into<String>, user_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            user_prompt: user_prompt.into(),
            response_format: LlmResponseFormat::Text,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub text: String,
    pub model: String,
}

#[async_trait]
pub trait LocalLlmRuntime: Send + Sync {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, Error>;
}

#[async_trait]
impl<T> LocalLlmRuntime for Arc<T>
where
    T: LocalLlmRuntime + ?Sized,
{
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, Error> {
        (**self).complete(request).await
    }
}

/// Runtime configuration for the reusable assistant handle.
#[derive(Debug, Clone)]
pub struct LlmRuntimeConfig {
    pub model: ModelRef,
    pub allow_download: bool,
    pub cache_dir: Option<PathBuf>,
    pub llama_server_path: Option<PathBuf>,
}

impl LlmRuntimeConfig {
    pub fn local_default(allow_download: bool) -> Self {
        Self {
            model: ModelRef::default(),
            allow_download,
            cache_dir: None,
            llama_server_path: None,
        }
    }

    pub fn with_model(mut self, model: ModelRef) -> Self {
        self.model = model;
        self
    }

    pub fn with_cache_dir(mut self, cache_dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(cache_dir.into());
        self
    }

    pub fn with_llama_server_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.llama_server_path = Some(path.into());
        self
    }
}

/// High-level assistant handle used by proxy features to send prompts.
#[derive(Clone)]
pub struct LlmAssistant {
    preparation: LlmPreparationManager,
}

impl LlmAssistant {
    pub fn from_config(config: LlmRuntimeConfig) -> Self {
        Self {
            preparation: LlmPreparationManager::new(config),
        }
    }

    pub fn start_background_preparation(&self) {
        self.preparation.start_background();
    }

    pub async fn preparation_status(&self) -> String {
        self.preparation.status().await
    }

    pub async fn complete(
        &self,
        system_prompt: impl Into<String>,
        user_prompt: impl Into<String>,
    ) -> Result<String, Error> {
        let response = self
            .complete_request(LlmRequest::new(system_prompt, user_prompt))
            .await?;
        Ok(response.text)
    }

    pub async fn complete_request(&self, request: LlmRequest) -> Result<LlmResponse, Error> {
        let prepared = self.preparation.prepare().await?;
        let runtime =
            EphemeralLlamaServerRuntime::new(prepared.model_path, prepared.model.to_string())
                .with_llama_server_path(prepared.llama_server_path);
        runtime.complete(request).await
    }
}

/// Runtime that starts official `llama-server` for one request, calls its
/// OpenAI-compatible chat endpoint, then tears it down.
///
/// The server mode is an internal detail: users configure the model, while the
/// runtime uses official llama.cpp with a clean JSON API and avoids keeping an
/// idle ~32k-context server resident for the full proxy session.
#[derive(Debug, Clone)]
pub struct EphemeralLlamaServerRuntime {
    model_path: PathBuf,
    model_ref: String,
    llama_server_path: PathBuf,
    client: reqwest::Client,
}

impl EphemeralLlamaServerRuntime {
    pub fn new(model_path: impl Into<PathBuf>, model_ref: impl Into<String>) -> Self {
        Self {
            model_path: model_path.into(),
            model_ref: model_ref.into(),
            llama_server_path: PathBuf::from(default_llama_server_binary()),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_llama_server_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.llama_server_path = path.into();
        self
    }

    async fn complete_once(&self, request: LlmRequest) -> Result<LlmResponse, Error> {
        let mut last_error: Option<Error> = None;
        for _ in 0..SERVER_START_RETRIES {
            match self.try_complete_once(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            Error::LlmAssist("llama-server failed before start was attempted".to_string())
        }))
    }

    async fn try_complete_once(&self, request: LlmRequest) -> Result<LlmResponse, Error> {
        let addr = reserve_local_addr()?;
        let base_url = format!("http://{addr}");
        let mut child = ServerChild::spawn(
            &self.llama_server_path,
            &self.model_path,
            addr,
            request.timeout,
        )?;
        child.wait_until_ready(&self.client, &base_url).await?;
        let response = self
            .post_chat_completion(&base_url, &request)
            .await
            .map_err(|error| {
                Error::LlmAssist(format!("llama-server completion failed: {error}"))
            })?;
        child.shutdown().await;
        Ok(response)
    }

    async fn post_chat_completion(
        &self,
        base_url: &str,
        request: &LlmRequest,
    ) -> Result<LlmResponse, Error> {
        #[derive(Serialize)]
        struct ChatRequest<'a> {
            model: &'a str,
            messages: Vec<ChatMessage<'a>>,
            temperature: f32,
            top_p: f32,
            max_tokens: usize,
            response_format: Option<ResponseFormat>,
        }

        #[derive(Serialize)]
        struct ChatMessage<'a> {
            role: &'a str,
            content: &'a str,
        }

        #[derive(Serialize)]
        struct ResponseFormat {
            #[serde(rename = "type")]
            kind: &'static str,
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<Choice>,
        }

        #[derive(Deserialize)]
        struct Choice {
            message: AssistantMessage,
        }

        #[derive(Deserialize)]
        struct AssistantMessage {
            content: String,
        }

        let response_format = match request.response_format {
            LlmResponseFormat::Text => None,
            LlmResponseFormat::Json => Some(ResponseFormat {
                kind: "json_object",
            }),
        };
        let body = ChatRequest {
            model: "local",
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: &request.system_prompt,
                },
                ChatMessage {
                    role: "user",
                    content: &request.user_prompt,
                },
            ],
            temperature: DEFAULT_TEMPERATURE,
            top_p: DEFAULT_TOP_P,
            max_tokens: OUTPUT_TOKEN_BUDGET,
            response_format,
        };

        let response = tokio::time::timeout(
            request.timeout,
            self.client
                .post(format!("{base_url}/v1/chat/completions"))
                .json(&body)
                .send(),
        )
        .await
        .map_err(|_| Error::LlmAssist("llama-server completion timed out".to_string()))??
        .error_for_status()?
        .json::<ChatResponse>()
        .await?;
        let text = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .ok_or_else(|| Error::LlmAssist("llama-server returned no choices".to_string()))?;
        Ok(LlmResponse {
            text,
            model: self.model_ref.clone(),
        })
    }
}

#[async_trait]
impl LocalLlmRuntime for EphemeralLlamaServerRuntime {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, Error> {
        tokio::time::timeout(request.timeout, self.complete_once(request))
            .await
            .map_err(|_| Error::LlmAssist("ephemeral llama-server request timed out".to_string()))?
    }
}

struct ServerChild {
    child: Child,
}

impl ServerChild {
    fn spawn(
        llama_server_path: &Path,
        model_path: &Path,
        addr: SocketAddr,
        timeout: Duration,
    ) -> Result<Self, Error> {
        let mut command = Command::new(llama_server_path);
        command
            .arg("-m")
            .arg(model_path)
            .arg("-c")
            .arg(INPUT_TOKEN_BUDGET.to_string())
            .arg("--host")
            .arg(addr.ip().to_string())
            .arg("--port")
            .arg(addr.port().to_string())
            .arg("--temp")
            .arg(DEFAULT_TEMPERATURE.to_string())
            .arg("--top-p")
            .arg(DEFAULT_TOP_P.to_string())
            .arg("--no-webui")
            .arg("--no-perf")
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if timeout < SERVER_READY_TIMEOUT {
            // The request-level timeout still controls the whole operation. This
            // branch is intentionally only a reminder that the child is tied to
            // the request lifetime through kill_on_drop and the outer timeout.
        }
        let child = command.spawn().map_err(|error| {
            Error::LlmAssist(format!(
                "failed to start llama-server at {}: {error}",
                llama_server_path.display()
            ))
        })?;
        Ok(Self { child })
    }

    async fn wait_until_ready(
        &mut self,
        client: &reqwest::Client,
        base_url: &str,
    ) -> Result<(), Error> {
        let deadline = tokio::time::Instant::now() + SERVER_READY_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Err(Error::LlmAssist(format!(
                    "llama-server exited before becoming ready with status {status}"
                )));
            }
            match client.get(format!("{base_url}/health")).send().await {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(_) | Err(_) => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::LlmAssist(
                    "llama-server did not become ready before timeout".to_string(),
                ));
            }
            tokio::time::sleep(SERVER_POLL_INTERVAL).await;
        }
    }

    async fn shutdown(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

impl Drop for ServerChild {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

async fn download_file(client: &reqwest::Client, url: &str, path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let partial_path = path.with_extension("part");
    let mut file = File::create(&partial_path)?;
    let response = client.get(url).send().await?.error_for_status()?;
    let expected_size = response.content_length();
    let mut stream = response.bytes_stream();
    let mut bytes_written = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        bytes_written += chunk.len() as u64;
    }
    file.flush()?;
    drop(file);
    if bytes_written == 0 {
        let _ = fs::remove_file(&partial_path);
        return Err(Error::LlmAssist(format!(
            "download from {url} returned no bytes"
        )));
    }
    if let Some(expected) = expected_size {
        if expected != bytes_written {
            let _ = fs::remove_file(&partial_path);
            return Err(Error::LlmAssist(format!(
                "download size mismatch for {url}: expected {expected} bytes, wrote {bytes_written} bytes"
            )));
        }
    }
    fs::rename(&partial_path, path)?;
    Ok(())
}

fn extract_archive(archive_path: &Path, destination: &Path) -> Result<(), Error> {
    let name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name.ends_with(".zip") {
        let file = File::open(archive_path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|error| Error::LlmAssist(format!("failed to open zip archive: {error}")))?;
        archive
            .extract(destination)
            .map_err(|error| Error::LlmAssist(format!("failed to extract zip archive: {error}")))?;
        Ok(())
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let file = File::open(archive_path)?;
        let decoder = GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(destination)?;
        Ok(())
    } else {
        Err(Error::LlmAssist(format!(
            "unsupported llama.cpp archive format: {}",
            archive_path.display()
        )))
    }
}

fn find_file_named(root: &Path, file_name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let entries = fs::read_dir(&path).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
                return Some(path);
            }
        }
    }
    None
}

fn find_on_path(binary_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), Error> {
    Ok(())
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn reserve_local_addr() -> Result<SocketAddr, Error> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
    let addr = listener.local_addr()?;
    drop(listener);
    Ok(addr)
}

fn derive_gguf_filename(repo_id: &str, quantization: Option<&str>) -> Result<String, Error> {
    let repo_name = repo_id
        .rsplit('/')
        .next()
        .ok_or_else(|| Error::LlmAssist(format!("invalid model repo id: {repo_id}")))?;
    let base = repo_name.strip_suffix("-GGUF").unwrap_or(repo_name);
    match quantization {
        Some(quant) => Ok(format!("{base}-{quant}.gguf")),
        None => Err(Error::LlmAssist(format!(
            "model reference {repo_id} must include a quantization suffix like :Q4_K_M or an explicit #filename"
        ))),
    }
}

fn validate_safe_filename(filename: &str) -> Result<(), Error> {
    if filename.contains('/')
        || filename.contains('\\')
        || filename.contains("..")
        || !filename.ends_with(".gguf")
    {
        return Err(Error::LlmAssist(format!(
            "model filename must be a safe .gguf filename: {filename}"
        )));
    }
    Ok(())
}

fn sanitize_path_segment(segment: &str) -> String {
    segment
        .chars()
        .map(|ch| match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}

fn default_llama_server_binary() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_model_reference() {
        let model = ModelRef::parse(DEFAULT_MODEL_REF).unwrap();
        assert_eq!(model.repo_id(), "LiquidAI/LFM2.5-350M-GGUF");
        assert_eq!(model.quantization(), Some("Q4_K_M"));
        assert_eq!(model.filename(), "LFM2.5-350M-Q4_K_M.gguf");
        assert_eq!(
            model.download_url(),
            "https://huggingface.co/LiquidAI/LFM2.5-350M-GGUF/resolve/main/LFM2.5-350M-Q4_K_M.gguf"
        );
    }

    #[test]
    fn parses_explicit_filename_reference() {
        let model = ModelRef::parse("Org/Repo-GGUF#custom-name.gguf").unwrap();
        assert_eq!(model.repo_id(), "Org/Repo-GGUF");
        assert_eq!(model.quantization(), None);
        assert_eq!(model.filename(), "custom-name.gguf");
    }

    #[test]
    fn rejects_unsafe_filename_reference() {
        assert!(ModelRef::parse("Org/Repo#../bad.gguf").is_err());
        assert!(ModelRef::parse("Org/Repo#bad.bin").is_err());
    }

    #[test]
    fn model_store_uses_sanitized_cache_key() {
        let store = ModelStore::new(PathBuf::from("/cache"));
        let model = ModelRef::parse(DEFAULT_MODEL_REF).unwrap();
        let path = store.model_path(&model);
        assert!(path.ends_with(Path::new(
            "LiquidAI/LFM2.5-350M-GGUF/main/LFM2.5-350M-Q4_K_M.gguf"
        )));
    }

    #[test]
    fn local_default_config_uses_default_model() {
        let config = LlmRuntimeConfig::local_default(false);
        assert_eq!(config.model.raw(), DEFAULT_MODEL_REF);
        assert!(!config.allow_download);
        assert!(config.cache_dir.is_none());
        assert!(config.llama_server_path.is_none());
    }

    #[test]
    fn config_accepts_llama_server_path() {
        let config =
            LlmRuntimeConfig::local_default(false).with_llama_server_path("/bin/llama-server");
        assert_eq!(
            config.llama_server_path,
            Some(PathBuf::from("/bin/llama-server"))
        );
    }

    #[test]
    fn reserve_local_addr_returns_localhost() {
        let addr = reserve_local_addr().unwrap();
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_ne!(addr.port(), 0);
    }

    #[tokio::test]
    async fn ensure_model_without_download_returns_actionable_error() {
        let tempdir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tempdir.path());
        let model = ModelRef::parse(DEFAULT_MODEL_REF).unwrap();
        let error = store.ensure_model(&model, false).await.unwrap_err();
        let message = error.to_string();
        assert!(message.contains("is not cached"));
        assert!(message.contains("enable model download"));
    }
}
