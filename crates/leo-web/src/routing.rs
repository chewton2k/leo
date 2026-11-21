use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::tools::Access;
use crate::vectors::Meaning;

const MOST_CHARS: usize = 120;
const MOST_TURNS: usize = 6;
const OPENINGS: [&str; 24] = [
    "when ",
    "where ",
    "which ",
    "find ",
    "search ",
    "show ",
    "list ",
    "open ",
    "do i ",
    "did i ",
    "have i ",
    "is there ",
    "are there ",
    "what time ",
    "what's my ",
    "whats my ",
    "what’s my ",
    "what is my ",
    "what are my ",
    "what's on ",
    "what’s on ",
    "what is on ",
    "how many notes ",
    "how many tasks ",
];
const GREETINGS: [&str; 8] = [
    "hi",
    "hello",
    "hey",
    "thanks",
    "thank you",
    "ok",
    "okay",
    "got it",
];
const THEIRS: [&str; 18] = [
    "my",
    "note",
    "notes",
    "due",
    "deadline",
    "deadlines",
    "task",
    "tasks",
    "exam",
    "exams",
    "midterm",
    "midterms",
    "final",
    "finals",
    "calendar",
    "schedule",
    "today",
    "tomorrow",
];
const THOUGHT: [&str; 33] = [
    "why",
    "explain",
    "tell",
    "describe",
    "understand",
    "teach",
    "quiz",
    "practice",
    "practise",
    "compare",
    "prove",
    "solve",
    "derive",
    "derivative",
    "calculate",
    "summar",
    "study",
    "write",
    "rewrite",
    "draft",
    "plan",
    "translate",
    "better",
    "difference",
    "pros",
    "mean",
    "wrong",
    "right",
    "correct",
    "mistake",
    "error",
    "bug",
    "contradict",
];
const WEIGHED: [&str; 3] = ["compar", "analy", "review"];
const REFERS: [&str; 5] = ["this", "these", "that", "those", "them"];
const CHANGES: [&str; 17] = [
    "fix", "change", "edit", "update", "move", "delete", "remove", "replace", "rename", "create",
    "make", "add", "merge", "combine", "restore", "pin", "trash",
];
const POLITE: [&str; 5] = ["please", "can", "could", "would", "you"];
const EDITS: [&str; 16] = [
    "turn",
    "convert",
    "change",
    "make",
    "format",
    "fix",
    "bold",
    "italicize",
    "italicise",
    "reorder",
    "sort",
    "tidy",
    "capitalize",
    "capitalise",
    "indent",
    "put",
];
const FORMS: [&str; 26] = [
    "checkbox",
    "checkboxes",
    "checklist",
    "bullet",
    "bullets",
    "bulleted",
    "heading",
    "headings",
    "header",
    "headers",
    "list",
    "lists",
    "table",
    "numbered",
    "bold",
    "italic",
    "italics",
    "typo",
    "typos",
    "spelling",
    "punctuation",
    "alphabetical",
    "alphabetically",
    "todo",
    "todos",
    "formatting",
];
const MARGIN: f32 = 0.02;
const LIGHT_EXAMPLES: [&str; 40] = [
    "when is my bio midterm",
    "what's due this week",
    "find my chem notes",
    "open my lecture 4 note",
    "show my tasks for tomorrow",
    "do I have anything on friday",
    "which note has the lab schedule",
    "what time is my exam",
    "where is my reading list",
    "list my notes from today",
    "how many tasks are left",
    "is there a note on enzymes",
    "turn this bullet into a checkbox",
    "make these a numbered list",
    "convert this list into a table",
    "fix the typos in this paragraph",
    "rename this note to Week 3",
    "make this line a heading",
    "bold the key terms",
    "sort this list alphabetically",
    "put these in a table",
    "change the bullets to checkboxes",
    "tidy up the formatting",
    "capitalize the headings",
    "add a checkbox to each line",
    "move this line to the top",
    "delete the empty bullets",
    "mark this task as done",
    "add a task for friday",
    "what is the capital of France",
    "what does DNA stand for",
    "what year did the second world war end",
    "define osmosis in one line",
    "what's the formula for the area of a circle",
    "what day is it tomorrow",
    "show the last note I edited",
    "thanks",
    "ok got it",
    "hello",
    "remind me when the next lecture is",
];
const FULL_EXAMPLES: [&str; 40] = [
    "explain why write-back caching is faster",
    "compare my two lecture notes",
    "quiz me on chapter 3",
    "summarize all my bio notes",
    "teach me recursion step by step",
    "help me study for my midterm",
    "analyse the argument in this essay",
    "why does the krebs cycle need oxygen",
    "how does a hash map handle collisions",
    "make a study plan for finals week",
    "what are the main themes across my history notes",
    "walk me through this proof",
    "what is the difference between mitosis and meiosis",
    "critique my thesis statement",
    "help me understand eigenvalues",
    "give me practice problems on integrals",
    "derive the quadratic formula",
    "solve this system of equations and show the steps",
    "write an essay outline on climate policy",
    "review my notes and tell me what I am missing",
    "find the gaps in my understanding of photosynthesis",
    "connect the ideas in my economics notes",
    "test me on the vocabulary from week 2",
    "plan my revision for the next two weeks",
    "what would happen if the central bank raised rates",
    "evaluate the strengths and weaknesses of this design",
    "explain this code line by line",
    "why does my function return the wrong value",
    "argue both sides of this debate",
    "pull the key insights out of my reading notes",
    "how should I structure my lab report",
    "turn my lecture notes into a study guide",
    "make flashcards from this chapter",
    "what are the implications of this result",
    "reason through this probability puzzle",
    "rewrite this paragraph to be more persuasive",
    "draft a summary of the meeting with action items",
    "what can I learn from my past exam mistakes",
    "go through chapter 5 with me",
    "is this argument sound",
];

static CENTROIDS: OnceLock<(Vec<f32>, Vec<f32>)> = OnceLock::new();
static BUILDING: AtomicBool = AtomicBool::new(false);

struct Building;

impl Drop for Building {
    fn drop(&mut self) {
        BUILDING.store(false, Ordering::Release);
    }
}

pub struct Asked<'a> {
    pub message: &'a str,
    pub turns: usize,
    pub attached: usize,
    pub documents: usize,
    pub selected: bool,
    pub studying: bool,
    pub access: Access,
}

fn plain(message: &str) -> String {
    message
        .trim()
        .trim_end_matches(['?', '!', '.'])
        .to_lowercase()
}

pub fn light(asked: &Asked<'_>) -> Option<bool> {
    if asked.studying
        || asked.attached > 0
        || asked.documents > 0
        || asked.turns > MOST_TURNS
        || matches!(asked.access, Access::Auto)
    {
        return Some(false);
    }
    let message = plain(asked.message);
    if message.chars().count() > MOST_CHARS || message.contains(['\n', '`', '$']) {
        return Some(false);
    }
    let words: Vec<&str> = message
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let has = |stems: &[&str]| {
        words
            .iter()
            .any(|word| stems.iter().any(|stem| word.starts_with(stem)))
    };
    let thought = has(&THOUGHT) || has(&WEIGHED);
    let thoughtful = thought || words.contains(&"how");
    let first = words
        .iter()
        .find(|word| !POLITE.contains(word))
        .copied()
        .unwrap_or("");
    let pointing = words.iter().any(|w| REFERS.contains(w));
    let shaping = EDITS.contains(&first) && words.iter().any(|w| FORMS.contains(w));
    let edit = (first == "rename" && pointing) || (shaping && (pointing || asked.selected));
    if edit && !thoughtful {
        return Some(true);
    }
    if asked.selected {
        return Some(false);
    }
    if GREETINGS.contains(&message.as_str()) {
        return Some(true);
    }
    if OPENINGS.iter().any(|opening| message.starts_with(opening))
        && words.iter().any(|word| THEIRS.contains(word))
        && !thought
        && !has(&CHANGES)
        && !words.iter().skip(1).any(|word| *word == "how")
    {
        return Some(true);
    }
    if thoughtful
        || (shaping && first == "put")
        || has(&CHANGES)
        || message.is_empty()
        || !message.is_ascii()
    {
        return Some(false);
    }
    None
}

pub fn choose(rule: Option<bool>, warm: Option<bool>, unclear: impl FnOnce() -> bool) -> bool {
    warm != Some(false) && rule.unwrap_or_else(unclear)
}

pub fn leans_light(meaning: &Meaning, asked: Option<&[f32]>) -> bool {
    let Some(asked) = asked else {
        return false;
    };
    let Some(centroids) = CENTROIDS.get() else {
        if !BUILDING.swap(true, Ordering::AcqRel) {
            let meaning = Arc::clone(meaning);
            let building = Building;
            let _ = std::thread::Builder::new()
                .name("leo-routing".into())
                .spawn(move || {
                    let _building = building;
                    if let Some(built) = centroids_of(&meaning) {
                        let _ = CENTROIDS.set(built);
                    }
                });
        }
        return false;
    };
    closer_to_light(asked, centroids)
}

fn centroids_of(meaning: &Meaning) -> Option<(Vec<f32>, Vec<f32>)> {
    let texts: Vec<String> = LIGHT_EXAMPLES
        .iter()
        .chain(&FULL_EXAMPLES)
        .map(|text| text.to_string())
        .collect();
    let found = meaning(&texts, true).ok()?;
    if found.len() != texts.len() {
        return None;
    }
    let (light, full) = found.split_at(LIGHT_EXAMPLES.len());
    Some((centre(light), centre(full)))
}

fn centre(vectors: &[Vec<f32>]) -> Vec<f32> {
    let mut sum = vec![0.0; vectors.first().map_or(0, Vec::len)];
    for vector in vectors {
        for (total, value) in sum.iter_mut().zip(vector) {
            *total += value;
        }
    }
    let length = sum.iter().map(|value| value * value).sum::<f32>().sqrt();
    sum.iter()
        .map(|value| value / length.max(f32::EPSILON))
        .collect()
}

fn closer_to_light(asked: &[f32], (light, full): &(Vec<f32>, Vec<f32>)) -> bool {
    let cosine = |centroid: &[f32]| asked.iter().zip(centroid).map(|(a, b)| a * b).sum::<f32>();
    cosine(light) - cosine(full) > MARGIN
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plainly(message: &str) -> Asked<'_> {
        Asked {
            message,
            turns: 1,
            attached: 0,
            documents: 0,
            selected: false,
            studying: false,
            access: Access::Ask,
        }
    }

    #[test]
    fn short_lookups_greetings_and_small_edits_go_to_the_cheap_model() {
        for message in [
            "when is my bio midterm?",
            "When is Lab 3 due",
            "where did I put my chem notes?",
            "which note has the Krebs cycle",
            "find my reading list",
            "show my tasks for friday",
            "do I have anything tomorrow?",
            "what's my next deadline?",
            "what’s my next deadline?",
            "how many notes mention enzymes?",
            "is there a note on enzymes",
            "when is the final",
            "thanks!",
            "ok",
            "turn this bullet into a checkbox",
            "make these a numbered list",
            "fix the typos in this paragraph",
            "rename this note to Week 3",
            "convert this list to a table",
            "change these bullets into checkboxes",
            "make this a heading",
            "please bold the key terms in this list",
            "can you sort this list alphabetically",
            "tidy up the formatting of this note",
            "when is my essay due",
            "find my essay",
            "open my lesson 4 note",
        ] {
            assert_eq!(light(&plainly(message)), Some(true), "{message}");
        }
    }

    #[test]
    fn anything_that_needs_thought_stays_on_the_chosen_model() {
        for message in [
            "explain why write-back is faster",
            "compare my two lecture notes",
            "quiz me on chapter 3",
            "summarize all my bio notes",
            "make a table comparing glycolysis and the krebs cycle",
            "make a table of the key ideas in my history notes",
            "make a list of the causes of world war one",
            "analyse this list",
            "turn my lecture notes into a study guide",
            "how should I structure my lab report",
            "what is the difference between glycolysis and the krebs cycle?",
            "explain the krebs cycle",
            "why is my code wrong?",
            "quiz me on chapter 7",
            "solve x^2 = 4",
            "which of these answers is right and why",
            "when should I study for bio?",
            "list the steps and explain each",
            "find my note and summarise it",
            "find my note and summarize it",
            "show me how to prove it",
            "where should I move this note?",
            "add a task for friday",
            "find the derivative of x^2",
            "how many ways can 5 people sit around a table",
            "list the pros and cons of microservices",
            "which is better, thesis a or b",
            "find my essay and rewrite the intro",
            "open my bio note and remove the second section",
            "where did i go wrong",
            "did i get this right",
            "is there a mistake in my proof",
            "find the bug in my code",
            "which of my notes contradict each other",
            "find my notes on recursion and tell me how it works",
            "find my notes on recursion and how it works",
            "show my notes that describe recursion",
            "show my review notes",
            "find my notes and analyze them",
            "make a list of things to do in paris",
            "put together a list of questions to ask my professor",
            "rename every note in my bio folder after its main topic",
            "wann ist meine prüfung",
            "",
            "show $x^2$",
            "find this\nand that",
            "find ```code```",
        ] {
            assert_eq!(light(&plainly(message)), Some(false), "{message}");
        }
        let long = format!("find {}", "notes ".repeat(30));
        assert_eq!(light(&plainly(&long)), Some(false));
    }

    #[test]
    fn plain_questions_the_rules_cannot_place_go_to_the_embedder() {
        for message in [
            "where does glycolysis happen",
            "when do i use a heap over a bst",
            "which method do i use for this integral",
            "do i need a visa for japan",
            "what is the capital of peru",
            "show that the square root of 2 is irrational",
            "bold the key terms",
        ] {
            assert_eq!(light(&plainly(message)), None, "{message}");
        }
    }

    #[test]
    fn surrounding_work_keeps_the_chosen_model() {
        for message in ["when is lab 3 due?", "turn this bullet into a checkbox"] {
            for asked in [
                Asked {
                    attached: 1,
                    ..plainly(message)
                },
                Asked {
                    documents: 1,
                    ..plainly(message)
                },
                Asked {
                    studying: true,
                    ..plainly(message)
                },
                Asked {
                    turns: MOST_TURNS + 1,
                    ..plainly(message)
                },
                Asked {
                    access: Access::Auto,
                    ..plainly(message)
                },
            ] {
                assert_eq!(light(&asked), Some(false), "{message}");
            }
        }
        for message in [
            "when is lab 3 due?",
            "thanks",
            "where does glycolysis happen",
        ] {
            let selected = Asked {
                selected: true,
                ..plainly(message)
            };
            assert_eq!(light(&selected), Some(false), "{message}");
        }
        let edit = Asked {
            selected: true,
            ..plainly("turn this bullet into a checkbox")
        };
        assert_eq!(light(&edit), Some(true));
        assert_eq!(
            light(&Asked {
                turns: MOST_TURNS,
                access: Access::Read,
                ..plainly("when is lab 3 due?")
            }),
            Some(true)
        );
    }

    #[test]
    fn a_warm_full_chat_never_routes_down() {
        let unasked = || -> bool { panic!("a warm full chat must not ask the embedder") };
        assert!(!choose(Some(true), Some(false), unasked));
        assert!(!choose(None, Some(false), unasked));
        assert!(!choose(Some(false), None, unasked));
        assert!(choose(Some(true), None, unasked));
        assert!(choose(Some(true), Some(true), unasked));
        assert!(choose(None, None, || true));
        assert!(choose(None, Some(true), || true));
        assert!(!choose(None, None, || false));
    }

    #[test]
    fn without_the_note_search_vector_the_embedder_is_never_called() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let meaning: Meaning = Arc::new(move |texts: &[String], _: bool| {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        });
        assert!(!leans_light(&meaning, None));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_embedder_routes_light_only_on_a_clear_margin() {
        let centroids = (
            centre(&[vec![1.0, 0.0], vec![0.9, 0.1]]),
            centre(&[vec![0.0, 1.0], vec![0.1, 0.9]]),
        );
        assert!(closer_to_light(&[1.0, 0.0], &centroids));
        assert!(!closer_to_light(&[0.0, 1.0], &centroids));
        assert!(!closer_to_light(&[0.6, 0.6], &centroids));
        let short: Meaning = Arc::new(|_: &[String], _: bool| Ok(vec![vec![1.0]]));
        assert!(centroids_of(&short).is_none());
        let failing: Meaning = Arc::new(|_: &[String], _: bool| anyhow::bail!("no model"));
        assert!(centroids_of(&failing).is_none());
    }
}
