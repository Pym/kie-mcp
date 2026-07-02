use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use crate::media::SavedMedia;

use super::{
    KieError,
    catalog::{self, ConvenienceField, UrlBinding},
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
    validate_generation_request(request)?;

    let mut input = match &request.input {
        Value::Object(map) => map.clone(),
        Value::Null => serde_json::Map::new(),
        _ => unreachable!("generation request input was validated"),
    };

    input.insert("prompt".to_string(), Value::String(request.prompt.clone()));
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
        add_urls_to_input(&mut input, model, spec, &urls)?;
    }

    Ok(json!({
        "model": model,
        "input": input,
    }))
}

pub fn validate_generation_request(request: &GenerationRequest) -> Result<(), KieError> {
    if request.prompt.trim().is_empty() {
        return Err(KieError::EmptyPrompt);
    }

    match &request.input {
        Value::Object(_) | Value::Null => {}
        _ => {
            return Err(KieError::InvalidRequest {
                message: "input must be a JSON object".to_string(),
            });
        }
    }

    validate_input_urls(&request.input_urls)
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
            UrlBinding::FirstLastFrame => matches!(field, "first_frame_url" | "last_frame_url"),
        })
}

fn add_urls_to_input(
    input: &mut serde_json::Map<String, Value>,
    model: &str,
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

    if let Some(spec) = spec {
        match spec.url_binding {
            UrlBinding::None => {
                return Err(KieError::InvalidRequest {
                    message: format!(
                        "{} does not expose a simple media URL binding; pass model-specific media fields in input",
                        spec.id
                    ),
                });
            }
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
                return Ok(());
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
                return Ok(());
            }
            UrlBinding::FirstLastFrame => {
                if urls.len() > 2 {
                    return Err(KieError::InvalidRequest {
                        message: format!(
                            "{} accepts at most first and last frame URLs, got {}",
                            spec.id,
                            urls.len()
                        ),
                    });
                }
                insert_first_last_frames(input, urls);
                return Ok(());
            }
        }
    }

    if model.contains("image-to-video") {
        if urls.len() > 2 {
            return Err(KieError::InvalidRequest {
                message: format!(
                    "{model} accepts at most first and last frame URLs, got {}",
                    urls.len()
                ),
            });
        }
        input.insert(
            "first_frame_url".to_string(),
            Value::String(urls[0].clone()),
        );
        if let Some(last) = urls.get(1) {
            input.insert("last_frame_url".to_string(), Value::String(last.clone()));
        }
    } else if model.contains("image-to-image") || model.contains("gpt-image") {
        input.insert(
            "input_urls".to_string(),
            Value::Array(urls.iter().cloned().map(Value::String).collect()),
        );
    } else if urls.len() == 1 {
        input.insert("image".to_string(), Value::String(urls[0].clone()));
    } else {
        input.insert(
            "input_urls".to_string(),
            Value::Array(urls.iter().cloned().map(Value::String).collect()),
        );
    }
    Ok(())
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

fn insert_first_last_frames(input: &mut serde_json::Map<String, Value>, urls: &[String]) {
    input.insert(
        "first_frame_url".to_string(),
        Value::String(urls[0].clone()),
    );
    if let Some(last) = urls.get(1) {
        input.insert("last_frame_url".to_string(), Value::String(last.clone()));
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
            let fallback = match convenience {
                ConvenienceField::AspectRatio => "aspect_ratio",
                ConvenienceField::Resolution => "resolution",
                ConvenienceField::OutputFormat => "output_format",
            };
            replace_convenience_value(
                input,
                convenience,
                fallback,
                Value::String(value.to_string()),
            );
            return Ok(());
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
            replace_convenience_value(
                input,
                ConvenienceField::OutputFormat,
                "output_format",
                Value::String(normalize_output_format_fallback(value)),
            );
            return Ok(());
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

fn normalize_output_format_fallback(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "jpg".to_string(),
        "png" => "png".to_string(),
        _ => value.to_string(),
    }
}

fn convenience_name(convenience: ConvenienceField) -> &'static str {
    match convenience {
        ConvenienceField::AspectRatio => "aspect_ratio",
        ConvenienceField::Resolution => "resolution",
        ConvenienceField::OutputFormat => "output_format",
    }
}

pub fn model_kind(model: &str) -> Option<GenerationKind> {
    let lowered = model.to_ascii_lowercase();
    if lowered.contains("audio")
        || lowered.contains("speech")
        || lowered.contains("voice")
        || lowered.contains("chat")
        || lowered.contains("gemini")
        || lowered.contains("claude")
    {
        return None;
    }
    if lowered.contains("video")
        || lowered.contains("kling")
        || lowered.contains("wan/")
        || lowered.contains("sora")
        || lowered.contains("runway")
        || lowered.contains("hailuo")
        || lowered.contains("veo")
        || lowered.contains("seedance")
    {
        return Some(GenerationKind::Video);
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
                UrlBinding::FirstLastFrame => {
                    assert!(is_explicit_media_input_field("first_frame_url"));
                    assert!(is_explicit_media_input_field("last_frame_url"));
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

            let err = validate_generation_request(&request).unwrap_err();
            assert!(err.to_string().contains("input_urls[0]"), "{value}");
        }

        let mut request = request("nano-banana-2");
        request.input_urls = vec!["https://example.com/input.png?token=secret".to_string()];
        assert!(validate_generation_request(&request).is_ok());
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
