use regex::Regex;
use std::sync::LazyLock;
use strsim::levenshtein;

fn build_ngram(words: &[&str]) -> String {
    words
        .iter()
        .map(|w| build_match_key(w))
        .collect::<Vec<_>>()
        .concat()
}

fn build_match_key(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

struct CustomWordMatchKey {
    word_index: usize,
    key: String,
}

fn build_custom_word_match_keys(word: &str, word_index: usize) -> Vec<CustomWordMatchKey> {
    let primary_key = build_match_key(word);
    let mut keys = Vec::with_capacity(2);

    if is_supported_fuzzy_key(&primary_key) {
        keys.push(CustomWordMatchKey {
            word_index,
            key: primary_key.clone(),
        });
    }

    if word.contains('&') {
        let expanded_key = build_match_key(&word.replace('&', " and "));
        if is_supported_fuzzy_key(&expanded_key) && expanded_key != primary_key {
            keys.push(CustomWordMatchKey {
                word_index,
                key: expanded_key,
            });
        }
    }

    keys
}

fn is_supported_fuzzy_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric())
}

fn soundex_code(word: &str) -> Vec<char> {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return Vec::new();
    };
    let mut code: Vec<char> = std::iter::once(first)
        .chain(chars.map(|c| match c {
            'b' | 'f' | 'p' | 'v' => '1',
            'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => '2',
            'd' | 't' => '3',
            'l' => '4',
            'm' | 'n' => '5',
            'r' => '6',
            'h' | 'w' => '9',
            _ => '0',
        }))
        .filter(|c| *c != '9')
        .collect();
    code.dedup();
    code.retain(|c| *c != '0');
    code.resize(4, '0');
    code
}

fn soundex(word1: &str, word2: &str) -> bool {
    soundex_code(word1) == soundex_code(word2)
}

fn supports_soundex(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphabetic())
}

fn find_best_match<'a>(
    candidate: &str,
    custom_words: &'a [String],
    custom_word_match_keys: &[CustomWordMatchKey],
    threshold: f64,
    phonetic: bool,
) -> Option<(&'a String, f64)> {
    if !is_supported_fuzzy_key(candidate) || candidate.chars().count() > 50 {
        return None;
    }

    if let Some(exact) = custom_word_match_keys.iter().find(|k| k.key == candidate) {
        return Some((&custom_words[exact.word_index], 0.0));
    }

    let mut best_match: Option<usize> = None;
    let mut best_score = f64::MAX;
    let mut runner_up = f64::MAX;

    for custom_word_key in custom_word_match_keys {
        if candidate.strip_suffix('s') == Some(custom_word_key.key.as_str()) {
            continue;
        }
        let candidate_len = candidate.chars().count();
        let custom_word_len = custom_word_key.key.chars().count();
        let len_diff = candidate_len.abs_diff(custom_word_len) as f64;
        let max_len = candidate_len.max(custom_word_len) as f64;
        let max_allowed_diff = (max_len * 0.25).max(2.0);
        if len_diff > max_allowed_diff {
            continue;
        }

        let levenshtein_dist = levenshtein(candidate, &custom_word_key.key);
        let levenshtein_score = if max_len > 0.0 {
            levenshtein_dist as f64 / max_len
        } else {
            1.0
        };

        let phonetic_match = phonetic
            && candidate_len >= 5
            && custom_word_len >= 5
            && levenshtein_dist <= 1
            && supports_soundex(candidate)
            && supports_soundex(&custom_word_key.key)
            && soundex(candidate, &custom_word_key.key);

        let combined_score = if phonetic_match {
            levenshtein_score * 0.3
        } else {
            levenshtein_score
        };

        if combined_score >= threshold {
            continue;
        }
        if combined_score < best_score {
            if best_match != Some(custom_word_key.word_index) {
                runner_up = best_score;
            }
            best_match = Some(custom_word_key.word_index);
            best_score = combined_score;
        } else if best_match != Some(custom_word_key.word_index) && combined_score < runner_up {
            runner_up = combined_score;
        }
    }

    if runner_up - best_score < 0.04 {
        return None;
    }
    best_match.map(|index| (&custom_words[index], best_score))
}

pub fn apply_custom_words(
    text: &str,
    custom_words: &[String],
    threshold: f64,
    language: Option<&str>,
) -> String {
    if custom_words.is_empty() {
        return text.to_string();
    }
    let phonetic = language.is_none_or(|l| l.split(['-', '_']).next() == Some("en"));

    let custom_word_match_keys: Vec<CustomWordMatchKey> = custom_words
        .iter()
        .enumerate()
        .flat_map(|(index, word)| build_custom_word_match_keys(word, index))
        .collect();

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let mut best_match: Option<(usize, &String, f64)> = None;

        for n in (1..=3).rev() {
            if i + n > words.len() {
                continue;
            }

            let ngram_words = &words[i..i + n];
            if ngram_words[..n.saturating_sub(1)]
                .iter()
                .any(|word| !extract_punctuation(word).1.is_empty())
            {
                continue;
            }
            let ngram = build_ngram(ngram_words);

            if let Some((replacement, score)) = find_best_match(
                &ngram,
                custom_words,
                &custom_word_match_keys,
                threshold,
                phonetic,
            ) {
                let is_better = best_match
                    .as_ref()
                    .is_none_or(|(_, _, best_score)| score < *best_score);
                if is_better {
                    best_match = Some((n, replacement, score));
                }
            }
        }

        if let Some((n, replacement, _)) = best_match {
            let ngram_words = &words[i..i + n];
            let (prefix, _) = extract_punctuation(ngram_words[0]);
            let (_, suffix) = extract_punctuation(ngram_words[n - 1]);

            let corrected = preserve_case_pattern(ngram_words[0], replacement);

            result.push(format!("{}{}{}", prefix, corrected, suffix));
            i += n;
        } else {
            result.push(words[i].to_string());
            i += 1;
        }
    }

    result.join(" ")
}

fn preserve_case_pattern(original: &str, replacement: &str) -> String {
    if original.chars().filter(|c| c.is_alphabetic()).count() > 1
        && original.chars().all(|c| c.is_uppercase())
    {
        replacement.to_uppercase()
    } else if replacement.chars().any(char::is_uppercase) {
        replacement.to_string()
    } else if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut chars: Vec<char> = replacement.chars().collect();
        if let Some(first_char) = chars.get_mut(0) {
            *first_char = first_char.to_uppercase().next().unwrap_or(*first_char);
        }
        chars.into_iter().collect()
    } else {
        replacement.to_string()
    }
}

fn extract_punctuation(word: &str) -> (&str, &str) {
    let prefix_end = word
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(index, _)| index)
        .unwrap_or(word.len());
    let suffix_start = word
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(index, c)| index + c.len_utf8())
        .unwrap_or(0);

    let prefix = if prefix_end > 0 {
        word.get(..prefix_end).unwrap_or_default()
    } else {
        ""
    };

    let suffix = if suffix_start < word.len() {
        word.get(suffix_start..).unwrap_or_default()
    } else {
        ""
    };

    (prefix, suffix)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputLanguageEvidence {
    UserSelected(String),
    TextDetected(String),
    Unknown,
}

impl OutputLanguageEvidence {
    pub(crate) fn language(&self) -> Option<&str> {
        match self {
            Self::UserSelected(language) | Self::TextDetected(language) => Some(language),
            Self::Unknown => None,
        }
    }
}

pub fn guess_language(text: &str) -> Option<&'static str> {
    if text.trim().chars().count() < 20 {
        return None;
    }
    let info = whatlang::detect(text)?;
    if info.confidence() < 0.9 {
        return None;
    }
    Some(match info.lang() {
        whatlang::Lang::Eng => "en",
        whatlang::Lang::Deu => "de",
        whatlang::Lang::Fra => "fr",
        other => other.code(),
    })
}

const UNIVERSAL_FILLER_WORDS: &[&str] = &[
    "uh", "uhm", "umm", "uhh", "uhhh", "ehh", "ehm", "ahm", "hmm", "hm", "mmm", "хм", "ммм",
];

fn gated_filler_words_for_language(lang: &str) -> &'static [&'static str] {
    let base_lang = lang.split(&['-', '_'][..]).next().unwrap_or(lang);

    match base_lang {
        "en" => &["um", "ah", "eh"],
        "de" => &["äh", "ähm"],
        "fr" => &["euh"],
        _ => &[],
    }
}

static MULTI_SPACE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());

fn collapse_stutters(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return text.to_string();
    }

    let mut result: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let word = words[i];
        let word_lower = word.to_lowercase();

        if word_lower.chars().all(|c| c.is_alphabetic()) {
            let mut count = 1;
            while i + count < words.len() && words[i + count].to_lowercase() == word_lower {
                count += 1;
            }

            if count >= 3 {
                result.push(word);
                i += count;
            } else {
                result.push(word);
                i += 1;
            }
        } else {
            result.push(word);
            i += 1;
        }
    }

    result.join(" ")
}

fn opens_sentence(kept: &str) -> bool {
    kept.trim_end()
        .chars()
        .next_back()
        .is_none_or(|c| matches!(c, '.' | '!' | '?' | '…'))
}

fn push_restoring_capital(kept: &mut String, segment: &str, capital_owed: &mut bool) {
    if *capital_owed {
        if let Some((index, first)) = segment.char_indices().find(|(_, c)| c.is_alphanumeric()) {
            *capital_owed = false;
            kept.push_str(segment.get(..index).unwrap_or_default());
            kept.extend(first.to_uppercase());
            kept.push_str(segment.get(index + first.len_utf8()..).unwrap_or_default());
            return;
        }
    }
    kept.push_str(segment);
}

static PROTECTED_SPAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)`[^`]*`|"[^"]*"|“[^”]*”|\b(?:https?|ftp)://\S+|\bwww\.\S+"#).unwrap()
});

fn remove_filler_matches(text: &str, pattern: &Regex) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut resume = 0;
    let mut capital_owed = false;
    let protected: Vec<_> = PROTECTED_SPAN.find_iter(text).map(|m| m.range()).collect();

    for filler in pattern.find_iter(text) {
        if text.get(..filler.start()).is_some_and(|h| h.ends_with('-'))
            || text.get(filler.end()..).is_some_and(|t| t.starts_with('-'))
            || protected.iter().any(|span| span.contains(&filler.start()))
        {
            continue;
        }
        push_restoring_capital(
            &mut kept,
            text.get(resume..filler.start()).unwrap_or_default(),
            &mut capital_owed,
        );
        let capitalized = filler.as_str().starts_with(char::is_uppercase);
        capital_owed |= capitalized && opens_sentence(&kept);
        resume = filler.end();
    }
    push_restoring_capital(
        &mut kept,
        text.get(resume..).unwrap_or_default(),
        &mut capital_owed,
    );

    kept
}

static YOU_KNOW_BETWEEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\byou know)?,\s*you know\s*,").unwrap());
static YOU_KNOW_OPENING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|[.!?]\s+)you know,\s*(\p{L})").unwrap());
static YOU_KNOW_CLOSING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\byou know)?,\s*you know([.!?]|$)").unwrap());
static ABBREVIATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?iu)(?:(?:^|[\s(])(?:etc|vs|mrs?|ms|dr|st|jr|sra?|prof|incl|esp|approx|cf|vgl|bzw|usw)|\p{L}\.\p{L})$")
        .unwrap()
});
static SCRATCH_THAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[.!?]\s+)scratch that(?:[.!?,]\s*|$)").unwrap());

fn remove_spoken_asides(text: &str) -> String {
    let mut text = text.to_string();
    while let Some(found) = SCRATCH_THAT.find(&text) {
        let head = text.get(..found.start()).unwrap_or_default();
        let kept = SENTENCE_BREAK
            .find_iter(head)
            .filter(|found| {
                !head
                    .get(found.start()..)
                    .is_some_and(|rest| rest.starts_with('.'))
                    || !ABBREVIATION.is_match(head.get(..found.start()).unwrap_or_default())
            })
            .last()
            .and_then(|found| head.get(..found.end()))
            .map(str::trim_end)
            .unwrap_or_default();
        let mut tail = String::new();
        push_restoring_capital(
            &mut tail,
            text.get(found.end()..).unwrap_or_default(),
            &mut true,
        );
        text = match (kept.is_empty(), tail.is_empty()) {
            (false, false) => format!("{kept} {tail}"),
            (false, true) => kept.to_string(),
            _ => tail,
        };
    }
    let text = YOU_KNOW_BETWEEN.replace_all(&text, |caps: &regex::Captures| {
        if caps.get(1).is_some() {
            caps[0].to_string()
        } else {
            ",".to_string()
        }
    });
    let text = YOU_KNOW_OPENING.replace_all(&text, |caps: &regex::Captures| {
        format!("{}{}", &caps[1], caps[2].to_uppercase())
    });
    YOU_KNOW_CLOSING
        .replace_all(&text, |caps: &regex::Captures| {
            if caps.get(1).is_some() {
                caps[0].to_string()
            } else {
                caps[2].to_string()
            }
        })
        .into_owned()
}

type Patterns = std::sync::Arc<[Regex]>;

fn filler_pattern(word: &str) -> Result<Regex, regex::Error> {
    Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(word)))
}

fn filler_patterns(language: &OutputLanguageEvidence) -> Patterns {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static DEFAULTS: LazyLock<Mutex<HashMap<&'static str, Patterns>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let gated = language
        .language()
        .map(gated_filler_words_for_language)
        .unwrap_or_default();
    let key = gated.first().copied().unwrap_or("");
    DEFAULTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(key)
        .or_insert_with(|| {
            UNIVERSAL_FILLER_WORDS
                .iter()
                .chain(gated)
                .map(|word| filler_pattern(word).unwrap())
                .collect()
        })
        .clone()
}

pub fn remove_filler_words(text: &str, language: &OutputLanguageEvidence) -> String {
    let patterns = filler_patterns(language);

    let mut filtered = text.to_string();
    for pattern in patterns.iter() {
        filtered = remove_filler_matches(&filtered, pattern);
    }
    if text.trim_end().ends_with('.') {
        let trimmed = filtered.trim_end().trim_end_matches(',').trim_end();
        if !trimmed.is_empty() && !trimmed.ends_with(['.', '!', '?']) {
            filtered = format!("{trimmed}.");
        }
    }

    remove_spoken_asides(&filtered)
}

static CLOCK_TIME_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(1[0-2]|0?[1-9])[:.]?([0-5]\d)(\s?)([ap]\.?\s?m\b\.?)").unwrap()
});

fn clock_times(text: &str) -> String {
    CLOCK_TIME_PATTERN
        .replace_all(text, "${1}:${2}${3}${4}")
        .into_owned()
}

pub fn normalize_transcription_output(text: &str) -> String {
    let mut normalized = collapse_stutters(text);

    normalized = MULTI_SPACE_PATTERN
        .replace_all(&normalized, " ")
        .to_string();

    normalized.trim().to_string()
}

pub fn meaningful(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

pub fn clean(text: &str, settings: &super::settings::Settings, language: Option<&str>) -> String {
    let (evidence, language) = match (language, guess_language(text)) {
        (Some(chosen), _) => (
            OutputLanguageEvidence::UserSelected(chosen.into()),
            language,
        ),
        (None, Some(guess)) => (
            OutputLanguageEvidence::TextDetected(guess.into()),
            Some(guess),
        ),
        (None, None) => (OutputLanguageEvidence::Unknown, None),
    };
    let fixed = apply_custom_words(text, &settings.custom_words, 0.18, language);
    let filtered = remove_filler_words(&fixed, &evidence);
    clock_times(&normalize_transcription_output(&filtered))
}

fn echo_normalize(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

static PROMPT_DELIMITER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[,、，]").unwrap());
static TRAILING_DELIMITER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[,、，]\s*$").unwrap());
static SENTENCE_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[.!?]\s+|[。！？]\s*").unwrap());
static NON_TERMINAL_PERIOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?iu)(?:(?:^|[\s(])(?:etc|vs|mrs?|ms|dr|st|jr|sra?|prof|incl|esp|approx|ca|cf|al|no|nr|ex|ej|vgl|bzw|usw|\p{L}|\d{1,2})|\p{L}\.\p{L})$|\.$",
    )
    .unwrap()
});

fn prompt_terms(prompt: &str) -> Vec<String> {
    PROMPT_DELIMITER
        .split(prompt)
        .map(echo_normalize)
        .filter(|term| !term.is_empty())
        .collect()
}

fn consecutive_term_run(text_words: &[&str], prompt: &str) -> bool {
    let sequence: Vec<(String, usize)> = prompt_terms(prompt)
        .into_iter()
        .enumerate()
        .flat_map(|(index, term)| {
            term.split(' ')
                .map(|word| (word.to_string(), index))
                .collect::<Vec<_>>()
        })
        .collect();
    if text_words.is_empty() || text_words.len() > sequence.len() {
        return false;
    }
    sequence.windows(text_words.len()).any(|window| {
        window
            .iter()
            .zip(text_words)
            .all(|((word, _), said)| word == said)
            && window[text_words.len() - 1].1 - window[0].1 + 1 >= 3
    })
}

fn overlap(words: &[&str], vocabulary: &std::collections::HashSet<&str>) -> (usize, usize) {
    let unique: std::collections::HashSet<&str> = words.iter().copied().collect();
    (
        unique.iter().filter(|w| vocabulary.contains(*w)).count(),
        unique.len(),
    )
}

fn is_prompt_fragment(text: &str, prompt: &str) -> bool {
    let (normalized, normalized_prompt) = (echo_normalize(text), echo_normalize(prompt));
    if normalized.is_empty() || normalized_prompt.is_empty() {
        return false;
    }
    let words: Vec<&str> = normalized.split(' ').collect();
    let vocabulary = normalized_prompt.split(' ').collect();
    let (matched, unique) = overlap(&words, &vocabulary);
    let mut counts = std::collections::HashMap::new();
    let looped = words.iter().any(|word| {
        let count = counts.entry(*word).or_insert(0);
        *count += 1;
        *count >= 3
    });
    let continues_prompt_shape = looped
        || (TRAILING_DELIMITER.is_match(text) && normalized.chars().count() <= 30)
        || (PROMPT_DELIMITER.is_match(text) && consecutive_term_run(&words, prompt));
    matched == unique && continues_prompt_shape
}

fn is_whole_prompt(text: &str, prompt: &str) -> bool {
    let (normalized, normalized_prompt) = (echo_normalize(text), echo_normalize(prompt));
    let words: Vec<&str> = normalized.split(' ').collect();
    if normalized_prompt.is_empty() || words.len() < 8 {
        return false;
    }
    if normalized == normalized_prompt {
        return true;
    }
    let vocabulary = normalized_prompt.split(' ').collect();
    let (matched, unique) = overlap(&words, &vocabulary);
    matched == unique && matched as f64 / vocabulary.len() as f64 >= 0.7
}

fn strip_trailing_echo<'a>(text: &'a str, words: &[String]) -> &'a str {
    let terms: Vec<String> = words.iter().flat_map(|word| prompt_terms(word)).collect();
    if terms.len() < 2 {
        return text;
    }
    let body = text.trim_end();
    let mut tail_start = 0;
    for found in SENTENCE_BREAK.find_iter(body) {
        let period = body
            .get(found.start()..)
            .is_some_and(|rest| rest.starts_with('.'));
        if found.end() >= body.len()
            || (period
                && NON_TERMINAL_PERIOD.is_match(body.get(..found.start()).unwrap_or_default()))
        {
            continue;
        }
        tail_start = found.end();
    }
    let tail = prompt_terms(body.get(tail_start..).unwrap_or_default());
    if tail_start == 0
        || !tail.first().is_some_and(|first| terms.contains(first))
        || tail.iter().filter(|term| terms.contains(term)).count() < 2
    {
        return text;
    }
    let indexes: Vec<Option<usize>> = tail
        .iter()
        .map(|term| terms.iter().position(|known| known == term))
        .collect();
    if !indexes
        .windows(2)
        .all(|pair| matches!(pair, [Some(a), Some(b)] if *b == *a + 1))
    {
        return text;
    }
    let opens_prompt = indexes[0] == Some(0);
    let closes_prompt = indexes.len() >= 5 && indexes.last() == Some(&Some(terms.len() - 1));
    if !opens_prompt && !closes_prompt {
        return text;
    }
    body.get(..tail_start).unwrap_or(text).trim_end()
}

pub fn strip_vocabulary_echo(text: &str, words: &[String]) -> String {
    let prompt = words.join(", ");
    if words.len() >= 3 && (is_whole_prompt(text, &prompt) || is_prompt_fragment(text, &prompt)) {
        return String::new();
    }
    strip_trailing_echo(text, words).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundex_codes_match_the_natural_crate_it_replaced() {
        let golden = [
            ("robert", "r163"),
            ("rupert", "r163"),
            ("rubin", "r150"),
            ("ashcraft", "a261"),
            ("ashcroft", "a261"),
            ("tymczak", "t522"),
            ("pfister", "p123"),
            ("honeyman", "h555"),
            ("Robert", "R163"),
            ("ROBERT", "R000"),
            ("jackson", "j250"),
            ("smith", "s530"),
            ("schmidt", "s253"),
            ("a", "a000"),
            ("bhb", "b100"),
            ("lloyd", "l430"),
            ("felix", "f420"),
            ("anthropic", "a536"),
            ("kubernetes", "k165"),
            ("whisper", "w216"),
            ("thequickbrown", "t221"),
        ];
        for (word, code) in golden {
            assert_eq!(
                soundex_code(word).iter().collect::<String>(),
                code,
                "{word}"
            );
        }
        assert!(soundex("robert", "rupert"));
        assert!(soundex("whisper", "wisper"));
        assert!(!soundex("robert", "Robert"));
        assert!(!soundex("leo", "leon"));
    }

    fn filter_transcription_output(text: &str, language: &str) -> String {
        let language = OutputLanguageEvidence::UserSelected(language.to_string());
        let filtered = remove_filler_words(text, &language);
        normalize_transcription_output(&filtered)
    }

    #[test]
    fn test_apply_custom_words_exact_match() {
        let text = "hello world";
        let custom_words = vec!["Hello".to_string(), "World".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_apply_custom_words_fuzzy_match() {
        let text = "helo wrold";
        let custom_words = vec!["hello".to_string(), "world".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_preserve_case_pattern() {
        assert_eq!(preserve_case_pattern("HELLO", "world"), "WORLD");
        assert_eq!(preserve_case_pattern("Hello", "world"), "World");
        assert_eq!(preserve_case_pattern("hello", "WORLD"), "WORLD");
    }

    #[test]
    fn test_extract_punctuation() {
        assert_eq!(extract_punctuation("hello"), ("", ""));
        assert_eq!(extract_punctuation("!hello?"), ("!", "?"));
        assert_eq!(extract_punctuation("...hello..."), ("...", "..."));
    }

    #[test]
    fn test_extract_punctuation_uses_unicode_boundaries() {
        assert_eq!(extract_punctuation("你好。"), ("", "。"));
        assert_eq!(extract_punctuation("「你好」"), ("「", "」"));
        assert_eq!(extract_punctuation("你好！"), ("", "！"));
    }

    #[test]
    fn test_empty_custom_words() {
        let text = "hello world";
        let custom_words = vec![];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn spoken_clock_times_get_their_colon_only_when_marked_as_times() {
        assert_eq!(clock_times("Meet at 230 pm."), "Meet at 2:30 pm.");
        assert_eq!(clock_times("2.30 p.m. works"), "2:30 p.m. works");
        assert_eq!(clock_times("1145AM"), "11:45AM");
        assert_eq!(clock_times("Call at 2:30 pm"), "Call at 2:30 pm");
        assert_eq!(clock_times("230 works for me"), "230 works for me");
        assert_eq!(
            clock_times("It costs 2.30 per item"),
            "It costs 2.30 per item"
        );
        assert_eq!(clock_times("1290 pm and 11230 pm"), "1290 pm and 11230 pm");
        assert_eq!(clock_times("230 apples"), "230 apples");
    }

    #[test]
    fn test_filter_filler_words() {
        let text = "So uhm I was thinking uh about this";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "So I was thinking about this");
    }

    #[test]
    fn test_filter_filler_words_case_insensitive() {
        let text = "UHM this is UH a test";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "This is a test");
    }

    #[test]
    fn test_filter_filler_words_with_punctuation() {
        let text = "Well, uhm, I think, uh. that's right";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "Well, I think, that's right");
    }

    #[test]
    fn test_filter_cleans_whitespace() {
        let text = "Hello    world   test";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "Hello world test");
    }

    #[test]
    fn test_filter_trims() {
        let text = "  Hello world  ";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_filter_combined() {
        let text = "  Uhm, so I was, uh, thinking about this  ";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "So I was, thinking about this");
    }

    #[test]
    fn test_filter_final_filler_keeps_the_full_stop() {
        let result = filter_transcription_output("We should ship it, um.", "en");
        assert_eq!(result, "We should ship it.");
        let result = filter_transcription_output("Call me tomorrow uh.", "en");
        assert_eq!(result, "Call me tomorrow.");
        assert_eq!(filter_transcription_output("Uh.", "en"), "");
    }

    #[test]
    fn test_filter_leading_filler_keeps_sentence_capital() {
        let result = filter_transcription_output("Um, so I think we should ship it.", "en");
        assert_eq!(result, "So I think we should ship it.");

        let result = filter_transcription_output("That works. Um, let me check.", "en");
        assert_eq!(result, "That works. Let me check.");

        let result = filter_transcription_output("He said, Um, not today.", "en");
        assert_eq!(result, "He said, not today.");
    }

    #[test]
    fn test_filter_preserves_valid_text() {
        let text = "This is a completely normal sentence.";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "This is a completely normal sentence.");
    }

    #[test]
    fn test_filter_stutter_collapse() {
        let text = "w wh wh wh wh wh wh wh wh wh why";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "w wh why");
    }

    #[test]
    fn test_filter_stutter_short_words() {
        let text = "I I I I think so so so so";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "I think so");
    }

    #[test]
    fn test_filter_stutter_longer_words() {
        let text = "Check data doc doc doc doc documentation.";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "Check data doc documentation.");
    }

    #[test]
    fn test_filter_stutter_mixed_case() {
        let text = "No NO no NO no";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "No");
    }

    #[test]
    fn test_filter_stutter_preserves_two_repetitions() {
        let text = "no no is fine";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "no no is fine");
    }

    #[test]
    fn test_filter_english_removes_um() {
        let text = "um I think um this is good";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "I think this is good");
    }

    #[test]
    fn test_filter_portuguese_preserves_um() {
        let text = "um gato bonito";
        let result = filter_transcription_output(text, "pt");
        assert_eq!(result, "um gato bonito");
    }

    #[test]
    fn test_filter_spanish_preserves_ha() {
        let text = "ha sido un buen día";
        let result = filter_transcription_output(text, "es");
        assert_eq!(result, "ha sido un buen día");
    }

    #[test]
    fn test_filter_language_code_with_region() {
        let text = "um gato bonito";
        let result = filter_transcription_output(text, "pt-BR");
        assert_eq!(result, "um gato bonito");
    }

    #[test]
    fn test_filter_unknown_language_still_removes_universal_fillers() {
        let text = "uh I think uhm this works";
        let result = filter_transcription_output(text, "xx");
        assert_eq!(result, "I think this works");
    }

    #[test]
    fn test_filter_unknown_language_does_not_remove_um() {
        let text = "um I think this works";
        let result = filter_transcription_output(text, "xx");
        assert_eq!(result, "um I think this works");
    }

    #[test]
    fn test_filter_unknown_evidence_removes_universal_keeps_gated() {
        let filtered = remove_filler_words(
            "uhh bueno hmm creo que um ha llegado",
            &OutputLanguageEvidence::Unknown,
        );
        assert_eq!(
            normalize_transcription_output(&filtered),
            "bueno creo que um ha llegado"
        );

        let cyrillic = remove_filler_words(
            "хм я думаю ммм это работает",
            &OutputLanguageEvidence::Unknown,
        );
        assert_eq!(
            normalize_transcription_output(&cyrillic),
            "я думаю это работает"
        );
    }

    #[test]
    fn test_filter_german_gated_fillers_require_evidence() {
        let text = "äh ich glaube ähm das passt";

        let unknown = remove_filler_words(text, &OutputLanguageEvidence::Unknown);
        assert_eq!(normalize_transcription_output(&unknown), text);

        let result = filter_transcription_output(text, "de");
        assert_eq!(result, "ich glaube das passt");
    }

    #[test]
    fn test_filter_preserves_millimetre_unit() {
        let text = "the screw is 5 mm long";
        let result = filter_transcription_output(text, "en");
        assert_eq!(result, "the screw is 5 mm long");
    }

    #[test]
    fn test_filter_detected_evidence_unlocks_gated_fillers() {
        let model = remove_filler_words(
            "um I think this works",
            &OutputLanguageEvidence::TextDetected("en".to_string()),
        );
        assert_eq!(normalize_transcription_output(&model), "I think this works");

        let text = remove_filler_words(
            "euh je pense que ça marche",
            &OutputLanguageEvidence::TextDetected("fr".to_string()),
        );
        assert_eq!(
            normalize_transcription_output(&text),
            "je pense que ça marche"
        );
    }

    #[test]
    fn test_apply_custom_words_ngram_two_words() {
        let text = "il cui nome è Charge B, che permette";
        let custom_words = vec!["ChargeBee".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert!(result.contains("ChargeBee,"), "unexpected result: {result}");
        assert!(!result.contains("Charge B"));
    }

    #[test]
    fn test_apply_custom_words_ngram_three_words() {
        let text = "use Chat G P T for this";
        let custom_words = vec!["ChatGPT".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert!(result.contains("ChatGPT"));
    }

    #[test]
    fn test_apply_custom_words_prefers_longer_ngram() {
        let text = "Open AI GPT model";
        let custom_words = vec!["OpenAI".to_string(), "GPT".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert_eq!(result, "OpenAI GPT model");
    }

    #[test]
    fn test_apply_custom_words_ngram_preserves_case() {
        let text = "CHARGE B is great";
        let custom_words = vec!["ChargeBee".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert!(result.contains("CHARGEBEE"));
    }

    #[test]
    fn test_apply_custom_words_ngram_with_spaces_in_custom() {
        let text = "using Mac Book Pro";
        let custom_words = vec!["MacBook Pro".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert_eq!(result, "using MacBook Pro");
    }

    #[test]
    fn test_apply_custom_words_trailing_number_not_doubled() {
        let text = "use GPT4 for this";
        let custom_words = vec!["GPT-4".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert!(
            !result.contains("GPT-44"),
            "got double-counted result: {}",
            result
        );
    }

    #[test]
    fn test_apply_custom_words_matches_ampersand_word() {
        let text = "send it to RD for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18, None);
        assert_eq!(result, "send it to R&D for review");
    }

    #[test]
    fn test_apply_custom_words_matches_spoken_ampersand_word() {
        let text = "send it to R and D for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18, None);
        assert_eq!(result, "send it to R&D for review");
    }

    #[test]
    fn test_apply_custom_words_preserves_ampersand_word() {
        let text = "send it to R&D for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18, None);
        assert_eq!(result, "send it to R&D for review");
    }

    #[test]
    fn test_apply_custom_words_handles_unicode_punctuation() {
        let text = "「Handee。」";
        let custom_words = vec!["Handy".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5, None);
        assert_eq!(result, "「Handy。」");
    }

    #[test]
    fn test_apply_custom_words_skips_cjk_fuzzy_matching() {
        let text = "你好。";
        let custom_words = vec!["你号".to_string()];
        let result = apply_custom_words(text, &custom_words, 1.0, None);
        assert_eq!(result, text);
    }

    #[test]
    fn spoken_asides_are_removed_only_where_they_are_asides() {
        let en = OutputLanguageEvidence::UserSelected("en".into());
        let clean = |text: &str| remove_filler_words(text, &en);
        assert_eq!(
            clean("We ship Tuesday, you know, maybe Wednesday"),
            "We ship Tuesday, maybe Wednesday"
        );
        assert_eq!(clean("You know, we ship Tuesday."), "We ship Tuesday.");
        assert_eq!(clean("It was late, you know."), "It was late.");
        assert_eq!(
            clean("Lunch at noon. Scratch that. Lunch at one."),
            "Lunch at one."
        );
        assert_eq!(
            clean("Hi. Lunch at noon. scratch that, lunch at one."),
            "Hi. Lunch at one."
        );
        assert_eq!(clean("Lunch at noon. Scratch that"), "");
        assert_eq!(clean("I paid 3.5 dollars. Scratch that."), "");
        assert_eq!(
            clean("Hi there. I paid 3.5 dollars. Scratch that. Four dollars."),
            "Hi there. Four dollars."
        );
        assert_eq!(
            clean("Ask Dr. Lee today. Scratch that, ask Sam."),
            "Ask Sam."
        );
        assert_eq!(
            clean("He said no. Lunch at noon. Scratch that. Lunch at one."),
            "He said no. Lunch at one."
        );
        for kept in [
            "I like it",
            "do you know the way",
            "Do you know, Sam?",
            "Please scratch that itch.",
            "I need to scratch that.",
            "Scratch that itch for me.",
            "you know what I mean",
            "If you know, you know.",
        ] {
            assert_eq!(clean(kept), kept);
        }
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    fn settings(custom: &[&str]) -> crate::dictation::settings::Settings {
        crate::dictation::settings::Settings {
            custom_words: words(custom),
            ..Default::default()
        }
    }

    #[test]
    fn short_custom_names_leave_ordinary_words_alone() {
        let names = words(&["Sam", "Tim", "Ann", "Max", "Uma"]);
        let text = "um I have the same time an hour to make it";
        assert_eq!(apply_custom_words(text, &names, 0.18, None), text);
        assert_eq!(apply_custom_words(text, &names, 0.18, Some("en")), text);
        assert_eq!(
            apply_custom_words("she said yes", &words(&["Shih"]), 0.18, None),
            "she said yes"
        );
    }

    #[test]
    fn phonetic_discount_needs_english_and_a_close_long_word() {
        let felix = words(&["Felix"]);
        assert_eq!(
            apply_custom_words("hi felex", &felix, 0.18, None),
            "hi Felix"
        );
        assert_eq!(
            apply_custom_words("hi felex", &felix, 0.18, Some("en-US")),
            "hi Felix"
        );
        assert_eq!(
            apply_custom_words("hi felex", &felix, 0.18, Some("pt")),
            "hi felex"
        );
        assert_eq!(
            apply_custom_words("hi fallex", &felix, 0.18, None),
            "hi fallex"
        );
    }

    #[test]
    fn a_token_equal_to_a_custom_word_is_not_pulled_to_another() {
        let both = words(&["Maxx", "Max"]);
        assert_eq!(apply_custom_words("max", &both, 0.5, None), "Max");
    }

    #[test]
    fn two_equally_close_custom_words_abstain() {
        let both = words(&["Felix", "Felex"]);
        assert_eq!(apply_custom_words("felax", &both, 0.18, None), "felax");
        assert_eq!(apply_custom_words("felix", &both, 0.18, None), "Felix");
    }

    #[test]
    fn plurals_and_possessives_of_custom_words_are_left_as_spoken() {
        assert_eq!(
            apply_custom_words("Anthropic's models", &words(&["Anthropic"]), 0.18, None),
            "Anthropic's models"
        );
        assert_eq!(
            apply_custom_words("count the tokens", &words(&["token"]), 0.18, None),
            "count the tokens"
        );
    }

    #[test]
    fn custom_word_casing_keeps_its_own_capitals() {
        let phone = words(&["iPhone"]);
        assert_eq!(
            apply_custom_words("I phone you later", &phone, 0.18, None),
            "iPhone you later"
        );
        assert_eq!(
            apply_custom_words("open X code now", &words(&["Xcode"]), 0.18, None),
            "open Xcode now"
        );
        assert_eq!(preserve_case_pattern("I", "world"), "World");
    }

    #[test]
    fn hyphenated_interjections_stay_whole() {
        assert_eq!(
            filter_transcription_output("Uh-huh, that works.", "en"),
            "Uh-huh, that works."
        );
        assert_eq!(
            filter_transcription_output("mm-hmm sounds good", "en"),
            "mm-hmm sounds good"
        );
        assert_eq!(
            filter_transcription_output("uh-oh uh we broke it", "en"),
            "uh-oh we broke it"
        );
    }

    #[test]
    fn quoted_code_and_url_tokens_keep_filler_like_words() {
        assert_eq!(
            filter_transcription_output("Say \"um\" then um go", "en"),
            "Say \"um\" then go"
        );
        assert_eq!(
            filter_transcription_output("Run `uh` now", "en"),
            "Run `uh` now"
        );
        assert_eq!(
            filter_transcription_output("Open https://example.com/uh/um now um", "en"),
            "Open https://example.com/uh/um now"
        );
    }

    #[test]
    fn scratch_that_keeps_the_next_sentence_capitalised() {
        let en = OutputLanguageEvidence::UserSelected("en".into());
        assert_eq!(
            remove_filler_words("I like cats. Scratch that, dogs are better.", &en),
            "Dogs are better."
        );
    }

    #[test]
    fn repeats_collapse() {
        assert_eq!(
            clean("no no no that's wrong", &settings(&[]), Some("en")),
            "no that's wrong"
        );
    }

    #[test]
    fn language_is_guessed_from_the_text_when_none_is_chosen() {
        let s = settings(&[]);
        assert_eq!(
            clean(
                "Um, so I was thinking that we could meet on Tuesday afternoon to talk about the budget.",
                &s,
                None
            ),
            "So I was thinking that we could meet on Tuesday afternoon to talk about the budget."
        );
        let portuguese = "um gato muito bonito está dormindo na minha cama porque ele gosta de ficar perto de mim";
        assert_eq!(clean(portuguese, &s, None), portuguese);
        assert_eq!(clean("Um, yes", &s, None), "Um, yes");
        assert_eq!(guess_language("too short"), None);
        assert_eq!(
            guess_language("So I was thinking that we could meet on Tuesday afternoon to talk about the budget."),
            Some("en")
        );
    }

    #[test]
    fn meaningful_needs_a_letter_or_digit() {
        assert!(!meaningful("."));
        assert!(!meaningful(" , "));
        assert!(!meaningful(""));
        assert!(meaningful("a"));
        assert!(meaningful("7."));
        assert!(meaningful("東京"));
    }

    const LARGE: &[&str] = &[
        "OpenWhispr",
        "Parakeet",
        "Alcahest",
        "Chromium",
        "TypeScript",
        "Electron",
        "testing",
        "data",
        "benchmark",
        "inference",
        "transcription",
        "dictionary",
        "microphone",
        "renderer",
        "latency",
        "pipeline",
    ];

    #[test]
    fn echo_filter_drops_a_whole_recited_list() {
        let three = words(&["OpenWhispr", "Parakeet", "Alcahest"]);
        for echo in [
            "OpenWhispr, Parakeet, Alcahest",
            "OpenWhispr, Parakeet, Alcahest.",
            "openwhispr, parakeet, alcahest",
            "OpenWhispr,  Parakeet,  Alcahest",
        ] {
            assert_eq!(strip_vocabulary_echo(echo, &three), "", "{echo}");
        }
        for said in [
            "I just installed OpenWhispr and it works great",
            "OpenWhispr, Parakeet",
            "The quick brown fox jumps over the lazy dog",
            "OpenWhispr Parakeet Alcahest",
        ] {
            assert_eq!(strip_vocabulary_echo(said, &three), said, "{said}");
        }
        let names = words(&["Leo", "Felix", "Claude", "Codex"]);
        assert_eq!(
            strip_vocabulary_echo("Leo Felix Claude", &names),
            "Leo Felix Claude"
        );
        let nato = words(&[
            "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
            "Juliet",
        ]);
        assert_eq!(
            strip_vocabulary_echo(
                "Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliet",
                &nato
            ),
            ""
        );
        assert_eq!(
            strip_vocabulary_echo("Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel", &nato),
            ""
        );
        for said in [
            "Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliet the",
            "Alpha Bravo Charlie Delta Echo Foxtrot Golf",
        ] {
            assert_eq!(strip_vocabulary_echo(said, &nato), said, "{said}");
        }
        let accents = words(&["Müller", "François", "José"]);
        assert_eq!(
            strip_vocabulary_echo("Müller, François, José", &accents),
            ""
        );
        assert_eq!(
            strip_vocabulary_echo("muller francois jose", &accents),
            "muller francois jose"
        );
        assert_eq!(
            strip_vocabulary_echo("東京, 大阪, 京都", &words(&["東京", "大阪", "京都"])),
            ""
        );
    }

    #[test]
    fn echo_filter_drops_looped_and_dangling_fragments_only() {
        let large = words(LARGE);
        for echo in [
            "data, data, data, data,",
            "testing,",
            "testing, ",
            "OpenWhispr, OpenWhispr, OpenWhispr",
            "TypeScript, Electron, testing, data, benchmark",
        ] {
            assert_eq!(strip_vocabulary_echo(echo, &large), "", "{echo}");
        }
        for said in [
            "OpenWhispr is working",
            "hello there",
            "testing",
            "Electron renderer latency",
            "OpenWhispr Parakeet Alcahest Chromium",
            "Electron, renderer",
            "testing, data",
            "OpenWhispr, OpenWhispr",
            "benchmark, TypeScript, data, Electron, testing",
            "yes, no, maybe, dunno,",
            "",
            "...",
        ] {
            assert_eq!(strip_vocabulary_echo(said, &large), said, "{said}");
        }
        assert_eq!(
            strip_vocabulary_echo("OpenWhispr", &words(&["OpenWhispr"])),
            "OpenWhispr"
        );
        assert_eq!(strip_vocabulary_echo("testing,", &[]), "testing,");
    }

    #[test]
    fn echo_filter_strips_a_recited_tail_and_keeps_speech() {
        let two = words(&["OpenWhispr", "n8n"]);
        assert_eq!(
            strip_vocabulary_echo(
                "Appreciate the time. Thanks, all. Thanks, you. OpenWhispr, n8n.",
                &two
            ),
            "Appreciate the time. Thanks, all. Thanks, you."
        );
        let six = words(&[
            "OpenWhispr",
            "n8n",
            "Traefik",
            "Tailscale",
            "Claude Code",
            "kubectl",
        ]);
        assert_eq!(
            strip_vocabulary_echo(
                "All right. Appreciate the time. Thanks, you. OpenWhispr, n8n, Traefik, Tailscale, Claude Code, kubectl",
                &six
            ),
            "All right. Appreciate the time. Thanks, you."
        );
        assert_eq!(
            strip_vocabulary_echo("Ship it today! openwhispr, n8n, traefik,", &six),
            "Ship it today!"
        );
        let eight = words(&[
            "OpenWhispr",
            "n8n",
            "Traefik",
            "Tailscale",
            "Claude Code",
            "kubectl",
            "Qwen",
            "Ollama",
        ]);
        assert_eq!(
            strip_vocabulary_echo(
                "We shipped it. Tailscale, Claude Code, kubectl, Qwen, Ollama.",
                &eight
            ),
            "We shipped it."
        );
        for said in [
            "Here are the tools I use. Traefik, Tailscale.",
            "We shipped it. kubectl, Qwen, Ollama.",
            "We shipped it. n8n, Traefik, Tailscale, Claude Code, kubectl.",
            "Which ones did you pick? Tailscale, Traefik, n8n.",
            "Tools. OpenWhispr, Traefik, Tailscale.",
            "The VPN was flaky, so we moved it to Tailscale.",
            "That was flaky. Tailscale.",
            "OpenWhispr, n8n. Those are the two tools I set up today.",
            "Today I wired OpenWhispr, n8n and Traefik together.",
            "It is ready. OpenWhispr, then n8n.",
            "Use the tools e.g. OpenWhispr, n8n",
            "Ask Mr. OpenWhispr, n8n",
            "Step 1. OpenWhispr, n8n",
            "So... OpenWhispr, n8n",
            "Wir nutzen Tools, z.B. OpenWhispr, n8n.",
        ] {
            assert_eq!(strip_vocabulary_echo(said, &eight), said, "{said}");
        }
        assert_eq!(
            strip_vocabulary_echo("We shipped in 2026. OpenWhispr, n8n.", &two),
            "We shipped in 2026."
        );
        let names = words(&["St. Louis", "Chicago", "J. R. R. Tolkien"]);
        assert_eq!(
            strip_vocabulary_echo("Done. St. Louis, Chicago", &names),
            "Done."
        );
        assert_eq!(
            strip_vocabulary_echo("I read it. Louis, Chicago", &names),
            "I read it. Louis, Chicago"
        );
    }
}
