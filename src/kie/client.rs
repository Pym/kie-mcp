use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant, UNIX_EPOCH},
};

use reqwest::{StatusCode, multipart};
use serde_json::{Value, json};
use tokio::{sync::Mutex as AsyncMutex, time::sleep};
use tracing::{debug, warn};

use crate::{
    config::Config,
    media::{SavedMedia, file_extension_from_url, preview_markdown, safe_stem},
};

use super::{
    KieError,
    jobs::{
        ApiEnvelope, CreateTaskData, CreateTaskResponse, CreditsResponse, GenerationKind,
        GenerationRequest, GenerationResult, TaskRecord, UploadedInput, create_task_payload,
        has_explicit_media_input, validate_generation_request, validate_model,
    },
    normalize::extract_media_urls,
};

#[derive(Debug, Clone)]
pub struct KieClient {
    http: reqwest::Client,
    config: Config,
    upload_cache: Arc<StdMutex<HashMap<UploadCacheKey, UploadedInput>>>,
    upload_locks: Arc<StdMutex<HashMap<UploadCacheKey, Arc<AsyncMutex<()>>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct UploadCacheKey {
    path: PathBuf,
    len: u64,
    modified_nanos: Option<u128>,
}

#[derive(Debug, Clone)]
struct LocalUpload {
    path: PathBuf,
    file_name: String,
    mime: String,
    key: UploadCacheKey,
}

impl KieClient {
    pub fn new(config: Config) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(config.http_timeout)
                .build()
                .expect("valid reqwest client configuration"),
            config,
            upload_cache: Arc::new(StdMutex::new(HashMap::new())),
            upload_locks: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    pub async fn credits(&self) -> Result<CreditsResponse, KieError> {
        let response = self
            .http
            .get(format!("{}/api/v1/chat/credit", self.config.api_base))
            .bearer_auth(self.config.require_api_key()?)
            .send()
            .await?;
        let credits: CreditsResponse = parse_response(response).await?;
        credits.into_success()
    }

    pub async fn upload_file(&self, path: &Path) -> Result<UploadedInput, KieError> {
        let local = self.local_upload(path).await?;
        let key = local.key.clone();
        if let Ok(cache) = self.upload_cache.lock()
            && let Some(cached) = cache.get(&key)
        {
            return Ok(cached.clone());
        }

        let upload_lock = self.upload_lock(&key);
        let _guard = upload_lock.lock().await;
        if let Ok(cache) = self.upload_cache.lock()
            && let Some(cached) = cache.get(&key)
        {
            return Ok(cached.clone());
        }

        let uploaded = self.upload_file_uncached(&local).await?;
        if let Ok(mut cache) = self.upload_cache.lock() {
            cache.retain(|cached_key, _| cached_key.path != key.path || cached_key == &key);
            cache.insert(key, uploaded.clone());
        }
        self.remove_upload_lock(&local.key, &upload_lock);
        Ok(uploaded)
    }

    fn upload_lock(&self, key: &UploadCacheKey) -> Arc<AsyncMutex<()>> {
        let Ok(mut locks) = self.upload_locks.lock() else {
            return Arc::new(AsyncMutex::new(()));
        };
        locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn remove_upload_lock(&self, key: &UploadCacheKey, lock: &Arc<AsyncMutex<()>>) {
        let Ok(mut locks) = self.upload_locks.lock() else {
            return;
        };
        if locks
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, lock))
        {
            locks.remove(key);
        }
    }

    async fn upload_file_uncached(&self, local: &LocalUpload) -> Result<UploadedInput, KieError> {
        let bytes = tokio::fs::read(&local.path).await?;
        let part = multipart::Part::bytes(bytes)
            .file_name(local.file_name.clone())
            .mime_str(&local.mime)?;
        let form = multipart::Form::new()
            .part("file", part)
            .text("uploadPath", "kie-mcp")
            .text("fileName", local.file_name.clone());

        let response = self
            .http
            .post(format!(
                "{}/api/file-stream-upload",
                self.config.upload_base
            ))
            .bearer_auth(self.config.require_api_key()?)
            .multipart(form)
            .send()
            .await?;
        let envelope: Value = parse_response(response).await?;
        if envelope.get("code").and_then(Value::as_i64).unwrap_or(200) != 200
            && !envelope
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            return Err(KieError::ApiCode {
                code: envelope.get("code").and_then(Value::as_i64).unwrap_or(500),
                message: envelope
                    .get("msg")
                    .and_then(Value::as_str)
                    .unwrap_or("upload failed")
                    .to_string(),
            });
        }
        let url = envelope
            .pointer("/data/fileUrl")
            .and_then(Value::as_str)
            .or_else(|| {
                envelope
                    .pointer("/data/downloadUrl")
                    .and_then(Value::as_str)
            })
            .ok_or_else(|| KieError::InvalidResponse {
                message: "upload response did not include fileUrl".to_string(),
            })?
            .to_string();

        Ok(UploadedInput {
            path: local.path.clone(),
            url,
        })
    }

    async fn local_upload(&self, path: &Path) -> Result<LocalUpload, KieError> {
        let display_path = path.display().to_string();
        let path = canonicalize_local_input(path).await?;
        let metadata = tokio::fs::metadata(&path).await?;
        if !metadata.is_file() {
            return Err(KieError::InvalidLocalInput {
                path: display_path,
                message: "path must be a regular file".to_string(),
            });
        }
        if metadata.len() > self.config.max_upload_bytes {
            return Err(KieError::LocalInputTooLarge {
                path: path.display().to_string(),
                size: metadata.len(),
                limit: self.config.max_upload_bytes,
            });
        }
        self.ensure_input_root(&path).await?;
        let mime = supported_upload_mime(&path)?;
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("input.bin")
            .to_string();
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_nanos());
        let key = UploadCacheKey {
            path: path.clone(),
            len: metadata.len(),
            modified_nanos,
        };
        Ok(LocalUpload {
            path,
            file_name,
            mime,
            key,
        })
    }

    async fn ensure_input_root(&self, path: &Path) -> Result<(), KieError> {
        if self.config.input_roots.is_empty() {
            return Ok(());
        }
        for root in &self.config.input_roots {
            let root = canonicalize_input_root(root).await?;
            if path.starts_with(&root) {
                return Ok(());
            }
        }
        Err(KieError::InvalidLocalInput {
            path: path.display().to_string(),
            message: "path is outside configured KIE_MCP_INPUT_ROOTS".to_string(),
        })
    }

    pub async fn create_task(
        &self,
        request: &GenerationRequest,
        kind: GenerationKind,
    ) -> Result<String, KieError> {
        validate_model(&request.model, kind)?;
        let spec = super::catalog::resolve_model(&request.model, kind);
        validate_generation_request(request, spec)?;
        let model = spec.map_or(request.model.as_str(), |spec| spec.id);
        let mut uploaded = Vec::new();
        if has_explicit_media_input(&request.input)
            && (!request.input_urls.is_empty() || !request.local_input_paths.is_empty())
        {
            return Err(KieError::InvalidRequest {
                message: "input already contains explicit media fields; use either input media fields or top-level input_urls/local_input_paths, not both".to_string(),
            });
        }
        for path in &request.local_input_paths {
            uploaded.push(self.upload_file(path).await?);
        }
        let payload = create_task_payload(request, &uploaded, model, spec)?;
        debug!(model = %model, requested_model = %request.model, "creating Kie task");
        let response = self
            .http
            .post(format!("{}/api/v1/jobs/createTask", self.config.api_base))
            .bearer_auth(self.config.require_api_key()?)
            .json(&payload)
            .send()
            .await?;
        let envelope: CreateTaskResponse = parse_response(response).await?;
        let data: CreateTaskData = envelope.into_data()?;
        Ok(data.task_id)
    }

    pub async fn record_info(&self, task_id: &str) -> Result<TaskRecord, KieError> {
        let response = self
            .http
            .get(format!("{}/api/v1/jobs/recordInfo", self.config.api_base))
            .bearer_auth(self.config.require_api_key()?)
            .query(&[("taskId", task_id)])
            .send()
            .await?;
        let envelope: ApiEnvelope<TaskRecord> = parse_response(response).await?;
        envelope.into_data()
    }

    pub async fn generate_and_wait(
        &self,
        request: GenerationRequest,
        kind: GenerationKind,
    ) -> Result<GenerationResult, KieError> {
        let task_id = self.create_task(&request, kind).await?;
        let record = self.wait_for_success(&task_id).await?;
        self.download_completed(record, kind, request.output_name.as_deref())
            .await
    }

    pub async fn wait_for_success(&self, task_id: &str) -> Result<TaskRecord, KieError> {
        let deadline = Instant::now() + self.config.timeout;
        let mut delay = Duration::from_secs(2);
        loop {
            let record = match self.record_info(task_id).await {
                Ok(record) => record,
                Err(err) if is_transient_poll_error(&err) => {
                    let message = redact(&err.to_string());
                    if Instant::now() >= deadline {
                        return Err(KieError::PollingTimeout {
                            task_id: task_id.to_string(),
                            seconds: self.config.timeout.as_secs(),
                            last_error: message,
                        });
                    }
                    warn!(error = %message, "transient Kie polling error; retrying");
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let sleep_for = delay.min(remaining);
                    if sleep_for.is_zero() {
                        return Err(KieError::PollingTimeout {
                            task_id: task_id.to_string(),
                            seconds: self.config.timeout.as_secs(),
                            last_error: message,
                        });
                    }
                    sleep(sleep_for).await;
                    delay = (delay + Duration::from_secs(1)).min(Duration::from_secs(10));
                    continue;
                }
                Err(err) => return Err(err),
            };
            if record.state.is_pending() {
                if Instant::now() >= deadline {
                    return Err(KieError::Timeout {
                        task_id: task_id.to_string(),
                        seconds: self.config.timeout.as_secs(),
                    });
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                sleep(delay.min(remaining)).await;
                delay = (delay + Duration::from_secs(1)).min(Duration::from_secs(10));
                continue;
            }
            if record.state == super::jobs::TaskState::Success {
                return Ok(record);
            }
            if let super::jobs::TaskState::Unknown(state) = &record.state {
                return Err(KieError::UnexpectedTaskState {
                    task_id: record.task_id,
                    state: state.clone(),
                });
            }
            return Err(KieError::TaskFailed {
                task_id: record.task_id,
                message: if record.fail_msg.is_empty() {
                    "Kie task failed".to_string()
                } else {
                    record.fail_msg
                },
            });
        }
    }

    pub async fn download_completed(
        &self,
        record: TaskRecord,
        kind: GenerationKind,
        output_name: Option<&str>,
    ) -> Result<GenerationResult, KieError> {
        let urls = extract_media_urls(&record.result_json);
        if urls.result_urls.is_empty() {
            return Err(KieError::NoMedia {
                task_id: record.task_id.clone(),
            });
        }
        let dir = self
            .config
            .output_dir
            .join(output_dir_name(output_name, &record.task_id));
        tokio::fs::create_dir_all(&dir).await?;

        let task_stem = safe_stem(Some(&record.task_id), "task");
        let stem = safe_stem(output_name, &task_stem);
        let media = self
            .download_url_set(&urls.result_urls, &dir, &stem, kind, false)
            .await?;
        let posters = self
            .download_url_set(
                &urls.poster_urls,
                &dir,
                "poster",
                GenerationKind::Image,
                true,
            )
            .await?;
        let markdown = preview_markdown(&media, &posters);

        Ok(GenerationResult {
            task_id: record.task_id.clone(),
            model: record.model.clone(),
            state: record.state.clone(),
            media_type: kind,
            media,
            source_urls: urls.result_urls,
            poster_urls: urls.poster_urls,
            markdown,
        })
    }

    async fn download_url_set(
        &self,
        urls: &[String],
        dir: &Path,
        stem: &str,
        kind: GenerationKind,
        poster: bool,
    ) -> Result<Vec<SavedMedia>, KieError> {
        let mut saved = Vec::with_capacity(urls.len());
        for (idx, url) in urls.iter().enumerate() {
            match self.download_one(url, dir, stem, idx, kind, poster).await {
                Ok(item) => saved.push(item),
                Err(err) if poster => {
                    warn!(error = %redact(&err.to_string()), "poster download failed")
                }
                Err(err) => return Err(err),
            }
        }
        Ok(saved)
    }

    async fn download_one(
        &self,
        url: &str,
        dir: &Path,
        stem: &str,
        idx: usize,
        kind: GenerationKind,
        poster: bool,
    ) -> Result<SavedMedia, KieError> {
        self.ensure_download_url(url, false)?;
        let download_url = match self.resolve_download_url(url).await {
            Ok(download_url) => download_url,
            Err(err) if is_transient_download_resolver_error(&err) => {
                warn!(
                    error = %redact(&err.to_string()),
                    "direct download URL resolution failed transiently; trying original URL"
                );
                url.to_string()
            }
            Err(err) => return Err(err),
        };
        self.ensure_download_url(&download_url, true)?;
        let response = self.http.get(&download_url).send().await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if !status.is_success() {
            return Err(KieError::HttpStatus {
                status: status.as_u16(),
                body: redact(&response.text().await.unwrap_or_default()),
            });
        }
        let ext = file_extension_from_url(&download_url, kind, content_type.as_deref());
        let name = if idx > 0 {
            format!("{stem}-{}.{}", idx + 1, ext)
        } else {
            format!("{stem}.{ext}")
        };
        let path = dir.join(name);
        let bytes = response.bytes().await?;
        tokio::fs::write(&path, bytes).await?;
        Ok(SavedMedia {
            source_url: url.to_string(),
            path: std::fs::canonicalize(path)?,
            kind: if poster || kind == GenerationKind::Image {
                "image".to_string()
            } else {
                "video".to_string()
            },
        })
    }

    async fn resolve_download_url(&self, url: &str) -> Result<String, KieError> {
        let response = self
            .http
            .post(format!(
                "{}/api/v1/common/download-url",
                self.config.api_base
            ))
            .bearer_auth(self.config.require_api_key()?)
            .json(&json!({ "url": url }))
            .send()
            .await?;
        let envelope: ApiEnvelope<String> = parse_response(response).await?;
        envelope.into_data()
    }

    fn ensure_download_url(
        &self,
        url: &str,
        allow_configured_api_host: bool,
    ) -> Result<(), KieError> {
        let allowed_host = allow_configured_api_host
            .then(|| api_base_host(&self.config.api_base))
            .flatten();
        ensure_safe_download_url(url, allowed_host.as_deref())
    }

    pub fn result_to_json(result: &GenerationResult) -> Value {
        json!({
            "task_id": result.task_id,
            "model": result.model,
            "state": result.state,
            "media_type": result.media_type,
            "media": result.media,
            "source_urls": result.source_urls,
            "poster_urls": result.poster_urls,
            "markdown": result.markdown,
        })
    }
}

fn is_transient_poll_error(err: &KieError) -> bool {
    match err {
        KieError::HttpStatus { status, .. } => *status == 408 || *status == 429 || *status >= 500,
        KieError::Reqwest(_) => true,
        _ => false,
    }
}

fn is_transient_download_resolver_error(err: &KieError) -> bool {
    match err {
        KieError::HttpStatus { status, .. } => *status == 408 || *status == 429 || *status >= 500,
        KieError::Reqwest(_) => true,
        _ => false,
    }
}

fn ensure_safe_download_url(url: &str, allowed_host: Option<&str>) -> Result<(), KieError> {
    let parsed = url::Url::parse(url).map_err(|_| KieError::InvalidResponse {
        message: "download URL must be a valid http or https URL".to_string(),
    })?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => Err(KieError::InvalidResponse {
            message: format!("download URL must use http or https, got {scheme}"),
        })?,
    }
    let host = parsed.host_str().ok_or_else(|| KieError::InvalidResponse {
        message: "download URL must include a host".to_string(),
    })?;
    let host = normalize_host(host);
    if allowed_host
        .map(normalize_host)
        .is_some_and(|allowed| allowed == host)
    {
        return Ok(());
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(KieError::InvalidResponse {
            message: "download URL host is local/private and is not allowed".to_string(),
        });
    }
    if let Ok(addr) = host.parse::<IpAddr>()
        && is_private_or_local_ip(addr)
    {
        return Err(KieError::InvalidResponse {
            message: "download URL host is local/private and is not allowed".to_string(),
        });
    }
    Ok(())
}

fn api_base_host(api_base: &str) -> Option<String> {
    url::Url::parse(api_base)
        .ok()
        .and_then(|url| url.host_str().map(normalize_host))
}

fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

fn is_private_or_local_ip(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(addr) => is_private_or_local_ipv4(addr),
        IpAddr::V6(addr) => is_private_or_local_ipv6(addr),
    }
}

fn is_private_or_local_ipv4(addr: Ipv4Addr) -> bool {
    let octets = addr.octets();
    addr.is_private()
        || addr.is_loopback()
        || addr.is_link_local()
        || addr.is_broadcast()
        || addr.is_unspecified()
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
}

fn is_private_or_local_ipv6(addr: Ipv6Addr) -> bool {
    addr.is_loopback()
        || addr.is_unspecified()
        || addr.is_unique_local()
        || addr.is_unicast_link_local()
}

async fn canonicalize_local_input(path: &Path) -> Result<PathBuf, KieError> {
    tokio::fs::canonicalize(path).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            KieError::MissingLocalInput {
                path: path.display().to_string(),
            }
        } else {
            KieError::Io(err)
        }
    })
}

async fn canonicalize_input_root(path: &Path) -> Result<PathBuf, KieError> {
    tokio::fs::canonicalize(path)
        .await
        .map_err(|err| KieError::InvalidRequest {
            message: format!(
                "configured KIE_MCP_INPUT_ROOTS entry is not accessible: {} ({err})",
                path.display()
            ),
        })
}

fn supported_upload_mime(path: &Path) -> Result<String, KieError> {
    let Some(mime) = mime_guess::from_path(path).first() else {
        return Err(KieError::InvalidLocalInput {
            path: path.display().to_string(),
            message: "file extension is not recognized as image or video media".to_string(),
        });
    };
    let mime = mime.essence_str();
    if mime.starts_with("image/") || mime.starts_with("video/") {
        Ok(mime.to_string())
    } else {
        Err(KieError::InvalidLocalInput {
            path: path.display().to_string(),
            message: format!("unsupported media type {mime}; expected image/* or video/*"),
        })
    }
}

fn output_dir_name(output_name: Option<&str>, task_id: &str) -> String {
    let task_stem = safe_stem(Some(task_id), "task");
    let base = safe_stem(output_name, &task_stem);
    if output_name.is_some() && base != task_stem {
        let short_task = task_stem.chars().take(12).collect::<String>();
        format!("{base}-{short_task}")
    } else {
        task_stem
    }
}

async fn parse_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, KieError> {
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        return Err(KieError::ApiCode {
            code: 401,
            message: "unauthorized".to_string(),
        });
    }
    if !status.is_success() {
        return Err(KieError::HttpStatus {
            status: status.as_u16(),
            body: redact(&response.text().await.unwrap_or_default()),
        });
    }
    Ok(response.json::<T>().await?)
}

pub fn redact(message: &str) -> String {
    if message.contains("X-Amz-")
        || message.contains("X-Goog-")
        || message.contains("Signature=")
        || message.contains("AWS4-HMAC")
    {
        return "[REDACTED_SIGNED_URL]".to_string();
    }

    redact_bearer_tokens(
        &message
            .replace("KIE_API_KEY", "[REDACTED_ENV]")
            .replace("Authorization", "[REDACTED_HEADER]"),
    )
}

fn redact_bearer_tokens(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(index) = rest.find("Bearer ") {
        let (head, tail) = rest.split_at(index);
        out.push_str(head);
        out.push_str("Bearer [REDACTED]");
        let after = &tail["Bearer ".len()..];
        let end = after
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | '}'))
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{KieClient, ensure_safe_download_url, redact};
    use crate::kie::jobs::{GenerationKind, GenerationResult, TaskRecord, TaskState};

    #[test]
    fn redacts_bearer_tokens_and_signed_urls() {
        assert_eq!(
            redact("Authorization: Bearer abc KIE_API_KEY"),
            "[REDACTED_HEADER]: Bearer [REDACTED] [REDACTED_ENV]"
        );
        assert_eq!(
            redact("https://example.com/file.png?X-Amz-Signature=abc"),
            "[REDACTED_SIGNED_URL]"
        );
    }

    #[test]
    fn download_urls_must_be_http_or_https() {
        assert!(ensure_safe_download_url("https://example.com/image.png", None).is_ok());
        assert!(ensure_safe_download_url("http://example.com/image.png", None).is_ok());

        let err = ensure_safe_download_url("file:///tmp/image.png", None).unwrap_err();
        assert!(err.to_string().contains("http or https"));

        let err = ensure_safe_download_url("not a url", None).unwrap_err();
        assert!(err.to_string().contains("valid http or https URL"));
    }

    #[test]
    fn download_urls_reject_private_or_local_hosts() {
        for url in [
            "http://localhost/image.png",
            "http://127.0.0.1/image.png",
            "http://10.0.0.1/image.png",
            "http://172.16.0.1/image.png",
            "http://192.168.1.10/image.png",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/image.png",
        ] {
            let err = ensure_safe_download_url(url, None).unwrap_err();
            assert!(err.to_string().contains("local/private"), "{url}");
        }

        assert!(ensure_safe_download_url("http://127.0.0.1/image.png", Some("127.0.0.1")).is_ok());
    }

    #[test]
    fn generation_result_json_omits_raw_record() {
        let record = TaskRecord {
            task_id: "task_1".to_string(),
            model: "model".to_string(),
            state: TaskState::Success,
            param_json: "{\"prompt\":\"secret\"}".to_string(),
            result_json: "{}".to_string(),
            fail_code: String::new(),
            fail_msg: String::new(),
            cost_time: None,
            complete_time: None,
            create_time: None,
            update_time: None,
            progress: None,
            credits_consumed: None,
        };
        let value = KieClient::result_to_json(&GenerationResult {
            task_id: record.task_id.clone(),
            model: record.model.clone(),
            state: record.state.clone(),
            media_type: GenerationKind::Image,
            media: Vec::new(),
            source_urls: Vec::new(),
            poster_urls: Vec::new(),
            markdown: String::new(),
        });

        assert_eq!(value["task_id"], json!("task_1"));
        assert!(value.get("record").is_none());
    }
}
