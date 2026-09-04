//! Local model inference engine.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::services::llm_client::{LocalModelConfig, ModelCapabilities, ModelEngine, ModelType};

const CANCELLED: &str = "Agent task cancelled";
const FEATURE_DISABLED: &str = "Local inference requires the `llama-cpp` feature";

/// Local inference configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalInferenceConfig {
    pub model_path: PathBuf,
    pub model_type: ModelType,
    pub model_file: String,
    pub n_threads: i32,
    pub n_ctx: u32,
    pub n_gpu_layers: i32,
    pub n_batch: i32,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub max_tokens: u32,
}

impl Default for LocalInferenceConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("~/.agent-ide/models"),
            model_type: ModelType::StarCoder,
            model_file: String::new(),
            n_threads: 4,
            n_ctx: 2048,
            n_gpu_layers: -1,
            n_batch: 512,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 512,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceResult {
    pub text: String,
    pub tokens_generated: u32,
    pub inference_time_ms: u64,
    pub tokens_per_second: f32,
    pub memory_used_mb: u64,
}

pub struct LlamaCppEngine {
    config: LocalInferenceConfig,
    capabilities: ModelCapabilities,
    model_loaded: Arc<AtomicBool>,
}

impl LlamaCppEngine {
    pub fn new(config: LocalInferenceConfig) -> Result<Self, String> {
        let capabilities = Self::determine_capabilities(&config);
        Ok(Self {
            config,
            capabilities,
            model_loaded: Arc::new(AtomicBool::new(false)),
        })
    }

    fn determine_capabilities(config: &LocalInferenceConfig) -> ModelCapabilities {
        ModelCapabilities {
            max_context_tokens: config.n_ctx,
            supported_languages: [
                "typescript",
                "javascript",
                "python",
                "rust",
                "go",
                "java",
                "cpp",
                "c",
                "html",
                "css",
                "json",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            inference_speed_ms: 50.0,
            memory_requirement_mb: 4096,
            supports_streaming: true,
            supports_tool_calls: false,
        }
    }

    fn model_full_path(&self) -> PathBuf {
        expand_model_path(&self.config.model_path).join(&self.config.model_file)
    }

    pub fn model_path(&self) -> PathBuf {
        self.model_full_path()
    }

    pub async fn load_model(&self) -> Result<(), String> {
        if self.model_loaded.load(Ordering::Acquire) {
            return Ok(());
        }
        let model_path = self.model_full_path();
        if !model_path.is_file() {
            return Err(format!("Model file not found: {}", model_path.display()));
        }

        #[cfg(feature = "llama-cpp")]
        {
            let path = model_path.clone();
            let config = self.config.clone();
            tokio::task::spawn_blocking(move || load_llama_model(&path, &config))
                .await
                .map_err(|e| format!("Model load task failed: {e}"))??;
            self.model_loaded.store(true, Ordering::Release);
            Ok(())
        }
        #[cfg(not(feature = "llama-cpp"))]
        {
            let _ = model_path;
            Err(FEATURE_DISABLED.to_string())
        }
    }

    pub async fn unload_model(&self) {
        self.model_loaded.store(false, Ordering::Release);
    }

    pub fn is_model_loaded(&self) -> bool {
        self.model_loaded.load(Ordering::Acquire)
    }

    pub async fn generate_text(&self, prompt: &str) -> Result<InferenceResult, String> {
        self.ensure_ready()?;
        let start = std::time::Instant::now();

        #[cfg(feature = "llama-cpp")]
        let (text, tokens_generated) = {
            let prompt = prompt.to_string();
            let config = self.config.clone();
            let model_path = self.model_full_path();
            tokio::task::spawn_blocking(move || generate_with_llama(&model_path, &config, &prompt))
                .await
                .map_err(|e| format!("Inference task failed: {e}"))??
        };
        #[cfg(not(feature = "llama-cpp"))]
        let (text, tokens_generated): (String, u32) = {
            let _ = prompt;
            return Err(FEATURE_DISABLED.to_string());
        };

        Ok(self.result(text, tokens_generated, start.elapsed()))
    }

    pub async fn generate_text_stream(
        &self,
        prompt: &str,
        tx: mpsc::Sender<String>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<InferenceResult, String> {
        self.ensure_ready()?;
        if cancel_flag.load(Ordering::Acquire) {
            return Err(CANCELLED.to_string());
        }
        let start = std::time::Instant::now();

        #[cfg(feature = "llama-cpp")]
        let text = generate_stream_with_llama(
            self.model_full_path(),
            self.config.clone(),
            prompt.to_string(),
            tx,
            cancel_flag.clone(),
        )
        .await?;
        #[cfg(not(feature = "llama-cpp"))]
        let text: String = {
            let _ = (prompt, tx, cancel_flag);
            return Err(FEATURE_DISABLED.to_string());
        };

        let tokens_generated = text.chars().count() as u32;
        Ok(self.result(text, tokens_generated, start.elapsed()))
    }

    fn ensure_ready(&self) -> Result<(), String> {
        if !self.is_model_loaded() {
            return Err("Model not loaded. Call load_model() first.".to_string());
        }
        Ok(())
    }

    fn result(
        &self,
        text: String,
        tokens_generated: u32,
        elapsed: std::time::Duration,
    ) -> InferenceResult {
        let milliseconds = elapsed.as_millis() as u64;
        InferenceResult {
            text,
            tokens_generated,
            inference_time_ms: milliseconds,
            tokens_per_second: if milliseconds == 0 {
                0.0
            } else {
                tokens_generated as f32 * 1000.0 / milliseconds as f32
            },
            memory_used_mb: self.estimate_memory_usage(),
        }
    }

    fn estimate_memory_usage(&self) -> u64 {
        2048 + (self.config.n_ctx as u64 / 2)
    }

    pub fn get_stats(&self) -> EngineStats {
        EngineStats {
            model_loaded: self.is_model_loaded(),
            config: self.config.clone(),
            capabilities: self.capabilities.clone(),
        }
    }
}

pub fn expand_model_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        if let Some(home) = dirs_next::home_dir() {
            return if value == "~" {
                home
            } else {
                home.join(&value[2..])
            };
        }
    }
    path.to_path_buf()
}

#[cfg(feature = "llama-cpp")]
fn llama_options(config: &LocalInferenceConfig) -> llama_cpp_rs::options::ModelOptions {
    let mut options = llama_cpp_rs::options::ModelOptions::default();
    options.set_context(config.n_ctx as i32);
    options.set_batch(config.n_batch);
    options.set_gpu_layers(config.n_gpu_layers);
    options
}

#[cfg(feature = "llama-cpp")]
fn predict_options(config: &LocalInferenceConfig) -> llama_cpp_rs::options::PredictOptions {
    let mut options = llama_cpp_rs::options::PredictOptions::default();
    options.set_threads(config.n_threads);
    options.set_tokens(config.max_tokens as i32);
    options.set_top_k(config.top_k);
    options.set_top_p(config.top_p);
    options.set_temperature(config.temperature);
    options.set_batch(config.n_batch);
    options
}

#[cfg(feature = "llama-cpp")]
fn load_llama_model(path: &Path, config: &LocalInferenceConfig) -> Result<(), String> {
    let _model =
        llama_cpp_rs::LLama::new(path.to_string_lossy().into_owned(), &llama_options(config))
            .map_err(|e| format!("Failed to load llama model: {e}"))?;
    Ok(())
}

#[cfg(feature = "llama-cpp")]
fn generate_with_llama(
    path: &Path,
    config: &LocalInferenceConfig,
    prompt: &str,
) -> Result<(String, u32), String> {
    let model =
        llama_cpp_rs::LLama::new(path.to_string_lossy().into_owned(), &llama_options(config))
            .map_err(|e| format!("Failed to load llama model: {e}"))?;
    let output = model
        .predict(prompt.to_string(), predict_options(config))
        .map_err(|e| format!("Llama prediction failed: {e}"))?;
    Ok((output.clone(), output.chars().count() as u32))
}

#[cfg(feature = "llama-cpp")]
async fn generate_stream_with_llama(
    path: PathBuf,
    config: LocalInferenceConfig,
    prompt: String,
    tx: mpsc::Sender<String>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<String, String> {
    let (parts_tx, mut parts_rx) = mpsc::channel::<String>(32);
    let callback_cancel = cancel_flag.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let model =
            llama_cpp_rs::LLama::new(path.to_string_lossy().into_owned(), &llama_options(&config))
                .map_err(|e| format!("Failed to load llama model: {e}"))?;
        let mut options = predict_options(&config);
        options.set_token_callback(Some(Box::new(move |part| {
            if callback_cancel.load(Ordering::Acquire) {
                return false;
            }
            parts_tx.blocking_send(part).is_ok()
        })));
        model
            .predict(prompt, options)
            .map_err(|e| format!("Llama prediction failed: {e}"))
    });

    let mut full_text = String::new();
    while let Some(part) = parts_rx.recv().await {
        if cancel_flag.load(Ordering::Acquire) {
            handle.abort();
            return Err(CANCELLED.to_string());
        }
        full_text.push_str(&part);
        tx.send(part)
            .await
            .map_err(|_| "LLM stream receiver dropped".to_string())?;
    }
    let predicted = handle
        .await
        .map_err(|e| format!("Inference task failed: {e}"))??;
    if cancel_flag.load(Ordering::Acquire) {
        return Err(CANCELLED.to_string());
    }
    if full_text.is_empty() {
        full_text = predicted;
    }
    Ok(full_text)
}

#[async_trait]
impl ModelEngine for LlamaCppEngine {
    async fn load_model(&self) -> Result<(), String> {
        LlamaCppEngine::load_model(self).await
    }

    async fn generate(&self, prompt: &str) -> Result<String, String> {
        Ok(self.generate_text(prompt).await?.text)
    }

    async fn generate_stream(
        &self,
        prompt: &str,
        tx: mpsc::Sender<String>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<String, String> {
        Ok(self
            .generate_text_stream(prompt, tx, cancel_flag)
            .await?
            .text)
    }

    fn capabilities(&self) -> ModelCapabilities {
        self.capabilities.clone()
    }

    fn model_type(&self) -> ModelType {
        self.config.model_type.clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineStats {
    pub model_loaded: bool,
    pub config: LocalInferenceConfig,
    pub capabilities: ModelCapabilities,
}

pub struct ModelDownloader {
    cache_dir: PathBuf,
}

impl ModelDownloader {
    pub fn new() -> Self {
        let cache_dir = dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".agent-ide")
            .join("models");
        let _ = std::fs::create_dir_all(&cache_dir);
        Self { cache_dir }
    }

    #[cfg(feature = "llama-cpp")]
    pub async fn download_model(
        &self,
        model_url: &str,
        model_name: &str,
    ) -> Result<PathBuf, String> {
        let model_path = self.cache_dir.join(model_name);
        if model_path.exists() {
            return Ok(model_path);
        }
        let url = model_url.to_string();
        let path = model_path.clone();
        tokio::task::spawn_blocking(move || {
            let response = ureq::get(&url)
                .call()
                .map_err(|e| format!("Failed to download model: {e}"))?;
            let mut reader = response.into_reader();
            let mut file = std::fs::File::create(&path)
                .map_err(|e| format!("Failed to create model file: {e}"))?;
            std::io::copy(&mut reader, &mut file)
                .map_err(|e| format!("Failed to write model file: {e}"))?;
            Ok(path)
        })
        .await
        .map_err(|e| format!("Download task failed: {e}"))?
    }

    #[cfg(not(feature = "llama-cpp"))]
    pub async fn download_model(
        &self,
        _model_url: &str,
        _model_name: &str,
    ) -> Result<PathBuf, String> {
        Err(FEATURE_DISABLED.to_string())
    }

    pub fn model_exists(&self, model_name: &str) -> bool {
        self.cache_dir.join(model_name).is_file()
    }

    pub fn get_model_path(&self, model_name: &str) -> PathBuf {
        self.cache_dir.join(model_name)
    }

    pub fn list_models(&self) -> Vec<String> {
        std::fs::read_dir(&self.cache_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn delete_model(&self, model_name: &str) -> Result<(), String> {
        std::fs::remove_file(self.cache_dir.join(model_name))
            .map_err(|e| format!("Failed to delete model: {e}"))
    }
}

impl Default for ModelDownloader {
    fn default() -> Self {
        Self::new()
    }
}

struct RunningTask {
    task_id: tokio::task::Id,
    cancel_flag: Arc<AtomicBool>,
    abort_handle: tokio::task::AbortHandle,
}

pub struct InferenceTaskManager {
    running_tasks: std::sync::Mutex<Vec<RunningTask>>,
}

impl InferenceTaskManager {
    pub fn new() -> Self {
        Self {
            running_tasks: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub async fn start_task<F, Fut>(&self, task_factory: F) -> Result<String, String>
    where
        F: FnOnce(Arc<AtomicBool>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let task_flag = cancel_flag.clone();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            let result = task_factory(task_flag).await;
            let _ = result_tx.send(result);
        });
        let task_id = handle.id();
        let abort_handle = handle.abort_handle();
        self.running_tasks
            .lock()
            .map_err(|e| e.to_string())?
            .push(RunningTask {
                task_id,
                cancel_flag,
                abort_handle,
            });

        let result = result_rx.await.map_err(|_| CANCELLED.to_string())?;
        if let Ok(mut tasks) = self.running_tasks.lock() {
            tasks.retain(|task| task.task_id != task_id);
        }
        result
    }

    pub async fn cancel_all_tasks(&self) {
        if let Ok(mut tasks) = self.running_tasks.lock() {
            for task in tasks.drain(..) {
                task.cancel_flag.store(true, Ordering::Release);
                task.abort_handle.abort();
            }
        }
    }

    pub fn active_task_count(&self) -> usize {
        self.running_tasks
            .lock()
            .map(|tasks| tasks.len())
            .unwrap_or(0)
    }
}

impl Default for InferenceTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_llama_engine_creation() {
        let config = LocalInferenceConfig {
            model_file: "test-model.gguf".to_string(),
            ..Default::default()
        };
        assert!(LlamaCppEngine::new(config).is_ok());
    }

    #[tokio::test]
    async fn test_model_downloader() {
        let downloader = ModelDownloader::new();
        let _models = downloader.list_models();
    }

    #[tokio::test]
    async fn test_inference_config_default() {
        let config = LocalInferenceConfig::default();
        assert_eq!(config.n_threads, 4);
        assert_eq!(config.n_ctx, 2048);
        assert_eq!(config.temperature, 0.7);
    }

    #[tokio::test]
    async fn test_task_manager() {
        let manager = InferenceTaskManager::new();
        assert_eq!(manager.active_task_count(), 0);
    }
}
