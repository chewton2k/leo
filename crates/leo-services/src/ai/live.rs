//! Live transcription over a recording that is still being written.
//!
//! The rolling loop re-transcribes the tail of the recording every
//! [`ROLL_INTERVAL`] through the local speech model. The transcript is shown as
//! it grows; there is no AI summary in between, which is what keeps it fast.
//!
//! Only the scheduling and text-stitching decisions live here, as pure
//! functions; the recording session performs the effects.

use std::time::Duration;

/// How often a new slice is cut from the growing recording.
///
/// Three seconds, so words appear while the speaker is still talking. It was
/// fifteen, which also meant the *first* text arrived fifteen seconds in — long
/// enough that a user who said one sentence and stopped saw nothing at all and
/// reasonably concluded the feature was broken.
///
/// The cost of a short interval is requests: roughly twenty a minute of
/// continuous speech. Two things keep that in check — silent slices are never
/// sent at all, and a failure backs the interval off ([`backoff`]) rather than
/// hammering a rate-limited provider.
pub const ROLL_INTERVAL: Duration = Duration::from_secs(3);

/// Ceiling for the backoff after repeated failures.
pub const MAX_ROLL_INTERVAL: Duration = Duration::from_secs(24);

/// How loud a slice must be before it is worth transcribing, as a fraction of
/// full scale.
///
/// Digital silence is not the same as a quiet room: a live microphone in a
/// silent room still reads around 0.001, while a muted or unpermitted one reads
/// below 0.0001. This sits between them.
pub const SILENCE_CEILING: f64 = 0.0005;
/// How many leading words of a new segment may be treated as repetition.
///
/// Generous on purpose. The nominal overlap is one second — a handful of
/// words — but a slice whose transcription failed leaves the cursor where it
/// was, so the next slice re-covers everything since the last success. That can
/// be a minute of speech, and a bound of "a few words" would let the whole span
/// through twice.
const MAX_OVERLAP_WORDS: usize = 240;
/// How much of the running transcript's tail is compared, in normalized
/// characters. Must be large enough to hold `MAX_OVERLAP_WORDS`; only the end
/// of the transcript can overlap, so bounding it keeps stitching cheap across a
/// long lecture.
const TAIL_COMPARE_CHARS: usize = 1600;
/// Shortest normalized run accepted as a repetition of two or more words.
/// Multi-word agreement is already strong evidence, so this only rules out
/// pairs like "a of".
const MIN_MULTI_WORD_CHARS: usize = 5;
/// Shortest normalized run accepted when only one word matches. Short function
/// words repeat by coincidence constantly ("the", "and", "of"), and dropping
/// one on that basis loses speech, so a lone word must be distinctive.
const MIN_SINGLE_WORD_CHARS: usize = 8;

/// The interval to wait after a failure, doubling up to [`MAX_ROLL_INTERVAL`].
///
/// A three-second cadence is only affordable while requests succeed. Against a
/// rate-limited provider, retrying at the same pace makes the limit permanent,
/// so each failure halves the request rate until it recovers.
pub const CLOUD_ROLL_INTERVAL: Duration = Duration::from_secs(8);
pub const SETTLE_AFTER_SECS: u64 = 12;
pub const SETTLE_EARLIEST_SECS: u64 = 6;
pub const SETTLE_KEEP_SECS: u64 = 2;

pub fn quietest(samples: &[i16], from: usize, to: usize, frame: usize) -> usize {
    let frame = frame.max(1);
    let to = to.min(samples.len().saturating_sub(frame));
    if from >= to {
        return to.max(from).min(samples.len());
    }
    (from..to)
        .step_by(frame)
        .min_by_key(|at| {
            samples[*at..*at + frame]
                .iter()
                .map(|s| (*s as i64) * (*s as i64))
                .sum::<i64>()
        })
        .unwrap_or(to)
}

pub fn backoff(current: Duration) -> Duration {
    let doubled = current.saturating_mul(2);
    if doubled > MAX_ROLL_INTERVAL {
        MAX_ROLL_INTERVAL
    } else {
        doubled
    }
}

/// Whether a measured peak amplitude means "nothing was said".
///
/// Sending silence to Whisper is worse than sending nothing: it does not return
/// an empty string, it invents plausible filler. "Thank you." and "Thanks for
/// watching!" are its favourites, which is how a user saying "hello, my name
/// is…" into a microphone macOS had muted got back "thank you".
pub fn is_silent(peak_amplitude: f64) -> bool {
    peak_amplitude < SILENCE_CEILING
}

/// Whether a transcription is one of Whisper's silence artifacts rather than
/// speech.
///
/// A second line of defence behind [`is_silent`], for audio quiet enough to
/// hallucinate on but loud enough to pass the level check. Only an exact whole
/// output counts: someone can genuinely say "thank you", but they rarely say it
/// as the entire contents of a slice that contained nothing else.
pub fn is_silence_artifact(text: &str) -> bool {
    const ARTIFACTS: &[&str] = &[
        "thank you",
        "thank you.",
        "thanks for watching",
        "thanks for watching!",
        "thank you for watching",
        "thank you very much",
        "please subscribe",
        "you",
        "bye",
        "bye.",
        ".",
        "[blank_audio]",
        "(silence)",
        "[silence]",
    ];
    let cleaned = text.trim().to_lowercase();
    ARTIFACTS.contains(&cleaned.as_str())
}

/// Reduce text to comparable characters: letters and digits only, lowercased.
///
/// Spaces and punctuation are dropped rather than normalized, because the same
/// speech can come back tokenized differently across two requests — "breadth
/// first search" one time, "Breadth-first search," the next. Comparing word
/// lists misses that; comparing the character run does not.
fn norm(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// How many leading words of `next_words` repeat the end of `previous_norm`.
///
/// Longest match wins, so a long re-covered span collapses in one step rather
/// than leaving most of it duplicated. Matches shorter than
/// [`MIN_OVERLAP_CHARS`] are ignored as coincidence.
fn repeated_word_count(previous_norm: &str, next_words: &[&str], max_words: usize) -> usize {
    let limit = max_words.min(next_words.len());
    for k in (1..=limit).rev() {
        let candidate = norm(&next_words[..k].join(""));
        let floor = if k == 1 {
            MIN_SINGLE_WORD_CHARS
        } else {
            MIN_MULTI_WORD_CHARS
        };
        if candidate.chars().count() < floor {
            continue;
        }
        if previous_norm.ends_with(&candidate) {
            return k;
        }
    }
    0
}

/// Append a newly transcribed segment to the running transcript, dropping the
/// part the audio overlap caused to be transcribed twice.
///
/// Whisper is not deterministic across requests, so this cannot assume the
/// repeated words come back identically — comparison is on a normalized
/// character run, and when no overlap is detectable the segment is appended
/// whole. A duplicated phrase reads worse than a missing one, but losing speech
/// is worse than either, so the tie is broken toward keeping text.
pub fn stitch(previous: &str, segment: &str) -> String {
    if segment.trim().is_empty() {
        return previous.to_string();
    }
    if previous.trim().is_empty() {
        return segment.trim().to_string();
    }

    let next_words: Vec<&str> = segment.split_whitespace().collect();
    // Only the tail of the transcript can overlap, and bounding it keeps this
    // cheap as the transcript grows through a long lecture.
    let previous_norm = {
        let n = norm(previous);
        let start = n
            .char_indices()
            .rev()
            .nth(TAIL_COMPARE_CHARS - 1)
            .map_or(0, |(at, _)| at);
        n[start..].to_string()
    };

    let dropped = repeated_word_count(&previous_norm, &next_words, MAX_OVERLAP_WORDS);
    let kept = &next_words[dropped..];
    if kept.is_empty() {
        return previous.trim_end().to_string();
    }
    if segment.contains('\n') {
        let mut rest = segment.trim_start();
        for _ in 0..dropped {
            rest = rest
                .split_once(char::is_whitespace)
                .map_or("", |(_, after)| after)
                .trim_start();
        }
        return format!("{}\n{}", previous.trim_end(), rest.trim_end());
    }
    format!("{} {}", previous.trim_end(), kept.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quietest_moment_is_found_inside_the_range() {
        let mut samples = vec![8000i16; 16_000 * 12];
        let pause = 16_000 * 9;
        for s in &mut samples[pause..pause + 3200] {
            *s = 10;
        }
        let at = quietest(&samples, 16_000 * 6, 16_000 * 10, 1600);
        assert!((pause..pause + 3200).contains(&at), "{at}");
        assert_eq!(quietest(&samples, 500, 100, 1600), 500);
        assert_eq!(quietest(&[], 0, 10, 1600), 0);
    }

    // ── slicing ─────────────────────────────────────────────────────────────

    /// The bug this guards: the first slice used to be due one full interval
    /// after recording began, so a user who said one sentence and stopped saw
    /// nothing and concluded the feature was broken.
    #[test]
    fn the_roll_interval_is_short_enough_to_feel_live() {
        assert!(
            ROLL_INTERVAL <= Duration::from_secs(5),
            "a {}s cadence is not live",
            ROLL_INTERVAL.as_secs()
        );
    }

    #[test]
    fn backoff_doubles_and_then_stops_at_the_ceiling() {
        assert_eq!(backoff(Duration::from_secs(3)), Duration::from_secs(6));
        assert_eq!(backoff(Duration::from_secs(6)), Duration::from_secs(12));
        assert_eq!(backoff(MAX_ROLL_INTERVAL), MAX_ROLL_INTERVAL);
        // And it never runs away, however many failures pile up.
        let mut interval = ROLL_INTERVAL;
        for _ in 0..50 {
            interval = backoff(interval);
        }
        assert_eq!(interval, MAX_ROLL_INTERVAL);
    }

    /// Real measurements: a muted or unpermitted microphone reads at digital
    /// silence, a quiet room reads above it, and speech is far above.
    #[test]
    fn silence_is_told_apart_from_a_quiet_room_and_from_speech() {
        // Measured from a macOS mic with permission denied, and from a
        // synthesised silent WAV — both 0.000031.
        assert!(is_silent(0.000031), "digital silence read as sound");
        assert!(is_silent(0.0));
        // A live mic in a quiet room.
        assert!(!is_silent(0.002), "a quiet room read as silence");
        // Measured from `say` output.
        assert!(!is_silent(0.784), "speech read as silence");
    }

    /// Whisper's invented filler must not reach the transcript.
    #[test]
    fn whispers_silence_artifacts_are_recognized() {
        for artifact in [
            "Thank you.",
            "thank you",
            "  Thanks for watching! ",
            "[BLANK_AUDIO]",
            ".",
            "you",
        ] {
            assert!(is_silence_artifact(artifact), "missed {artifact:?}");
        }
    }

    /// And real speech must survive, including a sentence that merely contains
    /// one of those phrases.
    #[test]
    fn real_speech_is_not_mistaken_for_an_artifact() {
        for speech in [
            "Hello, my name is Charlton",
            "thank you for the explanation, that makes sense",
            "So thank you. Next, the ownership rules.",
            "Thanks for watching the lecture recording I made yesterday",
        ] {
            assert!(
                !is_silence_artifact(speech),
                "dropped real speech: {speech:?}"
            );
        }
    }

    // ── stitching ───────────────────────────────────────────────────────────

    #[test]
    fn the_first_segment_becomes_the_transcript() {
        assert_eq!(stitch("", "hello there"), "hello there");
        assert_eq!(stitch("   ", "  hello there "), "hello there");
    }

    #[test]
    fn an_empty_segment_changes_nothing() {
        assert_eq!(stitch("hello there", ""), "hello there");
        assert_eq!(
            stitch(
                "[Speaker 1] so friday",
                "[Speaker 2] fine\n[Speaker 1] okay"
            ),
            "[Speaker 1] so friday\n[Speaker 2] fine\n[Speaker 1] okay"
        );
        assert_eq!(stitch("hello there", "   "), "hello there");
    }

    #[test]
    fn overlapping_words_are_dropped_once() {
        let previous = "the mitochondria is the powerhouse of the";
        let segment = "of the cell and it makes ATP";
        assert_eq!(
            stitch(previous, segment),
            "the mitochondria is the powerhouse of the cell and it makes ATP"
        );
    }

    /// Whisper re-punctuates and re-capitalizes across requests, so the
    /// repeated words rarely come back byte-identical.
    #[test]
    fn overlap_is_detected_despite_punctuation_and_case_changes() {
        let previous = "we will now discuss breadth first search";
        let segment = "Breadth-first search, explores level by level";
        let out = stitch(previous, segment);
        assert_eq!(
            out, "we will now discuss breadth first search explores level by level",
            "got: {out}"
        );
    }

    #[test]
    fn a_segment_entirely_repeating_the_tail_adds_nothing() {
        let previous = "one two three four";
        assert_eq!(stitch(previous, "three four"), "one two three four");
    }

    #[test]
    fn unrelated_text_is_appended_whole() {
        // Losing speech is worse than a duplicated phrase, so no overlap means
        // keep everything.
        let out = stitch("first topic done", "completely different words here");
        assert_eq!(out, "first topic done completely different words here");
    }

    /// A slice whose transcription failed leaves the cursor put, so the next
    /// slice re-covers everything since the last success. That span must
    /// collapse, not appear twice.
    #[test]
    fn a_long_re_covered_span_is_deduplicated_whole() {
        let previous = "breadth first search explores a graph level by level using a queue \
                        depth first search uses a stack instead and goes as deep as possible \
                        before backtracking";
        let segment = "uses a stack instead and goes as deep as possible before backtracking. \
                       Both run in linear time.";
        let out = stitch(previous, segment);
        assert_eq!(
            out.matches("uses a stack instead").count(),
            1,
            "the re-covered span was duplicated: {out}"
        );
        assert!(out.ends_with("Both run in linear time."), "got: {out}");
    }

    /// A lone short function word matching is coincidence, not repetition.
    #[test]
    fn a_single_short_word_is_not_treated_as_overlap() {
        let out = stitch("we discussed the", "the queue holds vertices");
        assert_eq!(out, "we discussed the the queue holds vertices");
    }

    /// A lone distinctive word is a real overlap.
    #[test]
    fn a_single_long_word_is_treated_as_overlap() {
        let out = stitch(
            "it finishes by backtracking",
            "backtracking to the previous vertex",
        );
        assert_eq!(out, "it finishes by backtracking to the previous vertex");
    }

    #[test]
    fn a_long_coincidental_repeat_is_not_treated_as_overlap() {
        // "the" appearing in both is not an overlap worth trusting beyond the
        // bounded window, and the result must never lose the new sentence.
        let previous = "a b c d e f g h i j k l m n o p the";
        let segment = "the second half of the lecture starts now";
        let out = stitch(previous, segment);
        assert!(
            out.ends_with("second half of the lecture starts now"),
            "got: {out}"
        );
        assert!(out.starts_with("a b c"), "got: {out}");
    }

    #[test]
    fn stitching_many_segments_in_sequence_stays_readable() {
        let segments = [
            "today we cover graphs",
            "cover graphs and their traversals",
            "traversals like BFS and DFS",
            "and DFS which uses a stack",
        ];
        let mut transcript = String::new();
        for s in segments {
            transcript = stitch(&transcript, s);
        }
        assert_eq!(
            transcript,
            "today we cover graphs and their traversals like BFS and DFS which uses a stack"
        );
    }
    #[test]
    fn a_transcript_in_wide_characters_can_be_stitched_whatever_its_length() {
        for count in 560..620 {
            let previous = "세".repeat(count);
            let stitched = stitch(&previous, "다음 문장");
            assert!(stitched.ends_with("다음 문장"), "{count}: {stitched}");
        }
    }

    #[test]
    fn overlap_is_found_in_text_that_is_not_ascii() {
        let previous = "모든 학생 여러분 오늘은 그래프 탐색을 배웁니다";
        let segment = "그래프 탐색을 배웁니다 그리고 다음은 큐입니다";
        assert_eq!(
            stitch(previous, segment),
            "모든 학생 여러분 오늘은 그래프 탐색을 배웁니다 그리고 다음은 큐입니다"
        );
    }

    #[test]
    fn accented_and_mixed_text_is_stitched_without_panicking() {
        for pairs in 600..612 {
            let previous = format!("{} déjà vu naïve café", "aü".repeat(pairs));
            let stitched = stitch(&previous, "café au lait");
            assert!(stitched.contains("au lait"), "{pairs}: {stitched}");
        }
    }
}
