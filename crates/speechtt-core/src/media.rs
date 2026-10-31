//! Which files may be offered as transcription input.
//!
//! The native file dialog cannot be restricted to a list of file types: neither
//! `gpui-kit`'s `PathPromptOptions` nor the Linux portal call it makes accepts
//! extension filters. So the check happens on the chosen path, before any take
//! exists, and this module is the single source of truth for it.
//!
//! It is deliberately a cheap, honest first gate rather than a claim about the
//! file's contents: `ffmpeg` stays the authority for a file whose bytes are not
//! what its extension says, and that case keeps failing through the worker's
//! `unsupported_file` / `decode_failed` path.

use std::path::Path;

/// Extensions accepted for file transcription, lowercase, without the dot.
///
/// Audio first, then video, then the container formats that carry both.
pub const MEDIA_EXTENSIONS: &[&str] = &[
    // Audio
    "wav", "mp3", "m4a", "aac", "flac", "ogg", "oga", "opus", "wma", "aiff", "aif", "amr", "mp2",
    // Video
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpeg", "mpg", "3gp", "3g2", "ts",
    "mts", "m2ts", "ogv",
];

/// True when `extension` (with or without a leading dot) is accepted.
///
/// Case-insensitive. An empty string is never accepted.
#[must_use]
pub fn is_media_extension(extension: &str) -> bool {
    let extension = extension.strip_prefix('.').unwrap_or(extension);
    if extension.is_empty() {
        return false;
    }
    MEDIA_EXTENSIONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(extension))
}

/// True when `path` carries an accepted media extension.
///
/// A missing extension, an empty one, or a trailing dot is not media. Only the
/// last extension counts, so `archive.tar.mp3` is accepted and `song.mp3.bak`
/// is not. No I/O: the path string is all that is read.
#[must_use]
pub fn is_media_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(is_media_extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_extension_is_accepted() {
        for extension in MEDIA_EXTENSIONS {
            assert!(is_media_extension(extension), "{extension}");
        }
    }

    #[test]
    fn matching_is_case_insensitive_and_accepts_a_leading_dot() {
        for candidate in ["MP4", "Mp4", "mp4", ".mp4", ".MP4"] {
            assert!(is_media_extension(candidate), "{candidate}");
        }
        assert!(is_media_extension("WAV"), "audio too");
    }

    #[test]
    fn non_media_extensions_are_rejected() {
        for candidate in ["pdf", "txt", "md", "zip", "exe", "json", "png", "csv"] {
            assert!(!is_media_extension(candidate), "{candidate}");
        }
    }

    #[test]
    fn an_empty_or_dot_only_extension_is_rejected() {
        assert!(!is_media_extension(""));
        assert!(!is_media_extension("."));
    }

    #[test]
    fn paths_use_their_last_extension_only() {
        assert!(is_media_path(Path::new("/tmp/archive.tar.mp3")));
        assert!(is_media_path(Path::new("interview.MP4")));
        assert!(!is_media_path(Path::new("/tmp/song.mp3.bak")));
        assert!(!is_media_path(Path::new("/tmp/report.pdf")));
    }

    #[test]
    fn a_path_without_an_extension_is_not_media() {
        assert!(!is_media_path(Path::new("/tmp/recording")));
        assert!(!is_media_path(Path::new("/tmp/trailing.")));
        assert!(!is_media_path(Path::new("/tmp")));
        assert!(!is_media_path(Path::new("")));
    }

    #[test]
    fn a_non_ascii_path_does_not_panic() {
        assert!(is_media_path(Path::new("/tmp/réunion-été.wav")));
        assert!(!is_media_path(Path::new("/tmp/ドキュメント.pdf")));
        assert!(!is_media_extension("✈"));
    }

    /// The list is a contract: changing it must be a deliberate edit here too.
    #[test]
    fn the_allowlist_is_what_the_ticket_specified() {
        assert_eq!(
            MEDIA_EXTENSIONS,
            &[
                "wav", "mp3", "m4a", "aac", "flac", "ogg", "oga", "opus", "wma", "aiff", "aif",
                "amr", "mp2", "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpeg",
                "mpg", "3gp", "3g2", "ts", "mts", "m2ts", "ogv",
            ]
        );
    }
}
