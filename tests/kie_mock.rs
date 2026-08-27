use std::{
    collections::{HashMap, HashSet},
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
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use kie_mcp::{
    config::Config,
    kie::{
        KieClient, KieError,
        catalog::CatalogStatus,
        client::redact,
        jobs::{GenerationKind, GenerationRequest},
        operations::{
            GROK_SEGMENT_MAP_MODEL, GeminiOmniVoice, OMNIHUMAN_IDENTIFICATION_MODEL,
            OMNIHUMAN_SUBJECT_DETECTION_MODEL, StructuredTaskOutput,
        },
    },
};
use serde_json::{Value, json};
use tempfile::TempDir;

#[derive(Clone)]
struct MockState {
    create_payloads: Arc<Mutex<Vec<Value>>>,
    credit_code: Arc<AtomicUsize>,
    final_state: Arc<Mutex<String>>,
    media_download_count: Arc<AtomicUsize>,
    next_task: Arc<AtomicUsize>,
    omni_audio_code: Arc<AtomicUsize>,
    omni_audio_payloads: Arc<Mutex<Vec<Value>>>,
    omni_character_payloads: Arc<Mutex<Vec<Value>>>,
    record_failures: Arc<AtomicUsize>,
    result_json: Arc<Mutex<Option<String>>>,
    task_models: Arc<Mutex<HashMap<String, String>>>,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_generations_create_distinct_tasks_and_download_without_collisions() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));
    let request = |index| GenerationRequest {
        model: "gpt-image-2-text-to-image".to_string(),
        prompt: format!("parallel image {index}"),
        input_urls: Vec::new(),
        local_input_paths: Vec::new(),
        input: json!({}),
        aspect_ratio: None,
        resolution: None,
        output_format: None,
        output_name: Some("parallel-output".to_string()),
    };

    let (first, second, third) = tokio::join!(
        client.generate_and_wait(request(1), GenerationKind::Image),
        client.generate_and_wait(request(2), GenerationKind::Image),
        client.generate_and_wait(request(3), GenerationKind::Image),
    );
    let results = [first.unwrap(), second.unwrap(), third.unwrap()];

    let task_ids = results
        .iter()
        .map(|result| result.task_id.clone())
        .collect::<HashSet<_>>();
    assert_eq!(task_ids.len(), 3, "each generation must own one Kie task");
    assert_eq!(server.state.create_payloads.lock().unwrap().len(), 3);
    assert_eq!(server.state.media_download_count.load(Ordering::SeqCst), 3);

    let media_paths = results
        .iter()
        .map(|result| {
            assert_eq!(result.media.len(), 1);
            result.media[0].path.clone()
        })
        .collect::<HashSet<_>>();
    assert_eq!(
        media_paths.len(),
        3,
        "parallel downloads must not overwrite each other"
    );
    assert!(media_paths.iter().all(|path| path.exists()));

    let source_urls = results
        .iter()
        .flat_map(|result| result.source_urls.iter().cloned())
        .collect::<HashSet<_>>();
    assert_eq!(source_urls.len(), 3);

    let mut media_contents = HashSet::new();
    for path in media_paths {
        media_contents.insert(tokio::fs::read(path).await.unwrap());
    }
    assert_eq!(
        media_contents.len(),
        3,
        "each mocked task must download its own media"
    );
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
async fn live_catalog_exposes_recursive_model_constraints() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let listing = client
        .catalog()
        .models(Some(GenerationKind::Image), Some("Nano Banana 2"), false)
        .await;

    assert!(matches!(listing.status, CatalogStatus::Live));
    assert_eq!(listing.schema_failures, 0);
    assert_eq!(listing.models.len(), 1);
    let model = serde_json::to_value(&listing.models[0]).unwrap();
    assert_eq!(model["id"], "nano-banana-2");
    assert_eq!(model["catalog_source"], "live_openapi");
    assert_eq!(model["schema_status"], "authoritative");
    assert_eq!(model["input_schema"]["required"], json!(["prompt"]));
    assert_eq!(
        model["input_schema"]["properties"]["aspect_ratio"]["enum"],
        json!(["1:1", "4:3", "16:9", "auto"])
    );
    assert_eq!(
        model["input_schema"]["properties"]["image_input"]["maxItems"],
        14
    );
    assert!(
        model["input_schema"]["properties"]["prompt"]
            .get("description")
            .is_none()
    );
}

#[tokio::test]
async fn live_catalog_rejects_invalid_enum_before_task_creation() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let error = client
        .create_task(
            &GenerationRequest {
                model: "Nano Banana 2".to_string(),
                prompt: "test image".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: None,
                resolution: Some("8K".to_string()),
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("input.resolution must be one of")
    );
    assert!(server.state.create_payloads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn live_catalog_rejects_invalid_nested_field_before_task_creation() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let error = client
        .create_task(
            &GenerationRequest {
                model: "kling-3.0/video".to_string(),
                prompt: "test video".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({
                    "mode": "pro",
                    "multi_prompt": [{ "prompt": "first shot", "duration": 16 }]
                }),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Video,
        )
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("input.multi_prompt[0].duration must be at most 15")
    );
    assert!(server.state.create_payloads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn live_catalog_rejects_missing_required_field_before_task_creation() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let error = client
        .create_task(
            &GenerationRequest {
                model: "kling-3.0/video".to_string(),
                prompt: "test video".to_string(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({}),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Video,
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("input.mode is required"));
    assert!(server.state.create_payloads.lock().unwrap().is_empty());
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
async fn uncataloged_model_shortcuts_are_rejected_before_local_upload() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("input.png");
    tokio::fs::write(&local, b"local-image").await.unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let error = client
        .create_task(
            &GenerationRequest {
                model: "future-image-to-video".to_string(),
                prompt: String::new(),
                input_urls: Vec::new(),
                local_input_paths: vec![local],
                input: json!({ "prompt": "animate this image" }),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Video,
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("top-level local_input_paths"));
    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 0);
    assert!(server.state.create_payloads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn promptless_model_does_not_send_legacy_prompt() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    client
        .create_task(
            &GenerationRequest {
                model: "topaz/image-upscale".to_string(),
                prompt: "legacy required prompt".to_string(),
                input_urls: vec![format!("{}/input.png", server.base_url)],
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

    let payloads = server.state.create_payloads.lock().unwrap();
    assert_eq!(
        payloads[0]["input"]["image_url"],
        format!("{}/input.png", server.base_url)
    );
    assert!(payloads[0]["input"].get("prompt").is_none());
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

#[tokio::test]
async fn gemini_omni_profiles_use_their_dedicated_routes() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let audio = client
        .create_gemini_omni_audio_profile(
            GeminiOmniVoice::Achernar,
            "Narrator",
            Some("Calm and clear"),
            Some("Hello there"),
        )
        .await
        .unwrap();
    assert_eq!(audio.audio_id, "audio_mock");

    let character = client
        .create_gemini_omni_character(
            "A silver-haired explorer",
            Some("https://example.com/character.png"),
            None,
            std::slice::from_ref(&audio.audio_id),
            Some("Nova"),
        )
        .await
        .unwrap();
    assert_eq!(character.character_id, "character_mock");

    let audio_payloads = server.state.omni_audio_payloads.lock().unwrap();
    assert_eq!(audio_payloads[0]["audio_id"], "achernar");
    assert_eq!(audio_payloads[0]["name"], "Narrator");
    drop(audio_payloads);

    let character_payloads = server.state.omni_character_payloads.lock().unwrap();
    assert_eq!(
        character_payloads[0]["descriptions"],
        "A silver-haired explorer"
    );
    assert!(character_payloads[0].get("description").is_none());
    assert_eq!(
        character_payloads[0]["image_urls"][0],
        "https://example.com/character.png"
    );
    assert_eq!(character_payloads[0]["audio_ids"][0], "audio_mock");
}

#[tokio::test]
async fn gemini_omni_audio_rejects_nonzero_api_codes_and_long_fields() {
    let server = MockServer::start_with_omni_audio_code(400).await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .create_gemini_omni_audio_profile(GeminiOmniVoice::Puck, "Narrator", None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, KieError::ApiCode { code: 400, .. }));

    let err = client
        .create_gemini_omni_audio_profile(GeminiOmniVoice::Puck, &"n".repeat(211), None, None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("210 characters"));
}

#[tokio::test]
async fn grok_segment_map_returns_indexes_urls_and_local_previews() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let result = client
        .grok_segment_map(Some("source_task"), None, None, Some("grok-segments"))
        .await
        .unwrap();

    assert_eq!(result.model, GROK_SEGMENT_MAP_MODEL);
    let StructuredTaskOutput::GrokSegmentMap {
        segments_count,
        segments,
    } = result.output
    else {
        panic!("expected Grok segment output");
    };
    assert_eq!(segments_count, 1);
    assert_eq!(segments[0].index, 0);
    assert_eq!(segments[0].name, "dog");
    assert!(segments[0].local_path.as_ref().unwrap().exists());

    let payloads = server.state.create_payloads.lock().unwrap();
    assert_eq!(payloads[0]["model"], GROK_SEGMENT_MAP_MODEL);
    assert_eq!(payloads[0]["input"]["task_id"], "source_task");
}

#[tokio::test]
async fn omnihuman_operations_return_raw_status_and_mask_previews() {
    let identification_server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let identification_client = client_for(&identification_server, temp.path().join("identify"));
    let identification = identification_client
        .omnihuman_identification(Some("https://example.com/portrait.png"), None)
        .await
        .unwrap();
    assert_eq!(identification.model, OMNIHUMAN_IDENTIFICATION_MODEL);
    assert!(matches!(
        identification.output,
        StructuredTaskOutput::OmnihumanIdentification { subject_status: 1 }
    ));

    let detection_server = MockServer::start().await;
    let detection_client = client_for(&detection_server, temp.path().join("detect"));
    let detection = detection_client
        .omnihuman_subject_detection(
            Some("https://example.com/group.png"),
            None,
            Some("subjects"),
        )
        .await
        .unwrap();
    assert_eq!(detection.model, OMNIHUMAN_SUBJECT_DETECTION_MODEL);
    let StructuredTaskOutput::OmnihumanSubjectDetection { masks } = detection.output else {
        panic!("expected OmniHuman subject masks");
    };
    assert_eq!(masks.len(), 2);
    assert!(
        masks
            .iter()
            .all(|mask| mask.local_path.as_ref().is_some_and(|path| path.exists()))
    );
}

#[tokio::test]
async fn structured_operations_validate_sources_and_stay_out_of_media_generation() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let err = client
        .grok_segment_map(
            Some("task_1"),
            Some("https://example.com/image.png"),
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("exactly one"));

    let err = client
        .create_task(
            &GenerationRequest {
                model: GROK_SEGMENT_MAP_MODEL.to_string(),
                prompt: String::new(),
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: json!({ "task_id": "task_1" }),
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            },
            GenerationKind::Image,
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("kie_grok_image_2_segment_map"));

    let err = client
        .omnihuman_identification(Some("file:///tmp/portrait.png"), None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("valid http or https URL"));
}

#[tokio::test]
async fn omnihuman_local_images_enforce_image_type_and_endpoint_size() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let client = client_for(&server, temp.path().join("out"));

    let video = temp.path().join("portrait.mp4");
    tokio::fs::write(&video, b"not-an-image").await.unwrap();
    let err = client
        .omnihuman_identification(None, Some(&video))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("must be an image file"));

    let webp = temp.path().join("portrait.webp");
    tokio::fs::write(&webp, b"webp").await.unwrap();
    let err = client
        .omnihuman_identification(None, Some(&webp))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("expected JPEG or PNG"));

    let large_image = temp.path().join("large.png");
    tokio::fs::write(&large_image, vec![0; 5 * 1024 * 1024 + 1])
        .await
        .unwrap();
    let err = client
        .omnihuman_identification(None, Some(&large_image))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        KieError::LocalInputTooLarge {
            limit: 5_242_880,
            ..
        }
    ));
    assert_eq!(server.state.upload_count.load(Ordering::SeqCst), 0);
}

fn client_for(server: &MockServer, output_dir: PathBuf) -> KieClient {
    KieClient::new(config_for(server, output_dir))
}

fn config_for(server: &MockServer, output_dir: PathBuf) -> Config {
    Config {
        api_key: Some("test-secret".to_string()),
        api_base: server.base_url.clone(),
        upload_base: server.base_url.clone(),
        catalog_url: format!("{}/catalog/llms.txt", server.base_url),
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

    async fn start_with_omni_audio_code(code: usize) -> Self {
        let server = Self::start().await;
        server.state.omni_audio_code.store(code, Ordering::SeqCst);
        server
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
            media_download_count: Arc::new(AtomicUsize::new(0)),
            next_task: Arc::new(AtomicUsize::new(0)),
            omni_audio_code: Arc::new(AtomicUsize::new(0)),
            omni_audio_payloads: Arc::new(Mutex::new(Vec::new())),
            omni_character_payloads: Arc::new(Mutex::new(Vec::new())),
            record_failures: Arc::new(AtomicUsize::new(record_failures)),
            result_json: Arc::new(Mutex::new(result_json)),
            task_models: Arc::new(Mutex::new(HashMap::new())),
            upload_count: Arc::new(AtomicUsize::new(0)),
            record_count: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new()
            .route("/catalog/llms.txt", get(catalog_index))
            .route("/market/google/nanobanana2.md", get(nano_banana_contract))
            .route("/market/kling/kling-3-0.md", get(kling_contract))
            .route("/api/v1/jobs/createTask", post(create_task))
            .route("/api/v1/jobs/recordInfo", get(record_info))
            .route("/api/v1/omni/audio/create", post(create_omni_audio))
            .route("/api/v1/omni/character/create", post(create_omni_character))
            .route("/api/v1/common/download-url", post(download_url))
            .route("/api/file-stream-upload", post(upload_file))
            .route("/api/v1/chat/credit", get(credits))
            .route("/media/generated.png", get(media_image))
            .route("/media/generated.mp4", get(media_video))
            .route("/media/poster.png", get(media_poster))
            .route("/media/{file}", get(media_dynamic))
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

async fn catalog_index() -> &'static str {
    include_str!("fixtures/kie_catalog_index.txt")
}

async fn nano_banana_contract() -> &'static str {
    include_str!("fixtures/kie_nano_banana_route.md")
}

async fn kling_contract() -> &'static str {
    include_str!("fixtures/kie_kling_route.md")
}

async fn create_task(
    State(state): State<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let sequence = state.next_task.fetch_add(1, Ordering::SeqCst);
    let task_id = if sequence == 0 {
        "task_mock".to_string()
    } else {
        format!("task_mock_{}", sequence + 1)
    };
    let model = payload["model"]
        .as_str()
        .unwrap_or("gpt-image-2-image-to-image")
        .to_string();
    state
        .task_models
        .lock()
        .unwrap()
        .insert(task_id.clone(), model);
    state.create_payloads.lock().unwrap().push(payload);
    Json(json!({
        "code": 200,
        "msg": "success",
        "data": { "taskId": task_id }
    }))
}

async fn create_omni_audio(
    State(state): State<MockState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    assert_bearer(&headers);
    state.omni_audio_payloads.lock().unwrap().push(payload);
    let code = state.omni_audio_code.load(Ordering::SeqCst);
    Json(json!({
        "code": code,
        "msg": if code == 0 { "success" } else { "audio profile error" },
        "data": { "kieAudioId": "audio_mock", "name": "Narrator" }
    }))
}

async fn create_omni_character(
    State(state): State<MockState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    assert_bearer(&headers);
    state.omni_character_payloads.lock().unwrap().push(payload);
    Json(json!({
        "code": 200,
        "msg": "success",
        "data": {
            "characterId": "character_mock",
            "characterName": "Nova",
            "imageUrl": "https://example.com/character-result.png"
        }
    }))
}

async fn record_info(
    State(state): State<MockState>,
    Query(query): Query<HashMap<String, String>>,
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

    let task_id = query
        .get("taskId")
        .cloned()
        .unwrap_or_else(|| "task_mock".to_string());
    let model = state
        .task_models
        .lock()
        .unwrap()
        .get(&task_id)
        .cloned()
        .unwrap_or_else(|| "gpt-image-2-image-to-image".to_string());
    let count = state.record_count.fetch_add(1, Ordering::SeqCst);
    if count == 0 {
        return Json(json!({
            "code": 200,
            "msg": "success",
            "data": {
                "taskId": task_id,
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
            if model == GROK_SEGMENT_MAP_MODEL {
                r#"{"resultObject":{"segments_count":1,"segments":[{"maskUrl":"https://kie.example/mask-0.png","name":"dog","index":0}]}}"#.to_string()
            } else if model == OMNIHUMAN_IDENTIFICATION_MODEL {
                r#"{"resultObject":{"subject_status":1}}"#.to_string()
            } else if model == OMNIHUMAN_SUBJECT_DETECTION_MODEL {
                r#"{"resultObject":{"mask_urls":["https://kie.example/mask-0.png","https://kie.example/mask-1.png"]}}"#.to_string()
            } else if model.contains("video") {
                "{\"videoInfo\":{\"videoUrl\":\"https://kie.example/video-will-be-rewritten\",\"imageUrl\":\"https://kie.example/poster-will-be-rewritten\"}}".to_string()
            } else if task_id == "task_mock" {
                "{\"resultUrls\":[\"https://kie.example/will-be-rewritten\"]}".to_string()
            } else {
                format!("{{\"resultUrls\":[\"https://kie.example/{task_id}.png\"]}}")
            }
        });
    let final_state = state.final_state.lock().unwrap().clone();
    Json(json!({
        "code": 200,
        "msg": "success",
        "data": {
            "taskId": task_id,
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
        "/media/generated.mp4".to_string()
    } else if url.contains("poster") {
        "/media/poster.png".to_string()
    } else if let Some(file) = url
        .rsplit('/')
        .next()
        .filter(|file| file.starts_with("task_mock"))
    {
        format!("/media/{file}")
    } else {
        "/media/generated.png".to_string()
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

fn assert_bearer(headers: &HeaderMap) {
    assert!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .starts_with("Bearer ")
    );
}

async fn media_image(State(state): State<MockState>) -> impl IntoResponse {
    state.media_download_count.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/png")],
        b"image-bytes",
    )
}

async fn media_video(State(state): State<MockState>) -> impl IntoResponse {
    state.media_download_count.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "video/mp4")],
        b"video-bytes",
    )
}

async fn media_poster(State(state): State<MockState>) -> impl IntoResponse {
    state.media_download_count.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/png")],
        b"poster-bytes",
    )
}

async fn media_dynamic(
    State(state): State<MockState>,
    AxumPath(file): AxumPath<String>,
) -> impl IntoResponse {
    state.media_download_count.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/png")],
        format!("image-bytes-{file}"),
    )
}
