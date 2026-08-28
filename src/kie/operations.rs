use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    KieError,
    jobs::{TaskRecord, TaskState},
};

pub const GROK_SEGMENT_MAP_MODEL: &str = "grok-imagine-image-2-0/segment-map";
pub const OMNIHUMAN_IDENTIFICATION_MODEL: &str = "omnihuman-1-5/human-identification";
pub const OMNIHUMAN_SUBJECT_DETECTION_MODEL: &str = "omnihuman-1-5/subject-detection";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredOperation {
    GrokSegmentMap,
    OmnihumanIdentification,
    OmnihumanSubjectDetection,
}

impl StructuredOperation {
    pub fn from_model(model: &str) -> Option<Self> {
        match model {
            GROK_SEGMENT_MAP_MODEL => Some(Self::GrokSegmentMap),
            OMNIHUMAN_IDENTIFICATION_MODEL => Some(Self::OmnihumanIdentification),
            OMNIHUMAN_SUBJECT_DETECTION_MODEL => Some(Self::OmnihumanSubjectDetection),
            _ => None,
        }
    }

    pub const fn model(self) -> &'static str {
        match self {
            Self::GrokSegmentMap => GROK_SEGMENT_MAP_MODEL,
            Self::OmnihumanIdentification => OMNIHUMAN_IDENTIFICATION_MODEL,
            Self::OmnihumanSubjectDetection => OMNIHUMAN_SUBJECT_DETECTION_MODEL,
        }
    }

    pub const fn tool_name(self) -> &'static str {
        match self {
            Self::GrokSegmentMap => "kie_grok_image_2_segment_map",
            Self::OmnihumanIdentification => "kie_omnihuman_human_identification",
            Self::OmnihumanSubjectDetection => "kie_omnihuman_subject_detection",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GeminiOmniVoice {
    Achernar,
    Achird,
    Algenib,
    Algieba,
    Alnilam,
    Aoede,
    Autonoe,
    Callirrhoe,
    Charon,
    Despina,
    Enceladus,
    Erinome,
    Fenrir,
    Gacrux,
    Iapetus,
    Kore,
    Laomedeia,
    Leda,
    Orus,
    Puck,
    Pulcherrima,
    Rasalgethi,
    Sadachbia,
    Sadaltager,
    Schedar,
    Sulafat,
    Umbriel,
    Vindemiatrix,
    Zephyr,
    Zubenelgenubi,
}

#[derive(Debug, Serialize)]
pub(crate) struct GeminiOmniAudioRequest<'a> {
    pub audio_id: GeminiOmniVoice,
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_dialogue: Option<&'a str>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeminiOmniAudioResult {
    #[serde(rename(deserialize = "kieAudioId"))]
    pub audio_id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct GeminiOmniCharacterRequest<'a> {
    pub descriptions: &'a str,
    pub image_urls: [&'a str; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_ids: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character_name: Option<&'a str>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeminiOmniCharacterResult {
    #[serde(rename(deserialize = "characterId"))]
    pub character_id: String,
    #[serde(rename(deserialize = "characterName"))]
    pub character_name: String,
    #[serde(rename(deserialize = "imageUrl"))]
    pub image_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuredTaskResult {
    pub task_id: String,
    pub model: String,
    pub state: TaskState,
    #[serde(flatten)]
    pub output: StructuredTaskOutput,
    pub markdown: String,
}

impl StructuredTaskResult {
    pub fn refresh_markdown(&mut self) {
        self.markdown = match &self.output {
            StructuredTaskOutput::GrokSegmentMap {
                segments_count,
                segments,
            } => {
                let mut lines = vec![
                    format!("Grok found {segments_count} segments."),
                    "Kie's current Segment Edit schema accepts selected indexes of 1 or greater; index 0 cannot be sent to Segment Edit.".to_string(),
                ];
                for segment in segments {
                    lines.push(format!(
                        "Segment {}: {}",
                        segment.index,
                        one_line(&segment.name)
                    ));
                    append_preview(
                        &mut lines,
                        segment.local_path.as_ref(),
                        segment.preview_error.as_deref(),
                    );
                }
                lines.join("\n")
            }
            StructuredTaskOutput::OmnihumanIdentification { subject_status } => {
                format!("OmniHuman returned subject_status {subject_status}.")
            }
            StructuredTaskOutput::OmnihumanSubjectDetection { masks } => {
                let mut lines = vec![format!("OmniHuman found {} subject masks.", masks.len())];
                for mask in masks {
                    lines.push(format!("Subject mask {}", mask.index));
                    append_preview(
                        &mut lines,
                        mask.local_path.as_ref(),
                        mask.preview_error.as_deref(),
                    );
                }
                lines.join("\n")
            }
        };
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum StructuredTaskOutput {
    GrokSegmentMap {
        segments_count: usize,
        segments: Vec<SegmentMapItem>,
    },
    OmnihumanIdentification {
        subject_status: i64,
    },
    OmnihumanSubjectDetection {
        masks: Vec<SubjectMask>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SegmentMapItem {
    pub index: usize,
    pub name: String,
    #[serde(rename(deserialize = "maskUrl"))]
    pub mask_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubjectMask {
    pub index: usize,
    pub mask_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResultEnvelope<T> {
    #[serde(rename = "resultObject")]
    result_object: T,
}

#[derive(Debug, Deserialize)]
struct SegmentMapOutput {
    segments_count: usize,
    segments: Vec<SegmentMapItem>,
}

#[derive(Debug, Deserialize)]
struct IdentificationOutput {
    subject_status: i64,
}

#[derive(Debug, Deserialize)]
struct SubjectDetectionOutput {
    mask_urls: Vec<String>,
}

pub fn parse_structured_record(record: TaskRecord) -> Result<StructuredTaskResult, KieError> {
    let operation = StructuredOperation::from_model(&record.model).ok_or_else(|| {
        KieError::InvalidResponse {
            message: format!("task model {} is not a structured operation", record.model),
        }
    })?;
    let output = match operation {
        StructuredOperation::GrokSegmentMap => {
            let parsed: ResultEnvelope<SegmentMapOutput> = parse_result_json(&record)?;
            StructuredTaskOutput::GrokSegmentMap {
                segments_count: parsed.result_object.segments_count,
                segments: parsed.result_object.segments,
            }
        }
        StructuredOperation::OmnihumanIdentification => {
            let parsed: ResultEnvelope<IdentificationOutput> = parse_result_json(&record)?;
            StructuredTaskOutput::OmnihumanIdentification {
                subject_status: parsed.result_object.subject_status,
            }
        }
        StructuredOperation::OmnihumanSubjectDetection => {
            let parsed: ResultEnvelope<SubjectDetectionOutput> = parse_result_json(&record)?;
            StructuredTaskOutput::OmnihumanSubjectDetection {
                masks: parsed
                    .result_object
                    .mask_urls
                    .into_iter()
                    .enumerate()
                    .map(|(index, mask_url)| SubjectMask {
                        index,
                        mask_url,
                        local_path: None,
                        preview_error: None,
                    })
                    .collect(),
            }
        }
    };

    let mut result = StructuredTaskResult {
        task_id: record.task_id,
        model: record.model,
        state: record.state,
        output,
        markdown: String::new(),
    };
    result.refresh_markdown();
    Ok(result)
}

fn parse_result_json<T: for<'de> Deserialize<'de>>(record: &TaskRecord) -> Result<T, KieError> {
    serde_json::from_str(&record.result_json).map_err(|err| KieError::InvalidResponse {
        message: format!(
            "task {} returned malformed structured result JSON: {err}",
            record.task_id
        ),
    })
}

fn append_preview(lines: &mut Vec<String>, path: Option<&PathBuf>, error: Option<&str>) {
    if let Some(path) = path {
        lines.push(format!("![mask](<{}>)", path.display()));
    } else if let Some(error) = error {
        lines.push(format!("Preview unavailable: {}", one_line(error)));
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn record(model: &str, result_json: &str) -> TaskRecord {
        TaskRecord {
            task_id: "task_1".to_string(),
            model: model.to_string(),
            state: TaskState::Success,
            param_json: String::new(),
            result_json: result_json.to_string(),
            fail_code: String::new(),
            fail_msg: String::new(),
            cost_time: None,
            complete_time: None,
            create_time: None,
            update_time: None,
            progress: None,
            credits_consumed: None,
        }
    }

    #[test]
    fn parses_grok_segments_and_serializes_normalized_keys() {
        let result = parse_structured_record(record(
            GROK_SEGMENT_MAP_MODEL,
            r#"{"resultObject":{"segments_count":1,"segments":[{"maskUrl":"https://example.com/mask.png","name":"dog","index":0}]}}"#,
        ))
        .unwrap();
        let value = serde_json::to_value(result).unwrap();

        assert_eq!(value["operation"], json!("grok_segment_map"));
        assert_eq!(value["segments_count"], json!(1));
        assert_eq!(
            value["segments"][0]["mask_url"],
            json!("https://example.com/mask.png")
        );
        assert!(
            value["markdown"]
                .as_str()
                .unwrap()
                .contains("Segment 0: dog")
        );
        assert!(
            value["markdown"]
                .as_str()
                .unwrap()
                .contains("indexes of 1 or greater")
        );
    }

    #[test]
    fn parses_omnihuman_outputs_without_interpreting_status() {
        let identification = parse_structured_record(record(
            OMNIHUMAN_IDENTIFICATION_MODEL,
            r#"{"resultObject":{"subject_status":7}}"#,
        ))
        .unwrap();
        let value = serde_json::to_value(identification).unwrap();
        assert_eq!(value["subject_status"], json!(7));

        let detection = parse_structured_record(record(
            OMNIHUMAN_SUBJECT_DETECTION_MODEL,
            r#"{"resultObject":{"mask_urls":["https://example.com/one.png"]}}"#,
        ))
        .unwrap();
        let value = serde_json::to_value(detection).unwrap();
        assert_eq!(value["masks"][0]["index"], json!(0));
    }

    #[test]
    fn rejects_malformed_structured_results() {
        let err = parse_structured_record(record(GROK_SEGMENT_MAP_MODEL, "{}"))
            .expect_err("missing resultObject should fail");
        assert!(err.to_string().contains("malformed structured result JSON"));
    }
}
