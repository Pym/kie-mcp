use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    time::{Duration, timeout},
};

#[tokio::test]
async fn stdio_server_lists_kie_tools() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kie-mcp"));
    command
        .arg("serve")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    for name in [
        "KIE_API_KEY",
        "KIE_MCP_API_BASE",
        "KIE_MCP_UPLOAD_BASE",
        "KIE_MCP_CATALOG_URL",
        "KIE_MCP_OUTPUT_DIR",
        "KIE_MCP_TIMEOUT_SECS",
        "KIE_MCP_HTTP_TIMEOUT_SECS",
        "KIE_MCP_MAX_UPLOAD_BYTES",
        "KIE_MCP_INPUT_ROOTS",
    ] {
        command.env_remove(name);
    }
    let mut child = command
        .spawn()
        .expect("kie-mcp test binary should spawn in serve mode");

    let mut stdin = child.stdin.take().expect("child stdin should be piped");
    let stdout = child.stdout.take().expect("child stdout should be piped");
    let mut reader = BufReader::new(stdout).lines();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "kie-mcp-test", "version": "0.0.0" }
            }
        }),
    )
    .await;
    let init = read_response(&mut reader).await;
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["serverInfo"]["name"], "kie-mcp");
    assert_eq!(
        init["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert!(init["result"]["instructions"].as_str().is_some());

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
    )
    .await;
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    )
    .await;
    let tools = read_response(&mut reader).await;
    assert_eq!(tools["id"], 2);
    let names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .expect("writing JSON-RPC request to child stdin should succeed")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    let expected = [
        "kie_generate_image",
        "kie_generate_video",
        "kie_models",
        "kie_gemini_omni_create_audio_profile",
        "kie_gemini_omni_create_character",
        "kie_grok_image_2_segment_map",
        "kie_omnihuman_human_identification",
        "kie_omnihuman_subject_detection",
        "kie_task_status",
        "kie_upload_media",
        "kie_credits",
    ];
    assert_eq!(names.len(), expected.len());
    for name in expected {
        assert!(names.iter().any(|candidate| candidate == name), "{name}");
    }

    let image_tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "kie_generate_image")
        .unwrap();
    let properties = &image_tool["inputSchema"]["properties"];
    let input_schema = &properties["input"];
    assert_eq!(input_schema["type"], "object");
    let required = image_tool["inputSchema"]["required"]
        .as_array()
        .expect("tool schema should list required fields");
    assert!(required.iter().any(|field| field == "model"));
    assert!(!required.iter().any(|field| field == "prompt"));

    let models_tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "kie_models")
        .unwrap();
    assert_eq!(
        models_tool["inputSchema"]["properties"]["include_descriptions"]["type"],
        "boolean"
    );

    let audio_tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "kie_gemini_omni_create_audio_profile")
        .unwrap();
    let audio_required = audio_tool["inputSchema"]["required"].as_array().unwrap();
    assert!(audio_required.iter().any(|field| field == "preset_voice"));
    assert!(audio_required.iter().any(|field| field == "name"));
    assert_eq!(
        audio_tool["inputSchema"]["properties"]["preset_voice"]["$ref"],
        "#/$defs/GeminiOmniVoice"
    );
    let voices = audio_tool["inputSchema"]["$defs"]["GeminiOmniVoice"]["enum"]
        .as_array()
        .unwrap();
    assert!(voices.iter().any(|voice| voice == "achernar"));

    let segment_tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "kie_grok_image_2_segment_map")
        .unwrap();
    let segment_properties = &segment_tool["inputSchema"]["properties"];
    assert!(segment_properties.get("task_id").is_some());
    assert!(segment_properties.get("image_url").is_some());
    assert!(segment_properties.get("local_image_path").is_some());
    let segment_description = segment_tool["description"].as_str().unwrap();
    assert!(
        segment_description.contains("grok-imagine-image-2-0/segment-edit"),
        "{segment_description}"
    );
    assert!(
        !segment_description.contains("with grok-imagine-image-2-0/image-edit"),
        "{segment_description}"
    );

    let detection_tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "kie_omnihuman_subject_detection")
        .unwrap();
    let detection_properties = &detection_tool["inputSchema"]["properties"];
    assert!(detection_properties.get("image_url").is_some());
    assert!(detection_properties.get("local_image_path").is_some());
    assert!(detection_properties.get("output_name").is_some());

    child.kill().await.unwrap();
}

async fn send(stdin: &mut tokio::process::ChildStdin, value: Value) {
    stdin
        .write_all(format!("{value}\n").as_bytes())
        .await
        .unwrap();
}

async fn read_response(
    reader: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
) -> Value {
    let line = timeout(Duration::from_secs(5), reader.next_line())
        .await
        .expect("stdio server should respond before timeout")
        .expect("reading line from child stdout should succeed")
        .expect("stdio server should not close stdout before response");
    serde_json::from_str(&line).expect("stdio server response should be valid JSON")
}
