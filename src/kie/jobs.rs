use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use crate::media::SavedMedia;

use super::{
    KieError,
    catalog::{self, ConvenienceField, PromptPolicy, UrlBinding},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationKind {
    Image,
    Video,
}

impl GenerationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GenerationRequest {
    pub model: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub input_urls: Vec<String>,
    #[serde(default)]
    pub local_input_paths: Vec<PathBuf>,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub output_format: Option<String>,
    pub output_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadedInput {
    pub path: PathBuf,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiEnvelope<T> {
    pub code: i64,
    #[serde(default)]
    pub msg: String,
    pub data: Option<T>,
}

impl<T> ApiEnvelope<T> {
    pub fn into_data(self) -> Result<T, KieError> {
        if self.code != 200 {
            return Err(KieError::ApiCode {
                code: self.code,
                message: self.msg,
            });
        }
        self.data.ok_or_else(|| KieError::InvalidResponse {
            message: "missing data field".to_string(),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateTaskData {
    #[serde(rename = "taskId")]
    pub task_id: String,
}

pub type CreateTaskResponse = ApiEnvelope<CreateTaskData>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreditsResponse {
    pub code: i64,
    #[serde(default)]
    pub msg: String,
    pub data: Option<Value>,
}

impl CreditsResponse {
    pub fn into_success(self) -> Result<Self, KieError> {
        if self.code != 200 {
            return Err(KieError::ApiCode {
                code: self.code,
                message: self.msg,
            });
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Waiting,
    Queuing,
    Generating,
    Success,
    Fail,
    Unknown(String),
}

impl<'de> Deserialize<'de> for TaskState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.to_ascii_lowercase().as_str() {
            "waiting" => Self::Waiting,
            "queuing" => Self::Queuing,
            "generating" => Self::Generating,
            "success" => Self::Success,
            "fail" => Self::Fail,
            _ => Self::Unknown(value),
        })
    }
}

impl Serialize for TaskState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::Waiting => "waiting",
            Self::Queuing => "queuing",
            Self::Generating => "generating",
            Self::Success => "success",
            Self::Fail => "fail",
            Self::Unknown(value) => value,
        })
    }
}

impl TaskState {
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Waiting | Self::Queuing | Self::Generating)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskRecord {
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(default, deserialize_with = "null_string")]
    pub model: String,
    pub state: TaskState,
    #[serde(default, deserialize_with = "null_string", rename = "param")]
    pub param_json: String,
    #[serde(default, deserialize_with = "null_string", rename = "resultJson")]
    pub result_json: String,
    #[serde(default, deserialize_with = "null_string", rename = "failCode")]
    pub fail_code: String,
    #[serde(default, deserialize_with = "null_string", rename = "failMsg")]
    pub fail_msg: String,
    #[serde(default, rename = "costTime")]
    pub cost_time: Option<i64>,
    #[serde(default, rename = "completeTime")]
    pub complete_time: Option<i64>,
    #[serde(default, rename = "createTime")]
    pub create_time: Option<i64>,
    #[serde(default, rename = "updateTime")]
    pub update_time: Option<i64>,
    #[serde(default)]
    pub progress: Option<i64>,
    #[serde(default, rename = "creditsConsumed")]
    pub credits_consumed: Option<f64>,
}

fn null_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerationResult {
    pub task_id: String,
    pub model: String,
    pub state: TaskState,
    pub media_type: GenerationKind,
    pub media: Vec<SavedMedia>,
    pub source_urls: Vec<String>,
    pub poster_urls: Vec<String>,
    pub markdown: String,
}

pub fn create_task_payload(
    request: &GenerationRequest,
    uploaded: &[UploadedInput],
    model: &str,
    spec: Option<&catalog::ModelSpec>,
) -> Result<Value, KieError> {
    validate_generation_request(request, spec)?;

    let mut input = match &request.input {
        Value::Object(map) => map.clone(),
        Value::Null => serde_json::Map::new(),
        _ => unreachable!("generation request input was validated"),
    };

    apply_prompt(&mut input, request, spec);
    insert_convenience(
        &mut input,
        spec,
        ConvenienceField::AspectRatio,
        request.aspect_ratio.as_deref(),
    )?;
    insert_convenience(
        &mut input,
        spec,
        ConvenienceField::Resolution,
        request.resolution.as_deref(),
    )?;
    insert_output_format(&mut input, spec, request.output_format.as_deref())?;

    let mut urls = request.input_urls.clone();
    urls.extend(uploaded.iter().map(|item| item.url.clone()));
    if !urls.is_empty() {
        add_urls_to_input(&mut input, spec, &urls)?;
    }

    Ok(json!({
        "model": model,
        "input": input,
    }))
}

pub fn validate_generation_request(
    request: &GenerationRequest,
    spec: Option<&catalog::ModelSpec>,
) -> Result<(), KieError> {
    match &request.input {
        Value::Object(_) | Value::Null => {}
        _ => {
            return Err(KieError::InvalidRequest {
                message: "input must be a JSON object".to_string(),
            });
        }
    }

    validate_input_urls(&request.input_urls)?;
    validate_uncataloged_shortcuts(request, spec)?;

    if spec.is_some_and(|spec| spec.prompt_policy == PromptPolicy::Required)
        && request.prompt.trim().is_empty()
        && request
            .input
            .as_object()
            .and_then(|input| input.get("prompt"))
            .and_then(Value::as_str)
            .is_none_or(|prompt| prompt.trim().is_empty())
    {
        return Err(KieError::EmptyPrompt);
    }

    Ok(())
}

fn validate_uncataloged_shortcuts(
    request: &GenerationRequest,
    spec: Option<&catalog::ModelSpec>,
) -> Result<(), KieError> {
    if spec.is_some() {
        return Ok(());
    }

    let shortcut = if !request.prompt.trim().is_empty() {
        Some("prompt")
    } else if !request.input_urls.is_empty() {
        Some("input_urls")
    } else if !request.local_input_paths.is_empty() {
        Some("local_input_paths")
    } else if request.aspect_ratio.is_some() {
        Some("aspect_ratio")
    } else if request.resolution.is_some() {
        Some("resolution")
    } else if request.output_format.is_some() {
        Some("output_format")
    } else {
        None
    };

    if let Some(shortcut) = shortcut {
        return Err(KieError::InvalidRequest {
            message: format!(
                "uncataloged model {} cannot safely map top-level {shortcut}; pass every model-specific Kie field in input",
                request.model
            ),
        });
    }

    Ok(())
}

fn apply_prompt(
    input: &mut serde_json::Map<String, Value>,
    request: &GenerationRequest,
    spec: Option<&catalog::ModelSpec>,
) {
    let Some(spec) = spec else {
        return;
    };
    if spec.prompt_policy == PromptPolicy::None {
        input.remove("prompt");
        return;
    }
    if !request.prompt.trim().is_empty() {
        input.insert("prompt".to_string(), Value::String(request.prompt.clone()));
    }
}

fn validate_input_urls(urls: &[String]) -> Result<(), KieError> {
    for (index, value) in urls.iter().enumerate() {
        let parsed = url::Url::parse(value).map_err(|_| KieError::InvalidRequest {
            message: format!("input_urls[{index}] must be a valid http or https URL"),
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(KieError::InvalidRequest {
                message: format!("input_urls[{index}] must be a valid http or https URL"),
            });
        }
    }
    Ok(())
}

pub fn has_explicit_media_input(input: &Value) -> bool {
    input
        .as_object()
        .is_some_and(|map| map.keys().any(|field| is_explicit_media_input_field(field)))
}

const EXTRA_MEDIA_INPUT_FIELDS: &[&str] = &[
    "firstFrameUrl",
    "first_frame_url",
    "image",
    "imageUrl",
    "imageUrls",
    "image_url",
    "image_urls",
    "inputUrls",
    "input_urls",
    "lastFrameUrl",
    "last_frame_url",
    "referenceImage",
    "referenceImageUrl",
    "referenceImageUrls",
    "referenceVideo",
    "referenceVideoUrl",
    "reference_image",
    "reference_video",
    "video",
    "videoUrl",
    "videoUrls",
    "video_url",
    "video_urls",
];

fn is_explicit_media_input_field(field: &str) -> bool {
    if EXTRA_MEDIA_INPUT_FIELDS.contains(&field) {
        return true;
    }
    catalog::model_catalog()
        .iter()
        .any(|model| match model.url_binding {
            UrlBinding::None => false,
            UrlBinding::Scalar {
                field: catalog_field,
            }
            | UrlBinding::Array {
                field: catalog_field,
                ..
            } => field == catalog_field,
            UrlBinding::FirstLastFrame {
                first_field,
                last_field,
            } => field == first_field || field == last_field,
        })
}

fn add_urls_to_input(
    input: &mut serde_json::Map<String, Value>,
    spec: Option<&catalog::ModelSpec>,
    urls: &[String],
) -> Result<(), KieError> {
    if input
        .keys()
        .any(|field| is_explicit_media_input_field(field))
    {
        return Err(KieError::InvalidRequest {
            message: "input already contains explicit media fields; use either input media fields or top-level input_urls/local_input_paths, not both".to_string(),
        });
    }

    let Some(spec) = spec else {
        return Err(KieError::InvalidRequest {
            message: "uncataloged models require model-specific media fields in input".to_string(),
        });
    };
    match spec.url_binding {
        UrlBinding::None => Err(KieError::InvalidRequest {
            message: format!(
                "{} does not expose a simple media URL binding; pass model-specific media fields in input",
                spec.id
            ),
        }),
        UrlBinding::Scalar { field } => {
            if urls.len() != 1 {
                return Err(KieError::InvalidRequest {
                    message: format!(
                        "{} field {} accepts exactly one input URL, got {}; choose one media input or use a model that accepts multiple inputs",
                        spec.id,
                        field,
                        urls.len()
                    ),
                });
            }
            insert_scalar_url(input, field, &urls[0]);
            Ok(())
        }
        UrlBinding::Array { field, max_items } => {
            if let Some(max) = max_items
                && urls.len() > max
            {
                return Err(KieError::InvalidRequest {
                    message: format!(
                        "{} field {} accepts at most {max} input URL(s), got {}",
                        spec.id,
                        field,
                        urls.len()
                    ),
                });
            }
            insert_array_urls(input, field, urls);
            Ok(())
        }
        UrlBinding::FirstLastFrame {
            first_field,
            last_field,
        } => {
            if urls.len() > 2 {
                return Err(KieError::InvalidRequest {
                    message: format!(
                        "{} accepts at most two ordered media URLs, got {}",
                        spec.id,
                        urls.len()
                    ),
                });
            }
            insert_first_last_frames(input, first_field, last_field, urls);
            Ok(())
        }
    }
}

fn insert_scalar_url(input: &mut serde_json::Map<String, Value>, field: &str, url: &str) {
    input.insert(field.to_string(), Value::String(url.to_string()));
}

fn insert_array_urls(input: &mut serde_json::Map<String, Value>, field: &str, urls: &[String]) {
    input.insert(
        field.to_string(),
        Value::Array(urls.iter().cloned().map(Value::String).collect()),
    );
}

fn insert_first_last_frames(
    input: &mut serde_json::Map<String, Value>,
    first_field: &str,
    last_field: &str,
    urls: &[String],
) {
    input.insert(first_field.to_string(), Value::String(urls[0].clone()));
    if let Some(last) = urls.get(1) {
        input.insert(last_field.to_string(), Value::String(last.clone()));
    }
}

fn insert_convenience(
    input: &mut serde_json::Map<String, Value>,
    spec: Option<&catalog::ModelSpec>,
    convenience: ConvenienceField,
    value: Option<&str>,
) -> Result<(), KieError> {
    if let Some(value) = value {
        let Some(spec) = spec else {
            return Err(unmapped_convenience_error(convenience));
        };
        let Some(field) = spec.field_for_convenience(convenience) else {
            return Err(KieError::InvalidRequest {
                message: format!(
                    "{} does not expose {} as a convenience field; pass it in input if the model supports it",
                    spec.id,
                    convenience_name(convenience)
                ),
            });
        };
        replace_convenience_value(input, convenience, field, Value::String(value.to_string()));
    } else if let Some(spec) = spec
        && let Some(field) = spec.field_for_convenience(convenience)
        && let Some(value) = take_convenience_value(input, convenience, field)
    {
        input.insert(field.to_string(), value);
    }
    Ok(())
}

fn insert_output_format(
    input: &mut serde_json::Map<String, Value>,
    spec: Option<&catalog::ModelSpec>,
    value: Option<&str>,
) -> Result<(), KieError> {
    if let Some(value) = value {
        let Some(spec) = spec else {
            return Err(unmapped_convenience_error(ConvenienceField::OutputFormat));
        };
        let Some(value) = spec.output_format_value(value) else {
            return Err(KieError::InvalidRequest {
                message: format!(
                    "{} does not support output_format value {value}; inspect kie_models for supported values",
                    spec.id
                ),
            });
        };
        replace_convenience_value(
            input,
            ConvenienceField::OutputFormat,
            "output_format",
            Value::String(value),
        );
    } else if let Some(spec) = spec
        && spec
            .field_for_convenience(ConvenienceField::OutputFormat)
            .is_some()
        && let Some(value) =
            take_convenience_string(input, ConvenienceField::OutputFormat, "output_format")
    {
        let Some(value) = spec.output_format_value(&value) else {
            return Err(KieError::InvalidRequest {
                message: format!(
                    "{} does not support output_format value {value}; inspect kie_models for supported values",
                    spec.id
                ),
            });
        };
        input.insert("output_format".to_string(), Value::String(value));
    }
    Ok(())
}

fn replace_convenience_value(
    input: &mut serde_json::Map<String, Value>,
    convenience: ConvenienceField,
    field: &str,
    value: Value,
) {
    remove_convenience_aliases(input, convenience);
    input.insert(field.to_string(), value);
}

fn take_convenience_value(
    input: &mut serde_json::Map<String, Value>,
    convenience: ConvenienceField,
    field: &str,
) -> Option<Value> {
    let aliases = convenience_aliases_for_field(convenience, field);
    let selected = aliases.iter().find_map(|alias| input.get(*alias).cloned());
    remove_convenience_aliases(input, convenience);
    selected
}

fn take_convenience_string(
    input: &mut serde_json::Map<String, Value>,
    convenience: ConvenienceField,
    field: &str,
) -> Option<String> {
    let aliases = convenience_aliases_for_field(convenience, field);
    let selected = aliases.iter().find_map(|alias| {
        input
            .get(*alias)
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    remove_convenience_aliases(input, convenience);
    selected
}

fn remove_convenience_aliases(
    input: &mut serde_json::Map<String, Value>,
    convenience: ConvenienceField,
) {
    for &alias in convenience_aliases(convenience) {
        input.remove(alias);
    }
}

fn convenience_aliases(convenience: ConvenienceField) -> &'static [&'static str] {
    match convenience {
        ConvenienceField::AspectRatio => &["aspect_ratio", "ratio", "aspectRatio"],
        ConvenienceField::Resolution => &[
            "resolution",
            "image_resolution",
            "output_resolution",
            "imageResolution",
            "outputResolution",
        ],
        ConvenienceField::OutputFormat => &["output_format", "outputFormat"],
    }
}

fn convenience_aliases_for_field(convenience: ConvenienceField, field: &str) -> Vec<&'static str> {
    let mut aliases = convenience_aliases(convenience).to_vec();
    if let Some(position) = aliases.iter().position(|alias| *alias == field) {
        aliases.swap(0, position);
    }
    aliases
}

fn convenience_name(convenience: ConvenienceField) -> &'static str {
    match convenience {
        ConvenienceField::AspectRatio => "aspect_ratio",
        ConvenienceField::Resolution => "resolution",
        ConvenienceField::OutputFormat => "output_format",
    }
}

fn unmapped_convenience_error(convenience: ConvenienceField) -> KieError {
    KieError::InvalidRequest {
        message: format!(
            "uncataloged models cannot safely map top-level {}; pass the exact Kie field in input",
            convenience_name(convenience)
        ),
    }
}

pub fn model_kind(model: &str) -> Option<GenerationKind> {
    let lowered = model.to_ascii_lowercase();
    if lowered.contains("chat") || lowered.contains("claude") {
        return None;
    }
    if lowered.contains("video")
        || lowered.contains("kling")
        || lowered.contains("sora")
        || lowered.contains("runway")
        || lowered.contains("hailuo")
        || lowered.contains("veo")
        || lowered.contains("seedance")
    {
        return Some(GenerationKind::Video);
    }
    if lowered.contains("audio") || lowered.contains("speech") || lowered.contains("voice") {
        return None;
    }
    if lowered.contains("image")
        || lowered.contains("imagen")
        || lowered.contains("banana")
        || lowered.contains("flux")
        || lowered.contains("recraft")
        || lowered.contains("seedream")
        || lowered.contains("ideogram")
        || lowered.contains("qwen")
        || lowered.contains("grok-imagine")
        || lowered.contains("z-image")
        || lowered.contains("topaz")
    {
        return Some(GenerationKind::Image);
    }
    None
}

pub fn validate_model(model: &str, expected: GenerationKind) -> Result<(), KieError> {
    if let Some(spec) = catalog::resolve_model_any_kind(model) {
        if spec.kind == expected {
            return Ok(());
        }
        return Err(KieError::UnsupportedModel {
            kind: expected.as_str(),
            model: model.to_string(),
        });
    }
    if catalog::has_catalog_match(model) {
        return Err(KieError::UnsupportedModel {
            kind: expected.as_str(),
            model: model.to_string(),
        });
    }
    match model_kind(model) {
        Some(kind) if kind == expected => Ok(()),
        _ => Err(KieError::UnsupportedModel {
            kind: expected.as_str(),
            model: model.to_string(),
        }),
    }
}

pub fn public_status(record: &TaskRecord) -> BTreeMap<&'static str, Value> {
    BTreeMap::from([
        ("task_id", Value::String(record.task_id.clone())),
        ("model", Value::String(record.model.clone())),
        ("state", json!(record.state)),
        ("fail_code", Value::String(record.fail_code.clone())),
        ("fail_msg", Value::String(record.fail_msg.clone())),
        (
            "credits_consumed",
            record
                .credits_consumed
                .map(Value::from)
                .unwrap_or(Value::Null),
        ),
        (
            "progress",
            record.progress.map(Value::from).unwrap_or(Value::Null),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(model: &str) -> GenerationRequest {
        GenerationRequest {
            model: model.to_string(),
            prompt: "test prompt".to_string(),
            input_urls: Vec::new(),
            local_input_paths: Vec::new(),
            input: json!({}),
            aspect_ratio: None,
            resolution: None,
            output_format: None,
            output_name: None,
        }
    }

    #[test]
    fn convenience_aspect_ratio_uses_model_specific_ratio_field() {
        let mut request = request("wan/2-7-text-to-video");
        request.aspect_ratio = Some("4:3".to_string());
        let spec = catalog::resolve_model(&request.model, GenerationKind::Video);
        let payload = create_task_payload(&request, &[], "wan/2-7-text-to-video", spec).unwrap();

        assert_eq!(payload["input"]["ratio"], "4:3");
        assert!(payload["input"].get("aspect_ratio").is_none());
    }

    #[test]
    fn convenience_output_format_follows_model_enum() {
        let mut request = request("google/nano-banana");
        request.output_format = Some("JPG".to_string());
        let spec = catalog::resolve_model(&request.model, GenerationKind::Image);
        let payload = create_task_payload(&request, &[], "google/nano-banana", spec).unwrap();

        assert_eq!(payload["input"]["output_format"], "jpeg");
    }

    #[test]
    fn top_level_convenience_overrides_input_aliases() {
        let mut request = request("wan/2-7-text-to-video");
        request.aspect_ratio = Some("4:3".to_string());
        request.input = json!({
            "aspect_ratio": "1:1",
            "ratio": "1:1"
        });
        let spec = catalog::resolve_model(&request.model, GenerationKind::Video);
        let payload = create_task_payload(&request, &[], "wan/2-7-text-to-video", spec).unwrap();

        assert_eq!(payload["input"]["ratio"], "4:3");
        assert!(payload["input"].get("aspect_ratio").is_none());
    }

    #[test]
    fn input_aliases_are_normalized_to_model_fields() {
        let mut request = request("wan/2-7-text-to-video");
        request.input = json!({ "aspectRatio": "4:3" });
        let spec = catalog::resolve_model(&request.model, GenerationKind::Video);
        let payload = create_task_payload(&request, &[], "wan/2-7-text-to-video", spec).unwrap();

        assert_eq!(payload["input"]["ratio"], "4:3");
        assert!(payload["input"].get("aspectRatio").is_none());
    }

    #[test]
    fn input_output_format_alias_is_normalized() {
        let mut request = request("nano-banana-pro");
        request.input = json!({ "outputFormat": "JPEG" });
        let spec = catalog::resolve_model(&request.model, GenerationKind::Image);
        let payload = create_task_payload(&request, &[], "nano-banana-pro", spec).unwrap();

        assert_eq!(payload["input"]["output_format"], "jpg");
        assert!(payload["input"].get("outputFormat").is_none());
    }

    #[test]
    fn catalog_url_fields_use_documented_scalar_names() {
        let mut image_request = request("topaz/image-upscale");
        image_request.input_urls = vec!["https://example.com/image.png".to_string()];
        let image_spec = catalog::resolve_model(&image_request.model, GenerationKind::Image);
        let image_payload =
            create_task_payload(&image_request, &[], "topaz/image-upscale", image_spec).unwrap();
        assert_eq!(
            image_payload["input"]["image_url"],
            "https://example.com/image.png"
        );

        let mut video_request = request("happyhorse/video-edit");
        video_request.input_urls = vec!["https://example.com/video.mp4".to_string()];
        let video_spec = catalog::resolve_model(&video_request.model, GenerationKind::Video);
        let video_payload =
            create_task_payload(&video_request, &[], "happyhorse/video-edit", video_spec).unwrap();
        assert_eq!(
            video_payload["input"]["video_url"],
            "https://example.com/video.mp4"
        );
    }

    #[test]
    fn model_specific_first_last_frame_fields_are_used() {
        let cases = [
            (
                "kling/v2-5-turbo-image-to-video-pro",
                "image_url",
                "tail_image_url",
            ),
            ("kling/v2-1-pro", "image_url", "tail_image_url"),
            (
                "bytedance/v1-lite-image-to-video",
                "image_url",
                "end_image_url",
            ),
            ("hailuo/02-image-to-video-pro", "image_url", "end_image_url"),
            (
                "hailuo/02-image-to-video-standard",
                "image_url",
                "end_image_url",
            ),
            (
                "pixverse-v6/transition",
                "first_frame_image_url",
                "last_frame_image_url",
            ),
        ];

        for (model, first_field, last_field) in cases {
            let mut request = request(model);
            request.input_urls = vec![
                "https://example.com/first.png".to_string(),
                "https://example.com/last.png".to_string(),
            ];
            let spec = catalog::resolve_model(model, GenerationKind::Video);
            let payload = create_task_payload(&request, &[], model, spec).unwrap();

            assert_eq!(
                payload["input"][first_field],
                "https://example.com/first.png"
            );
            assert_eq!(payload["input"][last_field], "https://example.com/last.png");
        }
    }

    #[test]
    fn corrected_array_limits_accept_the_boundary_and_reject_one_more() {
        let cases = [
            ("grok-imagine-video-1-5-preview", 7),
            ("ideogram/character", 1),
            ("kling-3.0/video", 2),
            ("kling/v3-turbo-image-to-video", 1),
            ("gemini-omni-video", 7),
            ("happyhorse/image-to-video", 1),
        ];

        for (model, max) in cases {
            let mut request = request(model);
            request.input_urls = (0..max)
                .map(|index| format!("https://example.com/{index}.png"))
                .collect();
            let spec = catalog::resolve_model_any_kind(model);
            assert!(
                create_task_payload(&request, &[], model, spec).is_ok(),
                "{model} should accept {max} URL(s)"
            );

            request
                .input_urls
                .push("https://example.com/overflow.png".to_string());
            let error = create_task_payload(&request, &[], model, spec).unwrap_err();
            assert!(error.to_string().contains("accepts at most"), "{model}");
        }
    }

    #[test]
    fn newly_bound_simple_media_fields_are_assembled() {
        let cases = [
            ("wan/2-7-videoedit", "video_url", false),
            ("happyhorse/image-to-video", "image_urls", true),
        ];

        for (model, field, array) in cases {
            let mut request = request(model);
            request.input_urls = vec!["https://example.com/input.mp4".to_string()];
            let spec = catalog::resolve_model(model, GenerationKind::Video);
            let payload = create_task_payload(&request, &[], model, spec).unwrap();
            if array {
                assert_eq!(
                    payload["input"][field],
                    json!(["https://example.com/input.mp4"]),
                    "{model}"
                );
            } else {
                assert_eq!(
                    payload["input"][field], "https://example.com/input.mp4",
                    "{model}"
                );
            }
        }
    }

    #[test]
    fn latest_model_convenience_fields_use_documented_names() {
        let mut qwen = request("qwen2/text-to-image");
        qwen.aspect_ratio = Some("16:9".to_string());
        qwen.output_format = Some("jpg".to_string());
        let qwen_spec = catalog::resolve_model(&qwen.model, GenerationKind::Image);
        let qwen_payload = create_task_payload(&qwen, &[], &qwen.model, qwen_spec).unwrap();
        assert_eq!(qwen_payload["input"]["image_size"], "16:9");
        assert_eq!(qwen_payload["input"]["output_format"], "jpeg");

        let mut pixverse = request("pixverse-v6/text-to-video");
        pixverse.resolution = Some("1080p".to_string());
        let pixverse_spec = catalog::resolve_model(&pixverse.model, GenerationKind::Video);
        let pixverse_payload =
            create_task_payload(&pixverse, &[], &pixverse.model, pixverse_spec).unwrap();
        assert_eq!(pixverse_payload["input"]["quality"], "1080p");
        assert!(pixverse_payload["input"].get("resolution").is_none());
    }

    #[test]
    fn corrected_image_conveniences_use_documented_fields_and_values() {
        let mut seedream = request("seedream/5-lite-text-to-image");
        seedream.resolution = Some("high".to_string());
        seedream.output_format = Some("jpg".to_string());
        let seedream_spec = catalog::resolve_model(&seedream.model, GenerationKind::Image);
        let seedream_payload =
            create_task_payload(&seedream, &[], &seedream.model, seedream_spec).unwrap();
        assert_eq!(seedream_payload["input"]["quality"], "high");
        assert_eq!(seedream_payload["input"]["output_format"], "jpeg");

        let mut qwen = request("qwen2/image-edit");
        qwen.aspect_ratio = Some("16:9".to_string());
        let qwen_spec = catalog::resolve_model(&qwen.model, GenerationKind::Image);
        let qwen_payload = create_task_payload(&qwen, &[], &qwen.model, qwen_spec).unwrap();
        assert_eq!(qwen_payload["input"]["image_size"], "16:9");
    }

    #[test]
    fn prompt_policy_controls_validation_and_payload_assembly() {
        let mut required = request("gpt-image-2-text-to-image");
        required.prompt.clear();
        let required_spec = catalog::resolve_model(&required.model, GenerationKind::Image);
        assert!(matches!(
            create_task_payload(&required, &[], &required.model, required_spec),
            Err(KieError::EmptyPrompt)
        ));

        required.input = json!({ "prompt": "prompt supplied in raw input" });
        let payload = create_task_payload(&required, &[], &required.model, required_spec).unwrap();
        assert_eq!(payload["input"]["prompt"], "prompt supplied in raw input");

        let mut optional = request("grok-imagine/image-to-video");
        optional.prompt.clear();
        let optional_spec = catalog::resolve_model(&optional.model, GenerationKind::Video);
        let payload = create_task_payload(&optional, &[], &optional.model, optional_spec).unwrap();
        assert!(payload["input"].get("prompt").is_none());

        let mut promptless = request("topaz/image-upscale");
        promptless.input_urls = vec!["https://example.com/input.png".to_string()];
        promptless.input = json!({ "prompt": "also ignored" });
        let promptless_spec = catalog::resolve_model(&promptless.model, GenerationKind::Image);
        let payload =
            create_task_payload(&promptless, &[], &promptless.model, promptless_spec).unwrap();
        assert!(payload["input"].get("prompt").is_none());
    }

    #[test]
    fn uncataloged_models_require_explicit_model_specific_input() {
        let mut request = request("future-image-to-video");
        request.prompt.clear();
        request.input = json!({
            "prompt": "future prompt",
            "image_url": "https://example.com/input.png"
        });
        let payload = create_task_payload(&request, &[], &request.model, None).unwrap();
        assert_eq!(payload["input"], request.input);

        request.prompt = "unsafe shortcut".to_string();
        let error = create_task_payload(&request, &[], &request.model, None).unwrap_err();
        assert!(error.to_string().contains("top-level prompt"));

        request.prompt.clear();
        request.input_urls = vec!["https://example.com/input.png".to_string()];
        let error = create_task_payload(&request, &[], &request.model, None).unwrap_err();
        assert!(error.to_string().contains("top-level input_urls"));

        request.input_urls.clear();
        request.aspect_ratio = Some("16:9".to_string());
        let error = create_task_payload(&request, &[], &request.model, None).unwrap_err();
        assert!(error.to_string().contains("top-level aspect_ratio"));
    }

    #[test]
    fn raw_model_kind_inference_prioritizes_output_media() {
        assert_eq!(
            model_kind("future/speech-to-video"),
            Some(GenerationKind::Video)
        );
        assert_eq!(
            model_kind("future/gemini-image-generator"),
            Some(GenerationKind::Image)
        );
        assert_eq!(
            model_kind("future/gemini-video-generator"),
            Some(GenerationKind::Video)
        );
        assert_eq!(model_kind("wan/future-image"), Some(GenerationKind::Image));
    }

    #[test]
    fn known_catalog_models_do_not_validate_for_wrong_kind() {
        let err = validate_model("wan/2-7-image", GenerationKind::Video).unwrap_err();
        assert!(matches!(err, KieError::UnsupportedModel { .. }));

        let err = validate_model("gemini-omni-video", GenerationKind::Image).unwrap_err();
        assert!(matches!(err, KieError::UnsupportedModel { .. }));
    }

    #[test]
    fn ambiguous_catalog_matches_do_not_fall_through_to_heuristics() {
        let err = validate_model("banana", GenerationKind::Image).unwrap_err();
        assert!(matches!(err, KieError::UnsupportedModel { .. }));
    }

    #[test]
    fn catalog_media_binding_fields_are_explicit_inputs() {
        for model in catalog::model_catalog() {
            match model.url_binding {
                UrlBinding::None => {}
                UrlBinding::Scalar { field } | UrlBinding::Array { field, .. } => {
                    assert!(
                        is_explicit_media_input_field(field),
                        "{} media field {field} is not detected as explicit input",
                        model.id
                    );
                }
                UrlBinding::FirstLastFrame {
                    first_field,
                    last_field,
                } => {
                    assert!(is_explicit_media_input_field(first_field));
                    assert!(is_explicit_media_input_field(last_field));
                }
            }
        }
    }

    #[test]
    fn common_media_field_aliases_are_explicit_inputs() {
        for field in [
            "imageUrl",
            "image_urls",
            "inputUrls",
            "referenceImageUrls",
            "reference_video",
            "videoUrl",
            "video_urls",
        ] {
            assert!(
                is_explicit_media_input_field(field),
                "media field {field} is not detected as explicit input"
            );
        }
    }

    #[test]
    fn generation_input_urls_must_be_http_or_https() {
        for value in ["not a URL", "file:///tmp/input.png", "https://"] {
            let mut request = request("nano-banana-2");
            request.input_urls = vec![value.to_string()];
            let spec = catalog::resolve_model(&request.model, GenerationKind::Image);

            let err = validate_generation_request(&request, spec).unwrap_err();
            assert!(err.to_string().contains("input_urls[0]"), "{value}");
        }

        let mut request = request("nano-banana-2");
        request.input_urls = vec!["https://example.com/input.png?token=secret".to_string()];
        let spec = catalog::resolve_model(&request.model, GenerationKind::Image);
        assert!(validate_generation_request(&request, spec).is_ok());
    }

    #[test]
    fn task_state_deserialization_is_case_insensitive() {
        for (value, expected) in [
            ("WAITING", TaskState::Waiting),
            ("Queuing", TaskState::Queuing),
            ("GENERATING", TaskState::Generating),
            ("SUCCESS", TaskState::Success),
            ("FAIL", TaskState::Fail),
            ("new-state", TaskState::Unknown("new-state".to_string())),
        ] {
            let state: TaskState = serde_json::from_value(json!(value)).unwrap();
            assert_eq!(state, expected);
        }

        assert_eq!(
            serde_json::to_value(TaskState::Unknown("processing-v2".to_string())).unwrap(),
            json!("processing-v2")
        );
    }

    #[test]
    fn task_record_accepts_null_text_fields() {
        let record: TaskRecord = serde_json::from_value(json!({
            "taskId": "task_1",
            "model": null,
            "state": "waiting",
            "param": null,
            "resultJson": null,
            "failCode": null,
            "failMsg": null
        }))
        .unwrap();

        assert_eq!(record.task_id, "task_1");
        assert_eq!(record.model, "");
        assert_eq!(record.param_json, "");
        assert_eq!(record.result_json, "");
        assert_eq!(record.fail_code, "");
        assert_eq!(record.fail_msg, "");
    }
}
