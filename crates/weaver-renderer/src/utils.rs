use markdown_weaver::CowStr;
use miette::IntoDiagnostic;
use n0_future::TryFutureExt;
use std::{path::Path, sync::OnceLock};

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
use regex::Regex;
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use regex_lite::Regex;

use markdown_weaver::BrokenLink;
use std::path::PathBuf;
use std::sync::Arc;
use unicode_bidi::{get_base_direction, Direction};
use unicode_normalization::UnicodeNormalization;

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub async fn inline_file(path: impl AsRef<Path>) -> Option<String> {
    tokio::fs::read_to_string(path).await.ok()
}
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub async fn inline_file(path: impl AsRef<Path>) -> Option<String> {
    todo!()
}

pub const AVOID_URL_CHARS: &[char] = &[
    '!', '#', '$', '&', '\'', '(', ')', '*', '+', ',', ';', '=', ':', '@', '%', '[', ']', '?', '/',
    '~', '|', '{', '}', '^', '`',
];

pub fn resolve_at_ident_or_uri<'s>(
    link: &markdown_weaver::Tag<'s>,
    appview: &str,
) -> markdown_weaver::Tag<'s> {
    use markdown_weaver::Tag;
    match link {
        Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        } => {
            if dest_url.starts_with("at://") {
                // Make the appview string swappable
                let at_uri = weaver_common::aturi_to_http(dest_url.as_ref(), appview);
                if let Some(at_uri) = at_uri {
                    Tag::Link {
                        link_type: *link_type,
                        dest_url: at_uri.into_static(),
                        title: title.clone(),
                        id: id.clone(),
                    }
                } else {
                    link.clone()
                }
            } else if dest_url.starts_with("@") {
                let maybe_identifier = dest_url.strip_prefix("@").unwrap();
                if let Some(identifier) = weaver_common::match_identifier(maybe_identifier) {
                    let link = CowStr::Boxed(
                        format!("https://{}/profile/{}", appview, identifier).into_boxed_str(),
                    );
                    Tag::Link {
                        link_type: *link_type,
                        dest_url: link,
                        title: title.clone(),
                        id: id.clone(),
                    }
                } else {
                    link.clone()
                }
            } else {
                link.clone()
            }
        }
        _ => link.clone(),
    }
}

/// Rough and ready check if a path is a local path.
/// Basically checks if the path is absolute and if so, is it accessible.
/// Relative paths are assumed to be local, but URL schemes are not
pub fn is_local_path(path: &str) -> bool {
    // Check for URL schemes (http, https, at, etc.)
    if path.contains("://") {
        return false;
    }
    let path = Path::new(path);
    path.is_relative() || path.try_exists().unwrap_or(false)
}

/// Is this link relative to somewhere?
/// Rust has built-in checks for file paths, so this just wraps that.
pub fn is_relative_link(link: &str) -> bool {
    let path = Path::new(link);
    path.is_relative()
}

/// Flatten a directory path to just the parent and filename, if present.
/// Maybe worth to swap to using the Path tools, but this works.
pub fn flatten_dir_to_just_one_parent(path: &str) -> (&str, &str) {
    static RE_PARENT_DIR: OnceLock<Regex> = OnceLock::new();
    let caps = RE_PARENT_DIR
        .get_or_init(|| {
            Regex::new(r".*[/\\](?P<parent>[^/\\]+)[/\\](?P<filename>[^/\\]+)$").unwrap()
        })
        .captures(path);
    if let Some(caps) = caps {
        if let Some(parent) = caps.name("parent") {
            if let Some(filename) = caps.name("filename") {
                return (parent.as_str(), filename.as_str());
            }
            return (parent.as_str(), "");
        }
        if let Some(filename) = caps.name("filename") {
            return ("", filename.as_str());
        }
    }
    ("", path)
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
use tokio::fs::{self, File};

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub async fn create_file(dest: &Path) -> miette::Result<File> {
    let file = File::create(dest)
        .or_else(async |err| {
            {
                if err.kind() == std::io::ErrorKind::NotFound {
                    let parent = dest.parent().expect("file should have a parent directory");
                    fs::create_dir_all(parent).await?
                }
                File::create(dest)
            }
            .await
        })
        .await
        .into_diagnostic()?;
    Ok(file)
}

/// Path lookup in an Obsidian vault
///
/// Credit to https://github.com/zoni
///
/// Taken from https://github.com/zoni/obsidian-export/blob/main/src/lib.rs.rs on 2025-05-21
///
pub fn lookup_filename_in_vault<'a>(
    filename: &str,
    vault_contents: &'a [PathBuf],
) -> Option<&'a PathBuf> {
    let filename = PathBuf::from(filename);
    let filename_normalized: String = filename.to_string_lossy().nfc().collect();

    vault_contents.iter().find(|path| {
        let path_normalized_str: String = path.to_string_lossy().nfc().collect();
        let path_normalized = PathBuf::from(&path_normalized_str);
        let path_normalized_lowered = PathBuf::from(&path_normalized_str.to_lowercase());

        // It would be convenient if we could just do `filename.set_extension("md")` at the start
        // of this funtion so we don't need multiple separate + ".md" match cases here, however
        // that would break with a reference of `[[Note.1]]` linking to `[[Note.1.md]]`.

        path_normalized.ends_with(&filename_normalized)
            || path_normalized.ends_with(filename_normalized.clone() + ".md")
            || path_normalized_lowered.ends_with(filename_normalized.to_lowercase())
            || path_normalized_lowered.ends_with(filename_normalized.to_lowercase() + ".md")
    })
}

pub struct VaultBrokenLinkCallback {
    pub vault_contents: Arc<[PathBuf]>,
}

impl<'input> markdown_weaver::BrokenLinkCallback<'input> for VaultBrokenLinkCallback {
    fn handle_broken_link(
        &mut self,
        link: BrokenLink<'input>,
    ) -> Option<(CowStr<'input>, CowStr<'input>)> {
        let text = link.reference;
        let captures = crate::OBSIDIAN_NOTE_LINK_RE
            .captures(&text)
            .expect("note link regex didn't match - bad input?");
        let file = captures.name("file").map(|v| v.as_str().trim());
        let label = captures.name("label").map(|v| v.as_str());
        let section = captures.name("section").map(|v| v.as_str().trim());

        if let Some(file) = file {
            if let Some(path) = lookup_filename_in_vault(file, self.vault_contents.as_ref()) {
                let mut link_text = String::from(path.to_string_lossy());
                if let Some(section) = section {
                    link_text.push('#');
                    link_text.push_str(section);
                    if let Some(label) = label {
                        let label = label.to_string();
                        Some((CowStr::from(link_text), CowStr::from(label)))
                    } else {
                        Some((link_text.into(), format!("{} > {}", file, section).into()))
                    }
                } else {
                    Some((link_text.into(), format!("{}", file).into()))
                }
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Detect text direction from first strong directional character.
/// Returns Some("rtl") for Hebrew/Arabic/etc, Some("ltr") for Latin, None if no strong char found.
pub fn detect_text_direction(text: &str) -> Option<&'static str> {
    match get_base_direction(text) {
        Direction::Ltr => Some("ltr"),
        Direction::Rtl => Some("rtl"),
        Direction::Mixed => None, // neutral/unknown - let browser decide
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_text_direction_ltr() {
        assert_eq!(detect_text_direction("Hello World"), Some("ltr"));
        assert_eq!(detect_text_direction("Привет мир"), Some("ltr"));
        assert_eq!(detect_text_direction("你好世界"), Some("ltr"));
        assert_eq!(detect_text_direction("Γειά σου κόσμε"), Some("ltr"));
    }

    #[test]
    fn test_detect_text_direction_rtl() {
        // Hebrew
        assert_eq!(detect_text_direction("שלום עולם"), Some("rtl"));
        // Arabic
        assert_eq!(detect_text_direction("مرحبا بالعالم"), Some("rtl"));
        // Mixed with leading whitespace and punctuation
        assert_eq!(detect_text_direction("   123... שלום"), Some("rtl"));
        assert_eq!(detect_text_direction("   456!!! مرحبا"), Some("rtl"));
    }

    #[test]
    fn test_detect_text_direction_neutral_only() {
        assert_eq!(detect_text_direction("   "), None);
        assert_eq!(detect_text_direction("123456"), None);
        assert_eq!(detect_text_direction("!!!..."), None);
        assert_eq!(detect_text_direction(""), None);
    }

    #[test]
    fn test_detect_text_direction_leading_neutrals() {
        assert_eq!(detect_text_direction("   123... Hello"), Some("ltr"));
        assert_eq!(detect_text_direction("!!!456 שלום"), Some("rtl"));
    }
}
