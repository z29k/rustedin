//! Media inputs: a public URL the platform will fetch, or a local file
//! rustedin uploads.
//!
//! The distinction matters most on Instagram, whose publishing API only ever
//! accepts URLs for images — see [`crate::providers::meta::instagram`] for how
//! local files are relayed through the linked Facebook Page. LinkedIn is the
//! mirror image: it only ever accepts bytes, so a URL has to be downloaded
//! first with [`MediaSource::fetch_bytes`].

use crate::core::http;
use std::path::{Path, PathBuf};

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "m4v", "avi", "mkv", "webm", "mpg", "mpeg", "3gp", "wmv", "flv",
];
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "heic", "bmp", "tif", "tiff",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSource {
    /// A publicly reachable URL. Most platforms download it server-side.
    Url(String),
    /// A file on this machine. rustedin uploads the bytes.
    File(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

impl MediaSource {
    /// Anything starting with `http://` or `https://` is a URL; everything else
    /// is a path, which must exist.
    pub fn parse(input: &str) -> Result<Self, String> {
        if input.starts_with("http://") || input.starts_with("https://") {
            return Ok(MediaSource::Url(input.to_string()));
        }
        let path = PathBuf::from(input);
        if !path.exists() {
            return Err(format!(
                "Media not found: {input}\n\
                 Pass a local file path or a public http(s) URL."
            ));
        }
        if !path.is_file() {
            return Err(format!("Media path is not a file: {input}"));
        }
        Ok(MediaSource::File(path))
    }

    pub fn as_str(&self) -> String {
        match self {
            MediaSource::Url(u) => u.clone(),
            MediaSource::File(p) => p.display().to_string(),
        }
    }

    pub fn is_url(&self) -> bool {
        matches!(self, MediaSource::Url(_))
    }

    /// File name to advertise in a multipart upload.
    pub fn file_name(&self) -> String {
        match self {
            MediaSource::File(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "upload".to_string()),
            MediaSource::Url(u) => url_path(u)
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("upload")
                .to_string(),
        }
    }

    pub fn extension(&self) -> Option<String> {
        let raw = match self {
            MediaSource::File(p) => p.extension().map(|e| e.to_string_lossy().into_owned()),
            MediaSource::Url(u) => Path::new(url_path(u))
                .extension()
                .map(|e| e.to_string_lossy().into_owned()),
        };
        raw.map(|e| e.to_lowercase())
    }

    /// Image or video, inferred from the extension.
    ///
    /// Returns `None` for extension-less URLs (common with CDNs and signed
    /// links) so callers can ask the user instead of guessing wrong.
    pub fn kind(&self) -> Option<MediaKind> {
        let ext = self.extension()?;
        if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            Some(MediaKind::Video)
        } else if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            Some(MediaKind::Image)
        } else {
            None
        }
    }

    /// [`Self::kind`], or an error naming the flag that resolves the ambiguity.
    pub fn require_kind(&self) -> Result<MediaKind, String> {
        self.kind().ok_or_else(|| {
            format!(
                "Cannot tell whether \"{}\" is an image or a video from its extension.\n\
                 Pass --media-type=image or --media-type=video.",
                self.as_str()
            )
        })
    }

    /// Read a local file. Errors on a URL — callers must not download media the
    /// platform can fetch itself.
    pub fn read_bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            MediaSource::File(p) => {
                std::fs::read(p).map_err(|e| format!("Failed to read {}: {e}", p.display()))
            }
            MediaSource::Url(u) => Err(format!("{u} is a URL, not a local file")),
        }
    }

    /// Bytes, wherever they live: read from disk, or downloaded.
    ///
    /// For platforms whose upload endpoint takes the bytes and nothing else —
    /// LinkedIn's image upload — so `--image` accepts a URL just the same.
    pub async fn fetch_bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            MediaSource::File(_) => self.read_bytes(),
            MediaSource::Url(url) => {
                let context = format!("GET {url}");
                let res = http::send(|| http::client().get(url), &context, &http::Retry::reads())
                    .await
                    .map_err(|e| format!("Failed to download {url}: {e}"))?;
                Ok(res.bytes)
            }
        }
    }

    /// Size in bytes for a local file; `None` for a URL.
    pub fn size(&self) -> Option<u64> {
        match self {
            MediaSource::File(p) => std::fs::metadata(p).ok().map(|m| m.len()),
            MediaSource::Url(_) => None,
        }
    }

    /// Fail when a local file exceeds what the endpoint accepts.
    pub fn check_size(&self, limit_bytes: u64, what: &str) -> Result<(), String> {
        match self.size() {
            Some(size) if size > limit_bytes => Err(format!(
                "{} is {:.1} MB — {what} allows at most {:.0} MB.",
                self.as_str(),
                size as f64 / (1024.0 * 1024.0),
                limit_bytes as f64 / (1024.0 * 1024.0),
            )),
            _ => Ok(()),
        }
    }
}

/// The path portion of a URL, without query string or fragment.
fn url_path(url: &str) -> &str {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path = after_scheme.split_once('/').map(|(_, p)| p).unwrap_or("");
    path.split(['?', '#']).next().unwrap_or("")
}

pub fn human_size(bytes: Option<u64>) -> String {
    match bytes {
        Some(b) if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        Some(b) => format!("{:.1} KB", b as f64 / 1024.0),
        None => "remote".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_detected() {
        let m = MediaSource::parse("https://cdn.example.com/a.jpg").unwrap();
        assert!(m.is_url());
        assert_eq!(m.kind(), Some(MediaKind::Image));
        assert_eq!(m.file_name(), "a.jpg");
    }

    #[test]
    fn query_strings_do_not_confuse_extension_detection() {
        let m = MediaSource::parse("https://cdn.example.com/clip.mp4?sig=abc.jpg").unwrap();
        assert_eq!(m.extension().as_deref(), Some("mp4"));
        assert_eq!(m.kind(), Some(MediaKind::Video));
    }

    #[test]
    fn extensionless_urls_are_ambiguous() {
        let m = MediaSource::parse("https://cdn.example.com/asset/98213").unwrap();
        assert_eq!(m.kind(), None);
        let err = m.require_kind().unwrap_err();
        assert!(err.contains("--media-type"));
    }

    #[test]
    fn missing_local_files_are_rejected_early() {
        let err = MediaSource::parse("/definitely/not/here.jpg").unwrap_err();
        assert!(err.contains("Media not found"));
    }

    #[test]
    fn local_files_are_read_as_paths() {
        let dir = std::env::temp_dir().join("rustedin-media-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("photo.JPEG");
        std::fs::write(&file, b"xx").unwrap();

        let m = MediaSource::parse(file.to_str().unwrap()).unwrap();
        assert!(!m.is_url());
        assert_eq!(m.extension().as_deref(), Some("jpeg"));
        assert_eq!(m.kind(), Some(MediaKind::Image));
        assert_eq!(m.file_name(), "photo.JPEG");
        assert_eq!(m.size(), Some(2));
        assert!(m.check_size(1, "Instagram").is_err());
        assert!(m.check_size(10, "Instagram").is_ok());

        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn url_size_checks_are_skipped() {
        let m = MediaSource::parse("https://cdn.example.com/huge.mp4").unwrap();
        assert_eq!(m.size(), None);
        assert!(m.check_size(1, "Instagram").is_ok());
    }

    #[test]
    fn url_path_strips_host_query_and_fragment() {
        assert_eq!(url_path("https://h.tld/a/b.jpg?x=1#y"), "a/b.jpg");
        assert_eq!(url_path("https://h.tld"), "");
    }

    #[test]
    fn human_size_switches_units() {
        assert_eq!(human_size(Some(2048)), "2.0 KB");
        assert_eq!(human_size(Some(3 * 1024 * 1024)), "3.0 MB");
        assert_eq!(human_size(None), "remote");
    }
}
