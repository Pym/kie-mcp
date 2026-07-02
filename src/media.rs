use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::kie::jobs::GenerationKind;

const MAX_SAFE_STEM_LEN: usize = 120;

#[derive(Debug, Clone, Serialize)]
pub struct SavedMedia {
    pub source_url: String,
    pub path: PathBuf,
    pub kind: String,
}

pub fn safe_stem(input: Option<&str>, fallback: &str) -> String {
    let candidate = input.unwrap_or(fallback);
    let mut out = String::with_capacity(candidate.len());
    for ch in candidate.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            out.push(ch);
        } else if matches!(ch, ' ' | '.' | '/' | '\\') || ch.is_whitespace() {
            out.push('-');
        } else if let Some(replacement) = ascii_transliteration(ch) {
            out.push_str(replacement);
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        fallback.chars().take(MAX_SAFE_STEM_LEN).collect()
    } else {
        trimmed.chars().take(MAX_SAFE_STEM_LEN).collect()
    }
}

fn ascii_transliteration(ch: char) -> Option<&'static str> {
    match ch {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' | 'Ǎ' => Some("A"),
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ǎ' | 'ª' => Some("a"),
        'Æ' => Some("AE"),
        'æ' => Some("ae"),
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => Some("C"),
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => Some("c"),
        'Ð' | 'Ď' | 'Đ' => Some("D"),
        'ð' | 'ď' | 'đ' => Some("d"),
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => Some("E"),
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => Some("e"),
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' => Some("G"),
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => Some("g"),
        'Ĥ' | 'Ħ' => Some("H"),
        'ĥ' | 'ħ' => Some("h"),
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' | 'Ǐ' => Some("I"),
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' | 'ǐ' => Some("i"),
        'Ĵ' => Some("J"),
        'ĵ' => Some("j"),
        'Ķ' => Some("K"),
        'ķ' => Some("k"),
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ŀ' | 'Ł' => Some("L"),
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => Some("l"),
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' => Some("N"),
        'ñ' | 'ń' | 'ņ' | 'ň' => Some("n"),
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' | 'Ǒ' => Some("O"),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'ǒ' | 'º' => Some("o"),
        'Œ' => Some("OE"),
        'œ' => Some("oe"),
        'Ŕ' | 'Ŗ' | 'Ř' => Some("R"),
        'ŕ' | 'ŗ' | 'ř' => Some("r"),
        'Ś' | 'Ŝ' | 'Ş' | 'Š' | 'Ș' => Some("S"),
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => Some("s"),
        'ß' => Some("ss"),
        'Ţ' | 'Ť' | 'Ŧ' | 'Ț' => Some("T"),
        'ţ' | 'ť' | 'ŧ' | 'ț' => Some("t"),
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' | 'Ǔ' => Some("U"),
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ǔ' => Some("u"),
        'Ŵ' => Some("W"),
        'ŵ' => Some("w"),
        'Ý' | 'Ŷ' | 'Ÿ' => Some("Y"),
        'ý' | 'ÿ' | 'ŷ' => Some("y"),
        'Ź' | 'Ż' | 'Ž' => Some("Z"),
        'ź' | 'ż' | 'ž' => Some("z"),
        _ => None,
    }
}

pub fn file_extension_from_url(
    url: &str,
    kind: GenerationKind,
    content_type: Option<&str>,
) -> String {
    if let Some(ext) = content_type.and_then(|value| media_type_extension(value, kind)) {
        return ext.to_string();
    }

    if let Ok(parsed) = url::Url::parse(url)
        && let Some(ext) = Path::new(parsed.path()).extension().and_then(OsStr::to_str)
    {
        let clean = ext
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>();
        let clean = clean.to_ascii_lowercase();
        if extension_matches_kind(&clean, kind) {
            return clean;
        }
    }

    default_extension(kind).to_string()
}

fn media_type_extension(content_type: &str, kind: GenerationKind) -> Option<&'static str> {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match (kind, media_type.as_str()) {
        (GenerationKind::Image, "image/avif") => Some("avif"),
        (GenerationKind::Image, "image/bmp") => Some("bmp"),
        (GenerationKind::Image, "image/gif") => Some("gif"),
        (GenerationKind::Image, "image/jpeg") => Some("jpg"),
        (GenerationKind::Image, "image/png") => Some("png"),
        (GenerationKind::Image, "image/tiff") => Some("tiff"),
        (GenerationKind::Image, "image/webp") => Some("webp"),
        (GenerationKind::Video, "video/mp4") => Some("mp4"),
        (GenerationKind::Video, "video/quicktime") => Some("mov"),
        (GenerationKind::Video, "video/webm") => Some("webm"),
        _ => None,
    }
}

fn extension_matches_kind(extension: &str, kind: GenerationKind) -> bool {
    match kind {
        GenerationKind::Image => matches!(
            extension,
            "avif" | "bmp" | "gif" | "jpeg" | "jpg" | "png" | "tif" | "tiff" | "webp"
        ),
        GenerationKind::Video => {
            matches!(extension, "avi" | "m4v" | "mkv" | "mov" | "mp4" | "webm")
        }
    }
}

fn default_extension(kind: GenerationKind) -> &'static str {
    match kind {
        GenerationKind::Image => "png",
        GenerationKind::Video => "mp4",
    }
}

pub fn preview_markdown(media: &[SavedMedia], posters: &[SavedMedia]) -> String {
    let mut lines = Vec::new();
    for item in media {
        let path = markdown_path(&item.path);
        if item.kind == "image" {
            lines.push(format!("![image]({path})"));
        } else {
            lines.push(format!("[video]({path})"));
        }
    }
    for poster in posters {
        lines.push(format!("![poster]({})", markdown_path(&poster.path)));
    }
    lines.join("\n")
}

fn markdown_path(path: &Path) -> String {
    format!("<{}>", path.display())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{SavedMedia, file_extension_from_url, preview_markdown, safe_stem};
    use crate::kie::jobs::GenerationKind;

    #[test]
    fn safe_stem_transliterates_latin_accents() {
        assert_eq!(
            safe_stem(Some("été à Montréal"), "task_123"),
            "ete-a-Montreal"
        );
    }

    #[test]
    fn safe_stem_falls_back_when_nothing_safe_remains() {
        assert_eq!(safe_stem(Some("東京"), "task_123"), "task_123");
    }

    #[test]
    fn safe_stem_limits_filesystem_component_length() {
        assert_eq!(safe_stem(Some(&"a".repeat(200)), "task").len(), 120);
    }

    #[test]
    fn file_extension_prefers_compatible_content_type() {
        assert_eq!(
            file_extension_from_url(
                "https://example.com/generated.php",
                GenerationKind::Image,
                Some("image/jpeg; charset=binary")
            ),
            "jpg"
        );
        assert_eq!(
            file_extension_from_url(
                "https://example.com/generated.png",
                GenerationKind::Video,
                Some("video/mp4")
            ),
            "mp4"
        );
    }

    #[test]
    fn file_extension_rejects_suffixes_for_the_wrong_media_kind() {
        assert_eq!(
            file_extension_from_url(
                "https://example.com/generated.mp4",
                GenerationKind::Image,
                None
            ),
            "png"
        );
        assert_eq!(
            file_extension_from_url(
                "https://example.com/generated.html",
                GenerationKind::Video,
                None
            ),
            "mp4"
        );
    }

    #[test]
    fn preview_markdown_wraps_paths_in_angle_brackets() {
        let media = [SavedMedia {
            source_url: "https://example.com/image.png".to_string(),
            path: PathBuf::from("/tmp/kie output/image (final).png"),
            kind: "image".to_string(),
        }];

        assert_eq!(
            preview_markdown(&media, &[]),
            "![image](</tmp/kie output/image (final).png>)"
        );
    }
}
