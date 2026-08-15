// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Small, pure helpers shared across `core` and `cli::commands`: filename
//! safety/collision handling, and language-code-to-display-name lookups.
//! Kept in one place so a change to a shared rule only has to happen once.

use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Removes filesystem-unsafe characters from a string for use as a filename.
/// Falls back to `"untitled"` if nothing usable remains.
pub fn clean_filename(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"[\\/*?:"<>|]"#).unwrap());
    let clean = re.replace_all(s.trim(), "").trim().to_string();
    if clean.is_empty() {
        "untitled".to_string()
    } else {
        clean
    }
}

/// Returns `path` unchanged if nothing exists there yet. Otherwise appends
/// " (1)", " (2)", … right before the extension until a free name is found,
/// so downloads and merges never silently overwrite an existing file.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str());

    let mut n = 1u32;
    loop {
        let candidate_name = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

// ── language codes ───────────────────────────────────────────────────────────

/// Converts a language code to ISO 639-2 (e.g. for the MKV subtitle track
/// `language` metadata `ffmpeg` sets). Only the base code before any `-`
/// variant suffix is looked up — YouTube attaches suffixes like `-orig`,
/// `-Hans`, or `-419` that ISO 639 itself doesn't recognize. Tries the
/// ISO 639-1 (two-letter) table first, then 639-3 (three-letter) — some
/// YouTube codes (`bho`, `ceb`, `haw`, …) are only in the latter. Falls
/// back to the base code unchanged if neither recognizes it.
pub fn to_iso639_2(lang: &str) -> String {
    let base = lang.split('-').next().unwrap_or(lang);
    isolang::Language::from_639_1(base)
        .or_else(|| isolang::Language::from_639_3(base))
        .map(|l| l.to_639_3().to_string())
        .unwrap_or_else(|| base.to_string())
}

/// Converts a language code to an English display name (e.g. for the MKV
/// track `title`, or `subtitleify languages`' listing). See
/// [`to_iso639_2`] — same two-lookup strategy, same base-code-only
/// lookup, same fallback behavior (returns `lang` unchanged if neither
/// table recognizes it).
pub fn lang_display_name(lang: &str) -> String {
    let base = lang.split('-').next().unwrap_or(lang);
    isolang::Language::from_639_1(base)
        .or_else(|| isolang::Language::from_639_3(base))
        .map(|l| l.to_name().to_string())
        .unwrap_or_else(|| lang.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_filename_strips_unsafe_characters() {
        assert_eq!(clean_filename("a/b\\c:d*e?f\"g<h>i|j"), "abcdefghij");
    }

    #[test]
    fn clean_filename_falls_back_when_empty() {
        assert_eq!(clean_filename("???"), "untitled");
    }

    #[test]
    fn unique_path_keeps_free_names_untouched() {
        let dir = std::env::temp_dir().join(format!("subtitleify_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("clip.srt");
        assert_eq!(unique_path(&target), target);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unique_path_increments_on_collision() {
        let dir =
            std::env::temp_dir().join(format!("subtitleify_test_collision_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("clip.srt");
        std::fs::write(&target, b"first").unwrap();
        std::fs::write(dir.join("clip (1).srt"), b"second").unwrap();

        let resolved = unique_path(&target);
        assert_eq!(resolved, dir.join("clip (2).srt"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn to_iso639_2_converts_known_codes() {
        assert_eq!(to_iso639_2("ar"), "ara");
        assert_eq!(to_iso639_2("en"), "eng");
    }

    #[test]
    fn to_iso639_2_strips_youtube_variant_suffixes_first() {
        assert_eq!(to_iso639_2("ar-orig"), "ara");
        assert_eq!(to_iso639_2("zh-Hans"), "zho");
    }

    #[test]
    fn to_iso639_2_falls_back_to_the_base_code_when_unknown() {
        assert_eq!(to_iso639_2("xx-orig"), "xx");
    }

    #[test]
    fn to_iso639_2_and_lang_display_name_also_resolve_iso_639_3_only_codes() {
        // "bho" (Bhojpuri), "ceb" (Cebuano), "haw" (Hawaiian) are codes
        // YouTube's auto-translate offers that only exist in ISO 639-3,
        // not 639-1 — from_639_1 alone would miss them.
        assert_eq!(to_iso639_2("bho"), "bho");
        assert_eq!(lang_display_name("bho"), "Bhojpuri");
        assert_eq!(lang_display_name("ceb"), "Cebuano");
        assert_eq!(lang_display_name("haw"), "Hawaiian");
    }

    #[test]
    fn lang_display_name_converts_known_codes() {
        assert_eq!(lang_display_name("ar"), "Arabic");
        assert_eq!(lang_display_name("en"), "English");
    }

    #[test]
    fn lang_display_name_falls_back_to_the_full_code_when_unknown() {
        assert_eq!(lang_display_name("xx-orig"), "xx-orig");
    }
}
