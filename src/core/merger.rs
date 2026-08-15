// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Matches video files to subtitle files and embeds subtitles into videos
//! via `ffmpeg`.

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use super::types::{MergeJob, SubEntry};
use super::util::{clean_filename, lang_display_name, to_iso639_2, unique_path};

// ── constants ─────────────────────────────────────────────────────────────────

const VIDEO_EXTS: &[&str] = &["mp4", "webm", "mkv", "avi", "mov"];
const SUB_EXTS: &[&str] = &["srt", "vtt"];

// ── title helpers ─────────────────────────────────────────────────────────────

/// Strips special characters and collapses whitespace for fuzzy matching.
pub fn normalize_title(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extracts a YouTube video ID like `[dQw4w9WgXcQ]` from a filename stem.
fn extract_id(s: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\[([A-Za-z0-9_-]{8,12})\]").unwrap());
    re.captures(s).map(|c| c[1].to_string())
}

/// Splits a video filename into (clean title, optional YouTube ID).
///
/// `"My Video [abc123].webm"` → `("My Video", Some("abc123"))`
fn parse_video_name(filename: &str) -> (String, Option<String>) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\[([A-Za-z0-9_-]{8,12})\]").unwrap());
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let id = re.captures(stem).map(|c| c[1].to_string());
    let title = re.replace(stem, "").trim().to_string();
    (title, id)
}

/// Splits a subtitle filename into (base title, language code).
///
/// `"My Video - ar.srt"` → `("My Video", "ar")`. Also handles:
/// - the language variant suffixes YouTube itself produces and
///   subtitleify downloads as-is — `ar-orig` (original-language marker),
///   `zh-Hans`/`zh-Hant` (script), and `es-419` (numeric region) — not
///   just simple two-letter region codes like `en-US`;
/// - the optional `" (manual)"`/`" (auto)"` suffix `subtitleify download`
///   adds when the same language was requested in both types (see
///   `commands::download::type_suffix`) — matched but discarded, so
///   `"My Video - ar (manual).srt"` still resolves to language `"ar"`
///   instead of silently falling back to `"und"`.
fn parse_sub_name(filename: &str) -> (String, String) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^(.+) - ([a-zA-Z]{2,3}(?:-[a-zA-Z0-9]+)?)(?: \((?:manual|auto)\))?\.[a-z]+$")
            .unwrap()
    });
    if let Some(caps) = re.captures(filename) {
        return (caps[1].to_string(), caps[2].to_string());
    }
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_string();
    (stem, "und".to_string())
}

/// Lists files directly inside `dir` whose extension (case-insensitively)
/// is in `extensions`, sorted alphabetically for deterministic ordering.
fn read_files(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|_| panic!("Cannot read directory: {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| {
                    let lower = e.to_lowercase();
                    extensions.contains(&lower.as_str())
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

// ── matching ──────────────────────────────────────────────────────────────────

/// Holds all subtitle files sharing the same base title, grouped for one video.
struct SubGroup {
    entries: Vec<SubEntry>,
    norm_title: String,
    embedded_id: Option<String>,
}

/// Matches every video file in `videos_dir` with its subtitle(s) in `subs_dir`.
///
/// Matching runs in three stages:
/// 1. YouTube ID match  — most reliable
/// 2. Normalized title  — handles special-character differences
/// 3. Alphabetical position — last resort fallback
///
/// Returns `(matched jobs, unmatched video paths)`.
pub fn match_videos_to_subs(videos_dir: &Path, subs_dir: &Path) -> (Vec<MergeJob>, Vec<PathBuf>) {
    let video_files = read_files(videos_dir, VIDEO_EXTS);
    let sub_files = read_files(subs_dir, SUB_EXTS);

    // ── build subtitle groups (one group per unique base title) ───────────────
    let mut group_map: HashMap<String, SubGroup> = HashMap::new();

    for path in &sub_files {
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let (base, lang) = parse_sub_name(name);
        let norm_title = normalize_title(&base);
        let embedded_id = extract_id(&base);

        let group = group_map.entry(norm_title.clone()).or_insert(SubGroup {
            entries: Vec::new(),
            norm_title: norm_title.clone(),
            embedded_id: embedded_id.clone(),
        });
        group.entries.push(SubEntry {
            path: path.clone(),
            lang,
        });
    }

    // Sort groups alphabetically for stable stage-3 position matching
    let mut groups: Vec<SubGroup> = group_map.into_values().collect();
    groups.sort_by(|a, b| a.norm_title.cmp(&b.norm_title));

    // Build fast-lookup indexes
    let id_index: HashMap<&str, usize> = groups
        .iter()
        .enumerate()
        .filter_map(|(i, g)| g.embedded_id.as_deref().map(|id| (id, i)))
        .collect();

    let title_index: HashMap<&str, usize> = groups
        .iter()
        .enumerate()
        .map(|(i, g)| (g.norm_title.as_str(), i))
        .collect();

    // ── match each video ──────────────────────────────────────────────────────
    let mut jobs: Vec<MergeJob> = Vec::new();
    let mut unmatched: Vec<PathBuf> = Vec::new();
    let mut used: HashSet<usize> = HashSet::new();
    let mut stage3_queue: Vec<(PathBuf, String)> = Vec::new();

    for video_path in &video_files {
        let name = match video_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let (title, id) = parse_video_name(name);
        let normalized = normalize_title(&title);
        let output_name = clean_filename(&title);

        // Stage 1: YouTube ID match
        if let Some(&idx) = id.as_deref().and_then(|id| id_index.get(id)) {
            used.insert(idx);
            jobs.push(MergeJob {
                video_path: video_path.clone(),
                subs: groups[idx].entries.clone(),
                output_name,
                match_stage: 1,
            });
            continue;
        }

        // Stage 2: Normalized title match
        if let Some(&idx) = title_index.get(normalized.as_str()) {
            used.insert(idx);
            jobs.push(MergeJob {
                video_path: video_path.clone(),
                subs: groups[idx].entries.clone(),
                output_name,
                match_stage: 2,
            });
            continue;
        }

        // Pending for stage 3
        stage3_queue.push((video_path.clone(), output_name));
    }

    // Stage 3: alphabetical position match among unmatched
    let unused_indices: Vec<usize> = (0..groups.len()).filter(|i| !used.contains(i)).collect();

    for (i, (video_path, output_name)) in stage3_queue.into_iter().enumerate() {
        if let Some(&idx) = unused_indices.get(i) {
            jobs.push(MergeJob {
                video_path,
                subs: groups[idx].entries.clone(),
                output_name,
                match_stage: 3,
            });
        } else {
            unmatched.push(video_path);
        }
    }

    jobs.sort_by(|a, b| a.video_path.cmp(&b.video_path));
    (jobs, unmatched)
}

// ── ffmpeg helpers ────────────────────────────────────────────────────────────

/// Returns the number of subtitle streams already embedded in a video file.
/// Falls back to 0 if ffprobe is unavailable or fails.
fn count_subtitle_streams(video_path: &Path) -> usize {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-select_streams",
            "s",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(video_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
        Err(_) => 0,
    }
}

// ── ffmpeg merge ──────────────────────────────────────────────────────────────

/// Embeds all subtitle tracks in `job` into a new MKV file using ffmpeg.
/// Language metadata is set for each track so media players display it correctly.
/// Returns the actual path written to (may differ from the "natural" name if
/// a file with that name already existed). In `--verbose` mode, ffmpeg's own
/// output is shown instead of being hidden — useful when a merge silently
/// fails for a reason subtitleify's own error message doesn't capture.
pub fn merge_video(
    job: &MergeJob,
    output_dir: &Path,
    verbose: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let output_path = unique_path(&output_dir.join(format!("{}.mkv", job.output_name)));
    let existing_sub_cnt = count_subtitle_streams(&job.video_path);

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");
    cmd.arg("-i").arg(&job.video_path);

    for sub in &job.subs {
        cmd.arg("-i").arg(&sub.path);
    }

    cmd.args(["-map", "0"]);
    for i in 0..job.subs.len() {
        cmd.arg("-map").arg((i + 1).to_string());
    }

    cmd.args(["-c", "copy", "-c:s", "srt"]);

    // Offset indices by existing subtitle count so we never overwrite
    // metadata of subtitle tracks already embedded in the video.
    for (i, sub) in job.subs.iter().enumerate() {
        let idx = existing_sub_cnt + i;
        cmd.arg(format!("-metadata:s:s:{idx}"))
            .arg(format!("language={}", to_iso639_2(&sub.lang)));
        cmd.arg(format!("-metadata:s:s:{idx}"))
            .arg(format!("title={}", lang_display_name(&sub.lang)));
    }

    cmd.arg(&output_path);

    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = cmd.status()?;

    if !status.success() {
        return Err(format!(
            "ffmpeg failed for: {}",
            job.video_path.file_name().unwrap().to_string_lossy()
        )
        .into());
    }

    Ok(output_path)
}

// ── single file merge ─────────────────────────────────────────────────────────

/// Embeds one or more subtitle files into a single video.
///
/// If `output_dir` is `Some`, the merged file is written there; otherwise it
/// is written next to the source video. Returns the actual path written to
/// (may differ from the "natural" name to avoid clobbering the source file
/// or an existing output).
pub fn merge_single(
    video_path: &Path,
    sub_paths: &[PathBuf],
    output_dir: Option<&Path>,
    verbose: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let (title, _) = parse_video_name(video_path.file_name().unwrap().to_str().unwrap());
    let output_name = clean_filename(&title);
    let output_dir = output_dir.unwrap_or_else(|| video_path.parent().unwrap_or(Path::new(".")));

    // If output would overwrite the source file (e.g. input is already .mkv),
    // append _merged to avoid ffmpeg reading and writing the same file.
    let candidate = output_dir.join(format!("{output_name}.mkv"));
    let candidate = if candidate == video_path {
        output_dir.join(format!("{output_name}_merged.mkv"))
    } else {
        candidate
    };
    let output_path = unique_path(&candidate);

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");
    cmd.arg("-i").arg(video_path);

    for sub in sub_paths {
        cmd.arg("-i").arg(sub);
    }

    cmd.args(["-map", "0"]);
    for i in 0..sub_paths.len() {
        cmd.arg("-map").arg((i + 1).to_string());
    }

    cmd.args(["-c", "copy", "-c:s", "srt"]);

    // Count existing subtitle streams so new tracks don't overwrite their metadata
    let existing_sub_cnt = count_subtitle_streams(video_path);

    for (i, sub) in sub_paths.iter().enumerate() {
        let name = sub.file_name().unwrap().to_str().unwrap();
        let (_, lang) = parse_sub_name(name);
        let idx = existing_sub_cnt + i;
        cmd.arg(format!("-metadata:s:s:{idx}"))
            .arg(format!("language={}", to_iso639_2(&lang)));
        cmd.arg(format!("-metadata:s:s:{idx}"))
            .arg(format!("title={}", lang_display_name(&lang)));
    }

    cmd.arg(&output_path);

    if !verbose {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = cmd.status()?;

    if !status.success() {
        return Err(format!(
            "ffmpeg failed for: {}",
            video_path.file_name().unwrap().to_string_lossy()
        )
        .into());
    }

    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sub_name_handles_youtube_variant_codes() {
        assert_eq!(
            parse_sub_name("My Video - ar.srt"),
            ("My Video".to_string(), "ar".to_string())
        );
        assert_eq!(
            parse_sub_name("My Video - ar-orig.srt"),
            ("My Video".to_string(), "ar-orig".to_string())
        );
        assert_eq!(
            parse_sub_name("My Video - en-orig.vtt"),
            ("My Video".to_string(), "en-orig".to_string())
        );
        assert_eq!(
            parse_sub_name("My Video - zh-Hans.srt"),
            ("My Video".to_string(), "zh-Hans".to_string())
        );
        assert_eq!(
            parse_sub_name("My Video - zh-Hant.srt"),
            ("My Video".to_string(), "zh-Hant".to_string())
        );
        assert_eq!(
            parse_sub_name("My Video - es-419.srt"),
            ("My Video".to_string(), "es-419".to_string())
        );
        assert_eq!(
            parse_sub_name("My Video - pt-PT.srt"),
            ("My Video".to_string(), "pt-PT".to_string())
        );
    }

    #[test]
    fn parse_sub_name_strips_the_manual_auto_disambiguation_suffix() {
        // `subtitleify download` appends " (manual)"/" (auto)" when the
        // same language is requested in both types, to avoid a generic,
        // meaningless "(1)" collision suffix. Merge's language detection
        // must still resolve the plain language code from these names,
        // not fall back to "und".
        assert_eq!(
            parse_sub_name("Test Video - ar (manual).vtt"),
            ("Test Video".to_string(), "ar".to_string())
        );
        assert_eq!(
            parse_sub_name("Test Video - ar (auto).vtt"),
            ("Test Video".to_string(), "ar".to_string())
        );
        assert_eq!(
            parse_sub_name("Test Video - en-orig (manual).srt"),
            ("Test Video".to_string(), "en-orig".to_string())
        );
    }

    #[test]
    fn parse_sub_name_falls_back_to_und_for_unrecognizable_names() {
        assert_eq!(
            parse_sub_name("random_filename.srt"),
            ("random_filename".to_string(), "und".to_string())
        );
    }

    #[test]
    fn variant_codes_still_resolve_to_correct_iso_and_display_name() {
        assert_eq!(to_iso639_2("ar-orig"), "ara");
        assert_eq!(lang_display_name("ar-orig"), "Arabic");
        assert_eq!(to_iso639_2("zh-Hans"), "zho");
        assert_eq!(lang_display_name("zh-Hant"), "Chinese");
    }

    #[test]
    fn previously_unsupported_languages_now_resolve_via_isolang() {
        // These were NOT in the old hand-written ~20-language table and used
        // to silently fall back to the raw code as both the ISO tag and the
        // display name. isolang covers the full ISO 639 set instead.
        assert_eq!(to_iso639_2("sw"), "swa");
        assert_eq!(lang_display_name("sw"), "Swahili");
        assert_eq!(to_iso639_2("th"), "tha");
        assert_eq!(lang_display_name("th"), "Thai");
        assert_eq!(to_iso639_2("vi"), "vie");
        assert_eq!(lang_display_name("vi"), "Vietnamese");
        assert_eq!(to_iso639_2("uk"), "ukr");
        assert_eq!(lang_display_name("uk"), "Ukrainian");
    }

    #[test]
    fn unrecognized_language_code_falls_back_to_itself() {
        assert_eq!(to_iso639_2("zzz-orig"), "zzz");
        assert_eq!(lang_display_name("totally-unknown"), "totally-unknown");
    }
}
