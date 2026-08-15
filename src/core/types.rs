// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Plain data types shared across `core`'s modules — no logic lives here,
//! just the shapes everything else passes around.

use std::path::PathBuf;

/// A raw timed segment parsed directly from the json3 source.
#[derive(Debug, Clone)]
pub struct Event {
    /// Start time in milliseconds, relative to the video.
    pub start_ms: i64,
    /// End time in milliseconds, when json3 reports a trusted duration for
    /// this event. `None` for the growing-caption events YouTube emits
    /// word-by-word, which have no fixed end until the next event replaces
    /// them — see [`crate::core::parser::group_into_cues`].
    pub end_ms: Option<i64>,
    /// The event's text, already stripped of empty segments.
    pub text: String,
}

/// A cleaned, timed subtitle cue ready to be written to any format.
#[derive(Debug, Clone)]
pub struct SubCue {
    /// 1-based display order, assigned after all cues are built.
    pub index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Output subtitle format.
#[derive(Debug, Clone, PartialEq)]
pub enum SubFormat {
    Vtt,
    Srt,
}

/// Source subtitle type on YouTube.
#[derive(Debug, Clone, PartialEq)]
pub enum SubType {
    /// Manually authored (community/creator-uploaded) captions.
    Manual,
    /// YouTube's speech-to-text auto-generated captions.
    Auto,
}

impl SubType {
    /// Short, lowercase display label — `"manual"` or `"auto"`. Used
    /// anywhere a type needs to appear in output text: confirmation
    /// lines, warnings, or disambiguating a filename when the same
    /// language is requested in more than one type.
    pub fn label(&self) -> &'static str {
        match self {
            SubType::Manual => "manual",
            SubType::Auto => "auto",
        }
    }
}

/// A single video entry inside a playlist.
#[derive(Debug, Clone)]
pub struct PlaylistVideo {
    pub url: String,
    pub title: String,
}

/// A YouTube playlist with its title and video list.
#[derive(Debug, Clone)]
pub struct Playlist {
    pub title: String,
    pub videos: Vec<PlaylistVideo>,
}

/// A single subtitle file paired with its language code.
#[derive(Debug, Clone)]
pub struct SubEntry {
    pub path: PathBuf,
    /// Language code as parsed from the filename (e.g. `"ar"`,
    /// `"zh-Hans"`), not yet normalized to ISO 639-2/3.
    pub lang: String,
}

/// A video file matched with its subtitle(s), ready to be merged.
#[derive(Debug, Clone)]
pub struct MergeJob {
    pub video_path: PathBuf,
    pub subs: Vec<SubEntry>,
    /// Cleaned filename (no extension) the merged output should be saved as.
    pub output_name: String,
    /// 1 = ID match, 2 = title match, 3 = position match (fallback)
    pub match_stage: u8,
}
