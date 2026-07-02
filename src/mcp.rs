use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo},
    schemars::JsonSchema,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    config::Config,
    kie::{
        KieClient, KieError,
        catalog::{CATALOG_SOURCE, models_for, resolve_model_any_kind},
        jobs::{GenerationKind, GenerationRequest, model_kind, public_status},
    },
};

const SERVER_INSTRUCTIONS: &str = "Generate Kie.ai images and videos, then use the returned local files. Call kie_models when the requested model is unclear. Put model-specific fields in input, and use input_urls or local_input_paths for reference media.";

#[derive(Debug, Clone)]
pub struct KieMcp {
    client: KieClient,
}

impl KieMcp {
    pub fn from_env() -> Result<Self, KieError> {
        Ok(Self {
            client: KieClient::new(Config::from_env()?),
        })
    }

    pub fn new(client: KieClient) -> Self {
        Self { client }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateParams {
    #[schemars(
        description = "Kie Market model id or catalog alias, for example Nano Banana 2, gpt-image-2-text-to-image, or wan/2-7-text-to-video. Use kie_models when unsure."
    )]
    pub model: String,
    #[schemars(description = "Prompt to send as input.prompt.")]
    pub prompt: String,
    #[serde(default)]
    #[schemars(
        description = "Public URLs to pass to the model. These are merged with uploaded local_input_paths when the selected model accepts an array media input."
    )]
    pub input_urls: Vec<String>,
    #[serde(default)]
    #[schemars(
        description = "Local image/video files to upload to Kie before task creation. Files must be regular media files within KIE_MCP_MAX_UPLOAD_BYTES and, when configured, KIE_MCP_INPUT_ROOTS. Reused unchanged files are cached during the MCP server lifetime, including concurrent uploads of the same file."
    )]
    pub local_input_paths: Vec<std::path::PathBuf>,
    #[serde(default)]
    #[schemars(
        description = "Optional object containing model-specific Kie input fields. Top-level convenience fields override equivalent values here."
    )]
    pub input: Map<String, Value>,
    #[serde(default)]
    #[schemars(description = "Convenience image aspect ratio such as 4:3, 1:1, 16:9, or auto.")]
    pub aspect_ratio: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Convenience image/video resolution or quality when supported by the selected model."
    )]
    pub resolution: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Convenience output image format such as jpg, jpeg, or png when supported."
    )]
    pub output_format: Option<String>,
    #[schemars(description = "Optional safe filename stem for downloaded outputs.")]
    pub output_name: Option<String>,
}

impl From<GenerateParams> for GenerationRequest {
    fn from(value: GenerateParams) -> Self {
        Self {
            model: value.model,
            prompt: value.prompt,
            input_urls: value.input_urls,
            local_input_paths: value.local_input_paths,
            input: Value::Object(value.input),
            aspect_ratio: value.aspect_ratio,
            resolution: value.resolution,
            output_format: value.output_format,
            output_name: value.output_name,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskStatusParams {
    #[schemars(description = "Kie task id returned by createTask or a generation tool.")]
    pub task_id: String,
    #[serde(default)]
    #[schemars(
        description = "If true, completed tasks are downloaded to the configured output directory."
    )]
    pub download_if_complete: bool,
    #[serde(default)]
    #[schemars(
        description = "Optional media type to use when downloading completed media. If omitted, the server infers it from the task model when possible and otherwise defaults to image."
    )]
    pub media_type: Option<GenerationKind>,
    #[schemars(
        description = "Optional safe filename and output directory stem for downloaded outputs."
    )]
    pub output_name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UploadParams {
    #[schemars(
        description = "Local image/video file to upload. The path must point to a regular file within the configured upload size and input-root policy."
    )]
    pub path: std::path::PathBuf,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ModelsParams {
    #[serde(default)]
    #[schemars(description = "Optional media type filter.")]
    pub media_type: Option<GenerationKind>,
    #[serde(default)]
    #[schemars(description = "Optional free-text search over ids, display names, and aliases.")]
    pub query: Option<String>,
}

#[derive(Debug, Serialize)]
struct UploadResult {
    path: std::path::PathBuf,
    url: String,
}

#[tool_router]
impl KieMcp {
    #[tool(
        description = "Generate or edit an image with a Kie.ai Market image model, wait for completion, download the image, and return local preview markdown."
    )]
    async fn kie_generate_image(
        &self,
        Parameters(params): Parameters<GenerateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.generate(params, GenerationKind::Image).await
    }

    #[tool(
        description = "Generate a video with a Kie.ai Market video model, wait for completion, download the MP4, and return a local link plus poster preview if available."
    )]
    async fn kie_generate_video(
        &self,
        Parameters(params): Parameters<GenerateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.generate(params, GenerationKind::Video).await
    }

    #[tool(
        description = "List Kie.ai Market image/video models known by this MCP, including canonical ids, aliases, simple media input bindings, and convenience fields."
    )]
    async fn kie_models(
        &self,
        Parameters(params): Parameters<ModelsParams>,
    ) -> Result<CallToolResult, McpError> {
        let models = models_for(params.media_type, params.query.as_deref());
        let value = serde_json::to_value(json!({
            "source": CATALOG_SOURCE,
            "count": models.len(),
            "models": models,
            "note": "Pass display_name, alias, or canonical id to kie_generate_image/kie_generate_video. Model-specific fields that are not listed as convenience fields belong in input."
        }))
        .map_err(to_mcp_error)?;
        let lines = models
            .iter()
            .map(|model| {
                let convenience = model.convenience_summary();
                let convenience = if convenience.is_empty() {
                    "model-specific input".to_string()
                } else {
                    convenience.join(", ")
                };
                format!(
                    "- {} (`{}`): input {}, convenience {}",
                    model.display_name,
                    model.id,
                    model.input_summary(),
                    convenience
                )
            })
            .collect::<Vec<_>>();
        let markdown = if lines.is_empty() {
            "No matching Kie models in the local catalog.".to_string()
        } else {
            lines.join("\n")
        };
        Ok(tool_success(value, &markdown))
    }

    #[tool(
        description = "Query a Kie.ai Market task status and optionally download media if it has completed, inferring media_type from the task model when omitted."
    )]
    async fn kie_task_status(
        &self,
        Parameters(params): Parameters<TaskStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let record = match self.client.record_info(&params.task_id).await {
            Ok(record) => record,
            Err(err) => return Ok(tool_error(err)),
        };
        if params.download_if_complete && record.state == crate::kie::jobs::TaskState::Success {
            let media_type = task_status_download_kind(&record.model, params.media_type);
            match self
                .client
                .download_completed(record, media_type, params.output_name.as_deref())
                .await
            {
                Ok(result) => {
                    return Ok(tool_success(
                        KieClient::result_to_json(&result),
                        &result.markdown,
                    ));
                }
                Err(err) => return Ok(tool_error(err)),
            }
        }
        let payload = json!(public_status(&record));
        Ok(tool_success(
            payload,
            &format!("Task {} is {:?}", record.task_id, record.state),
        ))
    }

    #[tool(
        description = "Upload a regular local image or video file to Kie.ai File Upload API, enforcing the configured size/input-root policy, and return the temporary Kie URL. Reused unchanged files are cached and concurrent uploads are deduplicated."
    )]
    async fn kie_upload_media(
        &self,
        Parameters(params): Parameters<UploadParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.client.upload_file(&params.path).await {
            Ok(item) => {
                let result = UploadResult {
                    path: item.path,
                    url: item.url,
                };
                let value = serde_json::to_value(result).map_err(to_mcp_error)?;
                Ok(tool_success(value, "Uploaded media to Kie."))
            }
            Err(err) => Ok(tool_error(err)),
        }
    }

    #[tool(description = "Return the current Kie.ai account credit balance.")]
    async fn kie_credits(&self) -> Result<CallToolResult, McpError> {
        match self.client.credits().await {
            Ok(result) => {
                let value = serde_json::to_value(result).map_err(to_mcp_error)?;
                Ok(tool_success(value, "Fetched Kie credit balance."))
            }
            Err(err) => Ok(tool_error(err)),
        }
    }
}

impl KieMcp {
    async fn generate(
        &self,
        params: GenerateParams,
        kind: GenerationKind,
    ) -> Result<CallToolResult, McpError> {
        match self.client.generate_and_wait(params.into(), kind).await {
            Ok(result) => Ok(tool_success(
                KieClient::result_to_json(&result),
                &result.markdown,
            )),
            Err(err) => Ok(tool_error(err)),
        }
    }
}

#[tool_handler]
impl ServerHandler for KieMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("kie-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(SERVER_INSTRUCTIONS)
    }
}

pub async fn serve_stdio() -> anyhow::Result<()> {
    let service = KieMcp::from_env()?.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

fn tool_success(structured: Value, markdown: &str) -> CallToolResult {
    let mut result = CallToolResult::structured(structured);
    result.content = vec![ContentBlock::text(markdown.to_string())];
    result
}

fn tool_error(err: KieError) -> CallToolResult {
    let message = crate::kie::client::redact(&err.to_string());
    let mut result = CallToolResult::structured_error(json!({ "error": message }));
    result.content = vec![ContentBlock::text(message)];
    result
}

fn to_mcp_error(err: serde_json::Error) -> McpError {
    McpError::internal_error(err.to_string(), None)
}

fn task_status_download_kind(model: &str, explicit: Option<GenerationKind>) -> GenerationKind {
    explicit
        .or_else(|| resolve_model_any_kind(model).map(|spec| spec.kind))
        .or_else(|| model_kind(model))
        .unwrap_or(GenerationKind::Image)
}

#[cfg(test)]
mod tests {
    use super::task_status_download_kind;
    use crate::kie::jobs::GenerationKind;

    #[test]
    fn task_status_download_kind_infers_catalog_video_models() {
        assert_eq!(
            task_status_download_kind("wan/2-7-text-to-video", None),
            GenerationKind::Video
        );
    }

    #[test]
    fn task_status_download_kind_keeps_explicit_override() {
        assert_eq!(
            task_status_download_kind("wan/2-7-text-to-video", Some(GenerationKind::Image)),
            GenerationKind::Image
        );
    }

    #[test]
    fn task_status_download_kind_defaults_to_image_when_unknown() {
        assert_eq!(
            task_status_download_kind("unknown-model", None),
            GenerationKind::Image
        );
    }
}
