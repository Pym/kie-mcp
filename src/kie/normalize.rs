use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize)]
pub struct MediaUrls {
    pub result_urls: Vec<String>,
    pub poster_urls: Vec<String>,
}

pub fn extract_media_urls(result_json: &str) -> MediaUrls {
    let Ok(value) = serde_json::from_str::<Value>(result_json) else {
        return MediaUrls::default();
    };

    let mut urls = Vec::new();
    let mut posters = Vec::new();
    let mut image_fallbacks = Vec::new();
    let mut generic_fallbacks = Vec::new();

    collect_array(
        &value,
        &["resultUrls", "urls", "video_urls", "image_urls"],
        &mut urls,
    );
    collect_scalar(
        &value,
        &[
            "videoUrl",
            "video_url",
            "resultImageUrl",
            "result_image_url",
        ],
        &mut urls,
    );
    collect_scalar(&value, &["url"], &mut generic_fallbacks);
    collect_array(&value, &["firstFrameUrl", "lastFrameUrl"], &mut posters);
    collect_scalar(
        &value,
        &[
            "coverUrl",
            "cover_url",
            "firstFrameUrl",
            "first_frame_url",
            "lastFrameUrl",
            "last_frame_url",
            "posterUrl",
            "poster_url",
            "thumbnailUrl",
            "thumbnail_url",
            "imageUrl",
            "image_url",
        ],
        &mut posters,
    );
    collect_scalar(&value, &["imageUrl", "image_url"], &mut image_fallbacks);

    dedupe(&mut urls);
    if urls.is_empty() {
        urls.append(&mut image_fallbacks);
        dedupe(&mut urls);
    }
    if urls.is_empty() {
        urls.append(&mut generic_fallbacks);
        dedupe(&mut urls);
    }
    dedupe(&mut posters);
    posters.retain(|poster| !urls.contains(poster));
    MediaUrls {
        result_urls: urls,
        poster_urls: posters,
    }
}

fn collect_array(value: &Value, keys: &[&str], out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(Value::Array(items)) = map.get(*key) {
                    out.extend(
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string)),
                    );
                }
            }
            for child in map.values() {
                collect_array(child, keys, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_array(child, keys, out);
            }
        }
        _ => {}
    }
}

fn collect_scalar(value: &Value, keys: &[&str], out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(url) = map.get(*key).and_then(Value::as_str) {
                    out.push(url.to_string());
                }
            }
            for child in map.values() {
                collect_scalar(child, keys, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_scalar(child, keys, out);
            }
        }
        _ => {}
    }
}

fn dedupe(items: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_nested_media_urls() {
        let media = extract_media_urls(
            r#"{"resultUrls":["https://x/a.png"],"videoInfo":{"videoUrl":"https://x/v.mp4","imageUrl":"https://x/p.png"}}"#,
        );
        assert_eq!(
            media.result_urls,
            vec!["https://x/a.png", "https://x/v.mp4"]
        );
        assert_eq!(media.poster_urls, vec!["https://x/p.png"]);
    }

    #[test]
    fn treats_image_url_as_result_when_it_is_the_only_media() {
        let media = extract_media_urls(r#"{"imageUrl":"https://x/image.png"}"#);

        assert_eq!(media.result_urls, vec!["https://x/image.png"]);
        assert!(media.poster_urls.is_empty());
    }

    #[test]
    fn ignores_generic_url_when_specific_result_url_exists() {
        let media = extract_media_urls(
            r#"{"url":"https://x/input.png","resultUrls":["https://x/generated.png"]}"#,
        );

        assert_eq!(media.result_urls, vec!["https://x/generated.png"]);
    }

    #[test]
    fn treats_generic_url_as_last_resort_result() {
        let media = extract_media_urls(r#"{"url":"https://x/generated.png"}"#);

        assert_eq!(media.result_urls, vec!["https://x/generated.png"]);
    }
}
