// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Renders [`SubCue`]s to WebVTT or SRT text.

use super::types::SubCue;

// ── timestamp helpers ─────────────────────────────────────────────────────────

/// Formats milliseconds as a WebVTT timestamp: `HH:MM:SS.mmm`.
fn ms_to_vtt_timestamp(ms: i64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

/// Formats milliseconds as an SRT timestamp: `HH:MM:SS,mmm`. Same as
/// [`ms_to_vtt_timestamp`] but with a comma before the milliseconds, per
/// each format's convention.
fn ms_to_srt_timestamp(ms: i64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{h:02}:{m:02}:{s:02},{millis:03}")
}

// ── RTL detection ─────────────────────────────────────────────────────────────

/// Returns true for language codes ([`write_vtt`] uses this to add RTL
/// styling) that read right-to-left: Arabic, Hebrew, Persian, Urdu.
/// Matches on the base code only, so region/script variants like
/// `"ar-orig"` or `"fa-IR"` are still detected correctly.
pub fn is_rtl_lang(lang: &str) -> bool {
    let l = lang.to_lowercase();
    l.starts_with("ar") || l.starts_with("he") || l.starts_with("fa") || l.starts_with("ur")
}

// ── writers ───────────────────────────────────────────────────────────────────

/// Renders `cues` as a complete WebVTT document. Adds an RTL `::cue` style
/// block automatically when `lang` is right-to-left (see [`is_rtl_lang`]),
/// so Arabic/Hebrew/Persian/Urdu subtitles display correctly without the
/// player needing to guess the text direction.
pub fn write_vtt(cues: &[SubCue], lang: &str) -> String {
    let mut out = String::from("WEBVTT\n");
    if is_rtl_lang(lang) {
        out.push_str("\nSTYLE\n::cue {\n  direction: rtl;\n  unicode-bidi: embed;\n}\n");
    }
    out.push('\n');
    for cue in cues {
        out.push_str(&cue.index.to_string());
        out.push('\n');
        out.push_str(&ms_to_vtt_timestamp(cue.start_ms));
        out.push_str(" --> ");
        out.push_str(&ms_to_vtt_timestamp(cue.end_ms));
        out.push('\n');
        out.push_str(&cue.text);
        out.push_str("\n\n");
    }
    out
}

/// Renders `cues` as a complete SRT document.
pub fn write_srt(cues: &[SubCue]) -> String {
    let mut out = String::new();
    for cue in cues {
        out.push_str(&cue.index.to_string());
        out.push('\n');
        out.push_str(&ms_to_srt_timestamp(cue.start_ms));
        out.push_str(" --> ");
        out.push_str(&ms_to_srt_timestamp(cue.end_ms));
        out.push('\n');
        out.push_str(&cue.text);
        out.push_str("\n\n");
    }
    out
}
