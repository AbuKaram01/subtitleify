// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Turns a downloaded json3 caption file into clean, deduplicated
//! [`SubCue`]s — the core of what makes subtitleify's output different from
//! YouTube's raw, word-by-word-repeating captions (see the README's
//! "Before / After" section).

use encoding_rs::{UTF_16BE, UTF_16LE, WINDOWS_1252};
use regex::Regex;
use serde_json::Value;
use std::fs;

use super::types::{Event, SubCue};

// ── decoding ──────────────────────────────────────────────────────────────────

/// Decodes raw file bytes to a `String`, trying UTF-8 first (after
/// stripping a BOM if present), then UTF-16LE/BE by BOM, and finally
/// falling back to Windows-1252 — json3 files aren't guaranteed to be
/// UTF-8, so this covers what `yt-dlp` is known to produce.
pub fn try_decode(bytes: &[u8]) -> String {
    let stripped = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(stripped) {
        return s.to_string();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (decoded, _, had_errors) = UTF_16LE.decode(&bytes[2..]);
        if !had_errors {
            return decoded.into_owned();
        }
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (decoded, _, had_errors) = UTF_16BE.decode(&bytes[2..]);
        if !had_errors {
            return decoded.into_owned();
        }
    }
    let (decoded, _, _) = WINDOWS_1252.decode(bytes);
    decoded.into_owned()
}

// ── json3 parsing ─────────────────────────────────────────────────────────────

/// Parses raw json3 bytes into [`Event`]s. Each event's text is the
/// concatenation of its non-empty segments; segments are also used to
/// estimate a per-word timestamp (spread evenly across the segment's
/// duration), which [`group_into_cues`] uses to decide where a growing
/// caption can be safely split into full sentences. Returns an empty list
/// (with a warning on stderr) if the bytes aren't valid json3.
pub fn parse_json3(bytes: &[u8]) -> Vec<Event> {
    let content = try_decode(bytes);
    let json: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  [warn] json3 parse error: {e}");
            return Vec::new();
        }
    };

    let events = match json.get("events").and_then(|v| v.as_array()) {
        Some(e) => e,
        None => return Vec::new(),
    };

    let mut out: Vec<Event> = Vec::new();

    for event in events {
        let t_start = event.get("tStartMs").and_then(|v| v.as_i64()).unwrap_or(0);

        let duration_ms = event.get("dDurationMs").and_then(|v| v.as_i64());

        let segs = match event.get("segs").and_then(|v| v.as_array()) {
            Some(s) => s,
            None => continue,
        };

        let mut event_words: Vec<(String, i64)> = Vec::new();

        for seg in segs {
            let text = match seg.get("utf8").and_then(|v| v.as_str()) {
                Some(t) => t.to_string(),
                None => continue,
            };
            if text.trim().is_empty() {
                continue;
            }

            let offset = seg.get("tOffsetMs").and_then(|v| v.as_i64()).unwrap_or(0);

            let tokens: Vec<&str> = text.split_whitespace().collect();
            let n = tokens.len() as i64;
            for (j, token) in tokens.iter().enumerate() {
                let spread = if n > 1 { (j as i64 * 300) / n } else { 0 };
                event_words.push((token.to_string(), t_start + offset + spread));
            }
        }

        if event_words.is_empty() {
            continue;
        }

        let text = event_words
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let start_ms = event_words[0].1;
        let end_ms = duration_ms.map(|d| t_start + d);

        out.push(Event {
            start_ms,
            end_ms,
            text,
        });
    }

    out
}

// ── cleaning ──────────────────────────────────────────────────────────────────

/// Strips HTML tags from a single word/token and trims it. Returns `None`
/// if nothing is left afterward, so callers can filter empty tokens out
/// with `filter_map` in one step.
fn clean_word(word: &str) -> Option<String> {
    use std::sync::OnceLock;
    static RE_HTML: OnceLock<Regex> = OnceLock::new();

    let re_html = RE_HTML.get_or_init(|| Regex::new(r"<[^>]+>").unwrap());

    let t = re_html.replace_all(word, "");
    let t = t.trim().to_string();

    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// Returns true if `word` ends with sentence-terminating punctuation
/// (including Arabic `؟`). [`group_into_cues`] treats this as a good place
/// to end a cue even before hitting the word/duration limits.
fn ends_sentence(word: &str) -> bool {
    let last = word.trim_end().chars().last().unwrap_or(' ');
    matches!(last, '.' | '؟' | '!' | '…' | '?')
}

/// Placeholder for character-width line wrapping — currently a no-op.
/// `_max_chars` is accepted (and callers already pass 42, a common subtitle
/// line-length convention) so wrapping can be added later without touching
/// every call site.
fn wrap_text(text: &str, _max_chars: usize) -> String {
    text.to_string()
}

// ── cue grouping ──────────────────────────────────────────────────────────────

/// Turns raw [`Event`]s into deduplicated, sentence-aware [`SubCue`]s.
///
/// YouTube's auto-captions come in two shapes, handled differently here:
/// - **Trusted-duration events** (`event.end_ms` is `Some`) are already a
///   complete, final line — emitted as their own cue as-is.
/// - **Growing-caption events** (`event.end_ms` is `None`) repeat the same
///   sentence over and over, appending one word at a time, until the next
///   event replaces them. Words are buffered instead of re-emitted, and the
///   buffer is only flushed into a cue once it hits a sentence boundary
///   ([`ends_sentence`]) or a size/duration limit — which is exactly what
///   collapses YouTube's rolling transcript into normal, readable lines
///   (see the README's "Before / After" section).
///
/// Cue `index` is left at `0` during grouping and assigned at the end, once
/// the final cue count is known.
pub fn group_into_cues(events: Vec<Event>) -> Vec<SubCue> {
    const MAX_WORDS: usize = 15;
    const MAX_DUR: i64 = 6_000;
    const MAX_CUE_DUR: i64 = 8_000;

    let mut cues: Vec<SubCue> = Vec::new();

    let mut buf_words: Vec<String> = Vec::new();
    let mut buf_start: i64 = 0;
    let mut buf_last_ms: i64 = 0;

    // Turns the current word buffer into a cue ending just before
    // `next_start_ms` (capped at `MAX_CUE_DUR`), then clears the buffer.
    // A no-op when the buffer is already empty.
    let flush = |buf_words: &mut Vec<String>,
                 buf_start: &mut i64,
                 buf_last_ms: &mut i64,
                 next_start_ms: i64,
                 cues: &mut Vec<SubCue>| {
        if buf_words.is_empty() {
            return;
        }
        let text = wrap_text(&buf_words.join(" "), 42);
        let end_ms = next_start_ms
            .saturating_sub(10)
            .min(*buf_start + MAX_CUE_DUR);
        cues.push(SubCue {
            index: 0,
            start_ms: *buf_start,
            end_ms,
            text,
        });
        buf_words.clear();
        *buf_last_ms = next_start_ms;
    };

    for (i, event) in events.iter().enumerate() {
        if let Some(_trusted_dur) = event.end_ms {
            // Trusted-duration event: flush whatever growing-caption buffer
            // was pending, then emit this event as its own complete cue.
            flush(
                &mut buf_words,
                &mut buf_start,
                &mut buf_last_ms,
                event.start_ms,
                &mut cues,
            );

            let clean_tokens: Vec<String> = event
                .text
                .split_whitespace()
                .filter_map(clean_word)
                .collect();
            if clean_tokens.is_empty() {
                continue;
            }

            let next_start = events
                .get(i + 1)
                .map(|e| e.start_ms)
                .unwrap_or(event.start_ms + 1_500);
            let end_ms = next_start
                .saturating_sub(10)
                .min(event.start_ms + MAX_CUE_DUR);

            let text = wrap_text(&clean_tokens.join(" "), 42);
            cues.push(SubCue {
                index: 0,
                start_ms: event.start_ms,
                end_ms,
                text,
            });
            buf_last_ms = end_ms;
            continue;
        }

        for token in event.text.split_whitespace() {
            // Growing-caption event: buffer words one at a time instead of
            // re-emitting the whole repeated sentence, and only flush once
            // a sentence ends or a size/duration limit is hit.
            if let Some(clean) = clean_word(token) {
                if buf_words.is_empty() {
                    buf_start = event.start_ms;
                }
                buf_words.push(clean.clone());
                buf_last_ms = event.start_ms;

                let dur = event.start_ms - buf_start;
                let at_sentence = ends_sentence(&clean);
                let at_limit = buf_words.len() >= MAX_WORDS || dur >= MAX_DUR;

                if at_sentence || at_limit {
                    let next_ms = events
                        .get(i + 1)
                        .map(|e| e.start_ms)
                        .unwrap_or(buf_last_ms + 1_500);
                    flush(
                        &mut buf_words,
                        &mut buf_start,
                        &mut buf_last_ms,
                        next_ms,
                        &mut cues,
                    );
                }
            }
        }
    }

    if !buf_words.is_empty() {
        let end_ms = (buf_last_ms + 1_500).min(buf_start + MAX_CUE_DUR);
        cues.push(SubCue {
            index: 0,
            start_ms: buf_start,
            end_ms,
            text: wrap_text(&buf_words.join(" "), 42),
        });
    }

    for (i, cue) in cues.iter_mut().enumerate() {
        cue.index = i + 1;
    }

    cues
}

// ── public entry point ────────────────────────────────────────────────────────

/// Reads a downloaded json3 file from disk and turns it into clean
/// [`SubCue`]s in one call — the function [`crate::cli::commands::download`]
/// actually uses. Errors if the file can't be read or contains no events.
pub fn process_json3(input_path: &str) -> Result<Vec<SubCue>, Box<dyn std::error::Error>> {
    let bytes = fs::read(input_path)?;
    let events = parse_json3(&bytes);
    if events.is_empty() {
        return Err("No events found in json3 file.".into());
    }
    Ok(group_into_cues(events))
}
