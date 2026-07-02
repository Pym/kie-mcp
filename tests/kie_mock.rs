use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use kie_mcp::{
    config::Config,
    kie::{
        KieClient, KieError,
        client::redact,
        jobs::{GenerationKind, GenerationRequest},
    },
};
use serde_json::{Value, json};
use tempfile::TempDir;

#[derive(Clone)]
struct MockState {
    create_payloads: Arc<Mutex<Vec<Value>>>,
    credit_code: Arc<AtomicUsize>,
    final_state: Arc<Mutex<String>>,
    record_failures: Arc<AtomicUsize>,
    result_json: Arc<Mutex<Option<String>>>,
    upload_count: Arc<AtomicUsize>,
    record_count: Arc<AtomicUsize>,
}

#[tokio::test]
async fn image_generation_uses_modern_job_route_and_downloads_media() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let result = client
        .generate_and_wait(
            GenerationRequest {
                model: "gpt-image-2-image-to-image".to_string(),
                prompt: "make it cinematic".to_string(),
                input_urls: vec![format!("{}/input.png", server.base_url)],
                local_input_paths: Vec::new(),
                input: json!({ "aspect_ratio": "1:1" }),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: Some("hero".to_string()),
            },
            GenerationKind::Image,
        )
        .await
        .unwrap();

    assert_eq!(result.task_id, "task_mock");
    assert!(result.markdown.contains("![image]("));
    assert_eq!(result.media.len(), 1);
    assert!(result.media[0].path.exists());
    assert_eq!(
        result.media[0]
            .path
            .parent()
            .and_then(|path| path.file_name())
            .and_then(|value| value.to_str()),
        Some("hero-task_mock")
    );
    assert_eq!(
        tokio::fs::read(&result.media[0].path).await.unwrap(),
        b"image-bytes"
    );

    let payloads = server.state.create_payloads.lock().unwrap();
    let payload = &payloads[0];
    assert_eq!(payload["model"], "gpt-image-2-image-to-image");
    assert_eq!(payload["input"]["prompt"], "make it cinematic");
    assert_eq!(payload["input"]["aspect_ratio"], "1:1");
    assert_eq!(
        payload["input"]["input_urls"][0],
        format!("{}/input.png", server.base_url)
    );
    assert!(server.state.record_count.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn image_generation_resolves_human_catalog_alias_and_convenience_input() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let result = client
        .generate_and_wait(
            GenerationRequest {
                model: "Nano Banana 2".to_string(),
                prompt: "chatons photographiques".to_string(),
                input_urls: vec![format!("{}/input.png", server.base_url)],
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: Some("4:3".to_string()),
                resolution: Some("1K".to_string()),
                output_format: Some("jpg".to_string()),
                output_name: Some("kitten".to_string()),
            },
            GenerationKind::Image,
        )
        .await
        .unwrap();

    assert_eq!(result.task_id, "task_mock");
    assert!(result.media[0].path.exists());

    let payloads = server.state.create_payloads.lock().unwrap();
    let payload = payloads.last().unwrap();
    assert_eq!(payload["model"], "nano-banana-2");
    assert_eq!(payload["input"]["prompt"], "chatons photographiques");
    assert_eq!(payload["input"]["aspect_ratio"], "4:3");
    assert_eq!(payload["input"]["resolution"], "1K");
    assert_eq!(payload["input"]["output_format"], "jpg");
    assert_eq!(
        payload["input"]["image_input"][0],
        format!("{}/input.png", server.base_url)
    );
}

#[tokio::test]
async fn local_inputs_are_uploaded_before_task_creation() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let client = client_for(&server, temp.path().join("out"));

    client
        .generate_and_wait(
            GenerationRequest {
                model: "gpt-image-2-image-to-image".to_string(),
                prompt: "edit this".to_string(),
                input_urls: Vec::new(),
                local_input_paths: vec![local],
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap();

    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 1);
    let payloads = server.state.create_payloads.lock().unwrap();
    assert_eq!(
        payloads[0]["input"]["input_urls"][0],
        format!("{}/uploaded/input.png", server.base_url)
    );
}

#[tokio::test]
async fn local_and_remote_inputs_are_merged_for_array_binding() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let client = client_for(&server, temp.path().join("out"));

    client
        .create_task(
            &GenerationRequest {
                model: "gpt-image-2-image-to-image".to_string(),
                prompt: "edit this".to_string(),
                input_urls: vec![format!("{}/remote.png", server.base_url)],
                local_input_paths: vec![local],
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap();

    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 1);
    let payloads = server.state.create_payloads.lock().unwrap();
    assert_eq!(
        payloads[0]["input"]["input_urls"],
        json!([
            format!("{}/remote.png", server.base_url),
            format!("{}/uploaded/input.png", server.base_url)
        ])
    );
}

#[tokio::test]
async fn explicit_media_input_rejects_top_level_media_shortcuts() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .create_task(
            &GenerationRequest {
                model: "gpt-image-2-image-to-image".to_string(),
                prompt: "edit this".to_string(),
                input_urls: vec![format!("{}/other.png", server.base_url)],
                local_input_paths: vec![local],
                input: json!({ "input_urls": [format!("{}/input.png", server.base_url)] }),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, KieError::InvalidRequest { .. }));
    assert!(err.to_string().contains("either input media fields"));
    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 0);
    assert!(server.state.create_payloads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn scalar_media_binding_rejects_multiple_urls() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .create_task(
            &GenerationRequest {
                model: "topaz/image-upscale".to_string(),
                prompt: "upscale this".to_string(),
                input_urls: vec![
                    format!("{}/first.png", server.base_url),
                    format!("{}/second.png", server.base_url),
                ],
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, KieError::InvalidRequest { .. }));
    assert!(err.to_string().contains("accepts exactly one input URL"));
    assert!(server.state.create_payloads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn invalid_input_url_is_rejected_before_local_upload() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .create_task(
            &GenerationRequest {
                model: "nano-banana-2".to_string(),
                prompt: "reject invalid URL".to_string(),
                input_urls: vec!["file:///tmp/input.png".to_string()],
                local_input_paths: vec![local],
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, KieError::InvalidRequest { .. }));
    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 0);
    assert!(server.state.create_payloads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn upload_cache_reuses_same_local_file_in_session() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let client = client_for(&server, temp.path().join("out"));

    for prompt in ["first edit", "second edit"] {
        client
            .create_task(
                &GenerationRequest {
                    model: "gpt-image-2-image-to-image".to_string(),
                    prompt: prompt.to_string(),
                    input_urls: Vec::new(),
                    local_input_paths: vec![local.clone()],
                    input: json!({}),
                    aspect_ratio: None,
                    resolution: None,
                    output_format: None,
                    output_name: None,
                },
                GenerationKind::Image,
            )
            .await
            .unwrap();
    }

    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 1);
    let payloads = server.state.create_payloads.lock().unwrap();
    assert_eq!(payloads.len(), 2);
    assert_eq!(
        payloads[0]["input"]["input_urls"][0],
        format!("{}/uploaded/input.png", server.base_url)
    );
    assert_eq!(
        payloads[1]["input"]["input_urls"][0],
        format!("{}/uploaded/input.png", server.base_url)
    );
}

#[tokio::test]
async fn concurrent_uploads_share_same_local_file_upload() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let (first, second) = tokio::join!(client.upload_file(&local), client.upload_file(&local));
    let first = first.unwrap();
    let second = second.unwrap();

    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 1);
    assert_eq!(first.url, second.url);
}

#[tokio::test]
async fn local_upload_rejects_directories_before_api_call() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client.upload_file(temp.path()).await.unwrap_err();

    assert!(matches!(err, KieError::InvalidLocalInput { .. }));
    assert!(err.to_string().contains("regular file"));
    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_upload_rejects_non_media_files_before_api_call() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("notes.txt");
    tokio::fs::write(&local, b"not-media").await.unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client.upload_file(&local).await.unwrap_err();

    assert!(matches!(err, KieError::InvalidLocalInput { .. }));
    assert!(err.to_string().contains("unsupported media type"));
    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_upload_rejects_files_over_configured_limit() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let mut config = config_for(&server, temp.path().join("out"));
    config.max_upload_bytes = 4;
    let client = KieClient::new(config);

    let err = client.upload_file(&local).await.unwrap_err();

    assert!(matches!(err, KieError::LocalInputTooLarge { .. }));
    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn local_upload_rejects_paths_outside_configured_roots() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let allowed = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let mut config = config_for(&server, temp.path().join("out"));
    config.input_roots = vec![allowed.path().to_path_buf()];
    let client = KieClient::new(config);

    let err = client.upload_file(&local).await.unwrap_err();

    assert!(matches!(err, KieError::InvalidLocalInput { .. }));
    assert!(err.to_string().contains("outside configured"));
    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn video_generation_downloads_mp4_and_poster() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let result = client
        .generate_and_wait(
            GenerationRequest {
                model: "wan/2-7-text-to-video".to_string(),
                prompt: "camera pushes forward".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({ "duration": 5, "ratio": "16:9" }),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: Some("clip".to_string()),
            },
            GenerationKind::Video,
        )
        .await
        .unwrap();

    assert!(result.markdown.contains("[video]("));
    assert!(result.markdown.contains("![poster]("));
    assert_eq!(result.media[0].kind, "video");
    assert_eq!(
        tokio::fs::read(&result.media[0].path).await.unwrap(),
        b"video-bytes"
    );
}

#[tokio::test]
async fn generation_rejects_non_http_download_urls() {
    let server =
        MockServer::start_with_result_json(r#"{"resultUrls":["file:///tmp/generated.png"]}"#).await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .generate_and_wait(
            GenerationRequest {
                model: "gpt-image-2-text-to-image".to_string(),
                prompt: "reject bad url".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, KieError::InvalidResponse { .. }));
    assert!(err.to_string().contains("http or https"));
}

#[tokio::test]
async fn generation_rejects_private_download_urls_before_resolver() {
    let server =
        MockServer::start_with_result_json(r#"{"resultUrls":["http://127.0.0.1/generated.png"]}"#)
            .await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .generate_and_wait(
            GenerationRequest {
                model: "gpt-image-2-text-to-image".to_string(),
                prompt: "reject private url".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, KieError::InvalidResponse { .. }));
    assert!(err.to_string().contains("local/private"));
}

#[tokio::test]
async fn generation_does_not_fallback_when_resolver_rejects_url() {
    let server = MockServer::start_with_result_json(
        r#"{"resultUrls":["https://kie.example/resolver-400.png"]}"#,
    )
    .await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .generate_and_wait(
            GenerationRequest {
                model: "gpt-image-2-text-to-image".to_string(),
                prompt: "reject resolver error".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, KieError::ApiCode { code: 400, .. }));
}

#[tokio::test]
async fn generation_retries_transient_record_info_errors() {
    let server = MockServer::start_with_record_failures(1).await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let result = client
        .generate_and_wait(
            GenerationRequest {
                model: "gpt-image-2-text-to-image".to_string(),
                prompt: "retry polling".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap();

    assert_eq!(result.task_id, "task_mock");
    assert!(server.state.record_count.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn unknown_task_state_is_preserved_in_polling_error() {
    let server = MockServer::start_with_final_state("PROCESSING_V2").await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .generate_and_wait(
            GenerationRequest {
                model: "gpt-image-2-text-to-image".to_string(),
                prompt: "unknown state".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        KieError::UnexpectedTaskState { ref state, .. } if state == "PROCESSING_V2"
    ));
}

#[tokio::test]
async fn credits_api_code_errors_are_reported() {
    let server = MockServer::start_with_credit_code(402).await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client.credits().await.unwrap_err();

    assert!(matches!(err, KieError::ApiCode { code: 402, .. }));
}

#[tokio::test]
async fn missing_api_key_is_reported_without_secret_material() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let mut config = config_for(&server, temp.path().join("out"));
    config.api_key = None;
    let client = KieClient::new(config);
    let err = client.credits().await.unwrap_err();
    assert!(matches!(err, KieError::MissingApiKey));
    assert_eq!(
        redact("Authorization: Bearer abc KIE_API_KEY"),
        "[REDACTED_HEADER]: Bearer [REDACTED] [REDACTED_ENV]"
    );
}

fn client_for(server: &MockServer, output_dir: PathBuf) -> KieClient {
    KieClient::new(config_for(server, output_dir))
}

fn config_for(server: &MockServer, output_dir: PathBuf) -> Config {
    Config {
        api_key: Some("test-secret".to_string()),
        api_base: server.base_url.clone(),
        upload_base: server.base_url.clone(),
        output_dir,
        timeout: Duration::from_secs(10),
        http_timeout: Duration::from_secs(10),
        max_upload_bytes: kie_mcp::config::DEFAULT_MAX_UPLOAD_BYTES,
        input_roots: Vec::new(),
    }
}

struct MockServer {
    base_url: String,
    state: MockState,
}

impl MockServer {
    async fn start() -> Self {
        Self::start_with_options(0, 200, None).await
    }

    async fn start_with_record_failures(record_failures: usize) -> Self {
        Self::start_with_options(record_failures, 200, None).await
    }

    async fn start_with_credit_code(credit_code: usize) -> Self {
        Self::start_with_options(0, credit_code, None).await
    }

    async fn start_with_result_json(result_json: &str) -> Self {
        Self::start_with_options(0, 200, Some(result_json.to_string())).await
    }

    async fn start_with_final_state(final_state: &str) -> Self {
        let server = Self::start().await;
        *server.state.final_state.lock().unwrap() = final_state.to_string();
        server
    }

    async fn start_with_options(
        record_failures: usize,
        credit_code: usize,
        result_json: Option<String>,
    ) -> Self {
        let state = MockState {
            create_payloads: Arc::new(Mutex::new(Vec::new())),
            credit_code: Arc::new(AtomicUsize::new(credit_code)),
            final_state: Arc::new(Mutex::new("success".to_string())),
            record_failures: Arc::new(AtomicUsize::new(record_failures)),
            result_json: Arc::new(Mutex::new(result_json)),
            upload_count: Arc::new(AtomicUsize::new(0)),
            record_count: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new()
            .route("/api/v1/jobs/createTask", post(create_task))
            .route("/api/v1/jobs/recordInfo", get(record_info))
            .route("/api/v1/common/download-url", post(download_url))
            .route("/api/file-stream-upload", post(upload_file))
            .route("/api/v1/chat/credit", get(credits))
            .route("/media/generated.png", get(media_image))
            .route("/media/generated.mp4", get(media_video))
            .route("/media/poster.png", get(media_poster))
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock Kie server should bind to a local ephemeral port");
        let addr: SocketAddr = listener
            .local_addr()
            .expect("mock Kie server should expose its local address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock Kie server should run");
        });

        Self {
            base_url: format!("http://{addr}"),
            state,
        }
    }
}

async fn create_task(
    State(state): State<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.create_payloads.lock().unwrap().push(payload);
    Json(json!({
        "code": 200,
        "msg": "success",
        "data": { "taskId": "task_mock" }
    }))
}

async fn record_info(
    State(state): State<MockState>,
    Query(_query): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    if state
        .record_failures
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
            (remaining > 0).then(|| remaining - 1)
        })
        .is_ok()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "temporary recordInfo failure",
        )
            .into_response();
    }

    let model = state
        .create_payloads
        .lock()
        .unwrap()
        .last()
        .and_then(|payload| payload["model"].as_str())
        .unwrap_or("gpt-image-2-image-to-image")
        .to_string();
    let count = state.record_count.fetch_add(1, Ordering::SeqCst);
    if count == 0 {
        return Json(json!({
            "code": 200,
            "msg": "success",
            "data": {
                "taskId": "task_mock",
                "model": model,
                "state": "generating",
                "resultJson": "",
                "failCode": "",
                "failMsg": ""
            }
        }))
        .into_response();
    }
    let result_json = state
        .result_json
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| {
            if model.contains("video") {
                "{\"videoInfo\":{\"videoUrl\":\"https://kie.example/video-will-be-rewritten\",\"imageUrl\":\"https://kie.example/poster-will-be-rewritten\"}}".to_string()
            } else {
                "{\"resultUrls\":[\"https://kie.example/will-be-rewritten\"]}".to_string()
            }
        });
    let final_state = state.final_state.lock().unwrap().clone();
    Json(json!({
        "code": 200,
        "msg": "success",
        "data": {
            "taskId": "task_mock",
            "model": model,
            "state": final_state,
            "resultJson": result_json,
            "failCode": "",
            "failMsg": "",
            "creditsConsumed": 1
        }
    }))
    .into_response()
}

async fn credits(State(state): State<MockState>) -> impl IntoResponse {
    let code = state.credit_code.load(Ordering::SeqCst);
    Json(json!({
        "code": code,
        "msg": if code == 200 { "success" } else { "credit error" },
        "data": { "credits": 123 }
    }))
}

async fn download_url(headers: HeaderMap, Json(payload): Json<Value>) -> axum::response::Response {
    let url = payload["url"].as_str().unwrap_or_default();
    if url.contains("resolver-400") {
        return Json(json!({
            "code": 400,
            "msg": "resolver rejected url",
            "data": null
        }))
        .into_response();
    }
    if url.contains("resolver-500") {
        return (StatusCode::INTERNAL_SERVER_ERROR, "resolver unavailable").into_response();
    }
    let path = if url.contains("video") {
        "/media/generated.mp4"
    } else if url.contains("poster") {
        "/media/poster.png"
    } else {
        "/media/generated.png"
    };
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .expect("download-url request should include a valid Host header");
    Json(json!({
        "code": 200,
        "msg": "success",
        "data": format!("http://{host}{path}")
    }))
    .into_response()
}

async fn upload_file(
    State(state): State<MockState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    assert!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .starts_with("Bearer ")
    );
    assert!(!body.is_empty());
    tokio::time::sleep(Duration::from_millis(25)).await;
    state.upload_count.fetch_add(1, Ordering::SeqCst);
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .expect("upload request should include a valid Host header");
    Json(json!({
        "success": true,
        "code": 200,
        "msg": "File upload successful",
        "data": {
            "downloadUrl": format!("http://{host}/uploaded/input.png")
        }
    }))
}

async fn media_image() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/png")],
        b"image-bytes",
    )
}

async fn media_video() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "video/mp4")],
        b"video-bytes",
    )
}

async fn media_poster() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/png")],
        b"poster-bytes",
    )
}
