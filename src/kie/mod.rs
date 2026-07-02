pub mod catalog;
pub mod client;
pub mod jobs;
pub mod normalize;

pub use client::KieClient;
pub use jobs::{
    CreateTaskResponse, CreditsResponse, GenerationKind, GenerationRequest, GenerationResult,
    TaskRecord, TaskState, UploadedInput,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KieError {
    #[error("KIE_API_KEY is not set")]
    MissingApiKey,
    #[error("unsupported {kind} model: {model}")]
    UnsupportedModel { kind: &'static str, model: String },
    #[error("prompt must not be empty")]
    EmptyPrompt,
    #[error("local input path does not exist: {path}")]
    MissingLocalInput { path: String },
    #[error("invalid local input path {path}: {message}")]
    InvalidLocalInput { path: String, message: String },
    #[error("local input file is too large: {path} is {size} bytes, limit is {limit} bytes")]
    LocalInputTooLarge { path: String, size: u64, limit: u64 },
    #[error("invalid configuration {name}: {message}")]
    InvalidConfig { name: &'static str, message: String },
    #[error("Kie API returned HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("Kie API returned code {code}: {message}")]
    ApiCode { code: i64, message: String },
    #[error("task {task_id} failed: {message}")]
    TaskFailed { task_id: String, message: String },
    #[error("task {task_id} returned unknown state: {state}")]
    UnexpectedTaskState { task_id: String, state: String },
    #[error("task {task_id} timed out after {seconds}s")]
    Timeout { task_id: String, seconds: u64 },
    #[error("task {task_id} timed out after {seconds}s; last polling error: {last_error}")]
    PollingTimeout {
        task_id: String,
        seconds: u64,
        last_error: String,
    },
    #[error("task {task_id} completed without downloadable media")]
    NoMedia { task_id: String },
    #[error("invalid request: {message}")]
    InvalidRequest { message: String },
    #[error("invalid response from Kie: {message}")]
    InvalidResponse { message: String },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
