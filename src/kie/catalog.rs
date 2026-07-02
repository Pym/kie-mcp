use serde::{Serialize, Serializer};

use super::jobs::GenerationKind;

mod models;

pub const CATALOG_SOURCE: &str = "https://docs.kie.ai/llms.txt";

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub kind: GenerationKind,
    pub url_binding: UrlBinding,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio_field: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_field: Option<&'static str>,
    pub output_format: OutputFormatStyle,
    #[serde(skip_serializing_if = "<[&str]>::is_empty")]
    pub aliases: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct RequestProfile {
    url_binding: UrlBinding,
    aspect_ratio_field: Option<&'static str>,
    resolution_field: Option<&'static str>,
    output_format: OutputFormatStyle,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UrlBinding {
    None,
    Scalar {
        field: &'static str,
    },
    Array {
        field: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_items: Option<usize>,
    },
    FirstLastFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormatStyle {
    None,
    Jpg,
    Jpeg,
}

impl Serialize for OutputFormatStyle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::None => "none",
            Self::Jpg => "jpg_png",
            Self::Jpeg => "jpeg_png",
        })
    }
}

impl ModelSpec {
    pub fn field_for_convenience(&self, convenience: ConvenienceField) -> Option<&'static str> {
        match convenience {
            ConvenienceField::AspectRatio => self.aspect_ratio_field,
            ConvenienceField::Resolution => self.resolution_field,
            ConvenienceField::OutputFormat => match self.output_format {
                OutputFormatStyle::None => None,
                _ => Some("output_format"),
            },
        }
    }

    pub fn output_format_value(&self, value: &str) -> Option<String> {
        let lowered = value.to_ascii_lowercase();
        match self.output_format {
            OutputFormatStyle::None => None,
            OutputFormatStyle::Jpg => match lowered.as_str() {
                "jpg" | "jpeg" => Some("jpg".to_string()),
                "png" => Some("png".to_string()),
                _ => None,
            },
            OutputFormatStyle::Jpeg => match lowered.as_str() {
                "jpg" | "jpeg" => Some("jpeg".to_string()),
                "png" => Some("png".to_string()),
                _ => None,
            },
        }
    }

    pub fn input_summary(&self) -> &'static str {
        match self.url_binding {
            UrlBinding::None => "model-specific input",
            UrlBinding::Scalar { field } => field,
            UrlBinding::Array { field, .. } => field,
            UrlBinding::FirstLastFrame => "first_frame_url/last_frame_url",
        }
    }

    pub fn convenience_summary(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if let Some(field) = self.aspect_ratio_field {
            fields.push(field);
        }
        if let Some(field) = self.resolution_field {
            fields.push(field);
        }
        if self.output_format != OutputFormatStyle::None {
            fields.push("output_format");
        }
        fields
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ConvenienceField {
    AspectRatio,
    Resolution,
    OutputFormat,
}

pub fn model_catalog() -> &'static [ModelSpec] {
    models::MODELS
}

pub fn models_for(kind: Option<GenerationKind>, query: Option<&str>) -> Vec<&'static ModelSpec> {
    let normalized_query = query.map(normalize_key);
    let models = model_catalog()
        .iter()
        .filter(|model| kind.is_none_or(|expected| model.kind == expected))
        .collect::<Vec<_>>();

    let Some(query) = normalized_query.as_deref() else {
        return models;
    };
    if query.is_empty() {
        return models;
    }

    let exact = models
        .iter()
        .copied()
        .filter(|model| model_exact_match(model, query))
        .collect::<Vec<_>>();
    if !exact.is_empty() {
        return exact;
    }

    models
        .into_iter()
        .filter(|model| model_contains_match(model, query))
        .collect()
}

pub fn resolve_model(model: &str, expected: GenerationKind) -> Option<&'static ModelSpec> {
    resolve_model_with_kind(model, Some(expected))
}

pub fn resolve_model_any_kind(model: &str) -> Option<&'static ModelSpec> {
    resolve_model_with_kind(model, None)
}

pub fn has_catalog_match(model: &str) -> bool {
    let normalized = normalize_key(model);
    !normalized.is_empty()
        && model_catalog().iter().any(|spec| {
            model_exact_match(spec, &normalized) || model_contains_match(spec, &normalized)
        })
}

fn resolve_model_with_kind(
    model: &str,
    expected: Option<GenerationKind>,
) -> Option<&'static ModelSpec> {
    let normalized = normalize_key(model);
    let mut exact = model_catalog()
        .iter()
        .filter(|spec| expected.is_none_or(|kind| spec.kind == kind))
        .filter(|spec| model_exact_match(spec, &normalized));
    if let Some(first) = exact.next() {
        return exact.next().is_none().then_some(first);
    }

    let mut fuzzy = model_catalog()
        .iter()
        .filter(|spec| expected.is_none_or(|kind| spec.kind == kind))
        .filter(|spec| model_contains_match(spec, &normalized));
    let first = fuzzy.next()?;
    fuzzy.next().is_none().then_some(first)
}

pub fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn model_exact_match(model: &ModelSpec, normalized_query: &str) -> bool {
    normalize_key(model.id) == normalized_query
        || normalize_key(model.display_name) == normalized_query
        || model
            .aliases
            .iter()
            .any(|alias| normalize_key(alias) == normalized_query)
}

fn model_contains_match(model: &ModelSpec, normalized_query: &str) -> bool {
    !normalized_query.is_empty()
        && (normalize_key(model.id).contains(normalized_query)
            || normalize_key(model.display_name).contains(normalized_query)
            || model
                .aliases
                .iter()
                .any(|alias| normalize_key(alias).contains(normalized_query)))
}

const I: GenerationKind = GenerationKind::Image;
const V: GenerationKind = GenerationKind::Video;
const NO_FIELD: Option<&str> = None;
const AR: Option<&str> = Some("aspect_ratio");
const RATIO: Option<&str> = Some("ratio");
const RES: Option<&str> = Some("resolution");
const IMG_RES: Option<&str> = Some("image_resolution");
const OUT_RES: Option<&str> = Some("output_resolution");
const OF_NONE: OutputFormatStyle = OutputFormatStyle::None;
const OF_JPG: OutputFormatStyle = OutputFormatStyle::Jpg;
const OF_JPEG: OutputFormatStyle = OutputFormatStyle::Jpeg;

const fn un() -> UrlBinding {
    UrlBinding::None
}
const fn us(field: &'static str) -> UrlBinding {
    UrlBinding::Scalar { field }
}
const fn ua(field: &'static str, max_items: Option<usize>) -> UrlBinding {
    UrlBinding::Array { field, max_items }
}
const fn ufr() -> UrlBinding {
    UrlBinding::FirstLastFrame
}
const fn profile(
    url_binding: UrlBinding,
    aspect_ratio_field: Option<&'static str>,
    resolution_field: Option<&'static str>,
    output_format: OutputFormatStyle,
) -> RequestProfile {
    RequestProfile {
        url_binding,
        aspect_ratio_field,
        resolution_field,
        output_format,
    }
}
const fn model(
    id: &'static str,
    display_name: &'static str,
    kind: GenerationKind,
    profile: RequestProfile,
    aliases: &'static [&'static str],
) -> ModelSpec {
    ModelSpec {
        id,
        display_name,
        kind,
        url_binding: profile.url_binding,
        aspect_ratio_field: profile.aspect_ratio_field,
        resolution_field: profile.resolution_field,
        output_format: profile.output_format,
        aliases,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_catalog_covers_market_image_video_models() {
        assert!(model_catalog().len() >= 100);
        assert!(
            model_catalog()
                .iter()
                .all(|model| !model.id.contains("subject-detection"))
        );
    }

    #[test]
    fn resolves_nano_banana_2_human_names() {
        let spaced = resolve_model("Nano Banana 2", GenerationKind::Image).unwrap();
        assert_eq!(spaced.id, "nano-banana-2");
        let compact = resolve_model("NanoBanana2", GenerationKind::Image).unwrap();
        assert_eq!(compact.id, "nano-banana-2");
    }

    #[test]
    fn nano_banana_2_profile_contains_only_request_construction_data() {
        let nano = resolve_model("Nano Banana 2", GenerationKind::Image).unwrap();
        assert_eq!(
            nano.url_binding,
            UrlBinding::Array {
                field: "image_input",
                max_items: Some(14)
            }
        );
        assert_eq!(nano.aspect_ratio_field, Some("aspect_ratio"));
        assert_eq!(nano.resolution_field, Some("resolution"));
        assert_eq!(nano.output_format, OutputFormatStyle::Jpg);
    }

    #[test]
    fn ambiguous_fuzzy_model_names_do_not_resolve() {
        assert!(resolve_model("banana", GenerationKind::Image).is_none());
        assert!(!models_for(Some(GenerationKind::Image), Some("banana")).is_empty());
        assert!(resolve_model_any_kind("banana").is_none());
        assert!(has_catalog_match("banana"));
    }

    #[test]
    fn resolves_known_models_without_expected_kind() {
        let image = resolve_model_any_kind("wan/2-7-image").unwrap();
        assert_eq!(image.kind, GenerationKind::Image);

        let video = resolve_model_any_kind("gemini-omni-video").unwrap();
        assert_eq!(video.kind, GenerationKind::Video);
    }

    #[test]
    fn exact_catalog_keys_are_unique() {
        let mut seen = std::collections::BTreeMap::new();
        for model in model_catalog() {
            for key in std::iter::once(model.id)
                .chain(std::iter::once(model.display_name))
                .chain(model.aliases.iter().copied())
            {
                let normalized = normalize_key(key);
                assert!(
                    !normalized.is_empty(),
                    "empty normalized key for {}",
                    model.id
                );
                let previous = seen.insert(normalized, model.id);
                assert!(
                    previous.is_none_or(|previous| previous == model.id),
                    "duplicate normalized catalog key between {} and {}",
                    previous.unwrap_or(model.id),
                    model.id
                );
            }
        }
    }
}
