//! Evaluation harness for post-processing prompt quality.
//!
//! Provides golden-case evaluation: a set of known input/expected pairs
//! scored against the live API to measure prompt effectiveness.
//!
//! Run with: `just eval-prompt` or `cargo test -p dictate-core golden -- --ignored --nocapture`

use std::collections::HashSet;
use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use super::{GroqPostProcessor, PostProcessConfig};
use crate::postprocess::PostProcessor;

/// Categories of golden test cases.
///
/// Each variant maps to a `snake_case` string in `golden_cases.json`.
/// Adding a case with a misspelled category (e.g. `"filler_remval"`)
/// will fail at deserialization time rather than silently passing.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Removal of filler words (um, uh, like, you know).
    FillerRemoval,
    /// Correct casing of technical terms (Kubernetes, `PostgreSQL`, etc.).
    TechnicalTerms,
    /// Punctuation and comma insertion.
    Punctuation,
    /// Combined filler + technical terms + punctuation.
    Mixed,
    /// Edge cases (clean input, all-filler input, etc.).
    EdgeCase,
    /// Cases where meaning must be preserved despite filler-like words.
    MeaningPreservation,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::FillerRemoval => "filler_removal",
            Self::TechnicalTerms => "technical_terms",
            Self::Punctuation => "punctuation",
            Self::Mixed => "mixed",
            Self::EdgeCase => "edge_case",
            Self::MeaningPreservation => "meaning_preservation",
        };
        f.pad(label)
    }
}

/// A single golden test case loaded from `golden_cases.json`.
#[derive(Deserialize)]
pub struct GoldenCase {
    pub category: Category,
    pub input: String,
    pub expected: String,
    pub note: String,
}

/// Load all golden cases from the embedded JSON fixture.
pub fn load_golden_cases() -> Vec<GoldenCase> {
    let json = include_str!("prompts/golden_cases.json");
    serde_json::from_str(json).expect("failed to parse golden_cases.json")
}

/// Normalised Levenshtein similarity (character-level).
///
/// Returns a value between 0.0 (completely different) and 1.0 (identical).
/// Delegates to [`strsim::normalized_levenshtein`].
pub fn similarity(actual: &str, expected: &str) -> f64 {
    strsim::normalized_levenshtein(actual, expected)
}

/// ROUGE-1 F1 score (word-level unigram overlap).
///
/// Measures how well the actual output preserves the *words* from the
/// expected output, regardless of character-level edits. This complements
/// Levenshtein: removing filler words like "um" improves ROUGE (fewer wrong
/// words) even though Levenshtein penalises the length change.
///
/// Returns 0.0 when there is no word overlap and 1.0 for identical word sets.
#[allow(clippy::cast_precision_loss)]
pub fn rouge1_f(actual: &str, expected: &str) -> f64 {
    let actual_words: HashSet<&str> = actual.split_whitespace().collect();
    let expected_words: HashSet<&str> = expected.split_whitespace().collect();

    if actual_words.is_empty() && expected_words.is_empty() {
        return 1.0;
    }
    if actual_words.is_empty() || expected_words.is_empty() {
        return 0.0;
    }

    let overlap = actual_words.intersection(&expected_words).count() as f64;
    let precision = overlap / actual_words.len() as f64;
    let recall = overlap / expected_words.len() as f64;

    if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    }
}

/// Minimum similarity score for a golden case to pass.
const PASS_THRESHOLD: f64 = 0.85;

// ──── Tests ────────────────────────────────────────────────────────────────

/// Run all golden cases against the live API and report per-case results.
///
/// Requires `GROQ_API_KEY` environment variable. Skipped in normal CI.
///
/// ```bash
/// cargo test -p dictate-core golden_eval -- --ignored --nocapture
/// ```
#[test]
#[ignore = "hits live Groq API — run with: cargo test -p dictate-core golden -- --ignored --nocapture"]
fn golden_eval_against_live_api() {
    let api_key = match std::env::var("GROQ_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            eprintln!("GROQ_API_KEY not set — skipping golden eval");
            return;
        }
    };

    let cases = load_golden_cases();
    let pp = GroqPostProcessor;

    let mut pass = 0;
    let mut fail = 0;
    let mut total_lev = 0.0;
    let mut total_rouge = 0.0;

    eprintln!("\n╔═══════════════════════════════════════════════════════════════════════╗");
    eprintln!("║  Golden Evaluation — prompt: prompts/cleanup.txt                    ║");
    eprintln!("╠═══════════════════════════════════════════════════════════════════════╣");

    for (i, case) in cases.iter().enumerate() {
        let config = PostProcessConfig {
            api_key: &api_key,
            base_url: None,
            model: None,
        };

        let result = pp.process(&case.input, config);

        match result {
            Ok(actual) => {
                let lev = similarity(&actual, &case.expected);
                let rouge = rouge1_f(&actual, &case.expected);
                total_lev += lev;
                total_rouge += rouge;

                let verdict = if lev >= PASS_THRESHOLD { "PASS" } else { "FAIL" };
                if lev >= PASS_THRESHOLD {
                    pass += 1;
                } else {
                    fail += 1;
                }

                eprintln!(
                    "║ [{:>2}] {:<22} {verdict}  lev={lev:.2}  rouge1={rouge:.2}",
                    i + 1,
                    case.category
                );
                if lev < PASS_THRESHOLD {
                    eprintln!("║      input:    {}", case.input);
                    eprintln!("║      expected: {}", case.expected);
                    eprintln!("║      actual:   {actual}");
                    eprintln!("║      note:     {}", case.note);
                }
            }
            Err(e) => {
                fail += 1;
                eprintln!("║ [{:>2}] {:<22} ERROR: {e}", i + 1, case.category);
            }
        }

        // Avoid rate limits between cases
        std::thread::sleep(Duration::from_millis(500));
    }

    #[allow(clippy::cast_precision_loss)]
    let (avg_lev, avg_rouge) = if cases.is_empty() {
        (0.0, 0.0)
    } else {
        let n = cases.len() as f64;
        (total_lev / n, total_rouge / n)
    };

    eprintln!("╠═══════════════════════════════════════════════════════════════════════╣");
    eprintln!(
        "║  Results: {pass} pass, {fail} fail — avg lev={avg_lev:.2}  avg rouge1={avg_rouge:.2}"
    );
    eprintln!("╚═══════════════════════════════════════════════════════════════════════╝\n");

    assert!(
        fail == 0,
        "{fail} golden cases failed (similarity < {PASS_THRESHOLD}). Run with --nocapture for details."
    );
}

#[test]
fn golden_harness_loads_and_parses() {
    let cases = load_golden_cases();
    assert!(
        cases.len() >= 10,
        "Expected at least 10 golden cases, found {}",
        cases.len()
    );
    for case in &cases {
        assert!(!case.input.is_empty(), "Case has empty input");
        // Category is validated by serde deserialization — no need to check emptiness.
    }
}

#[test]
fn similarity_identical_strings() {
    assert!((similarity("hello", "hello") - 1.0).abs() < f64::EPSILON);
}

#[test]
fn similarity_completely_different() {
    assert!(similarity("abc", "xyz") < 0.5);
}

#[test]
fn similarity_close_strings() {
    let score = similarity(
        "I was thinking we could use the API.",
        "I was thinking we could use the API",
    );
    assert!(score > 0.9, "Expected high similarity, got {score}");
}

#[test]
fn rouge1_identical_strings() {
    assert!((rouge1_f("hello world", "hello world") - 1.0).abs() < f64::EPSILON);
}

#[test]
fn rouge1_no_overlap() {
    assert!(rouge1_f("alpha beta", "gamma delta") < f64::EPSILON);
}

#[test]
fn rouge1_filler_removal() {
    // Removing fillers *increases* ROUGE because only correct words remain.
    let score = rouge1_f(
        "I think we should use the API",
        "um I think like we should you know use the API",
    );
    // All words in actual appear in expected → perfect precision, partial recall.
    assert!(score > 0.7, "Expected decent ROUGE-1 after filler removal, got {score}");
}

#[test]
fn rouge1_empty_strings() {
    assert!((rouge1_f("", "") - 1.0).abs() < f64::EPSILON);
    assert!(rouge1_f("hello", "") < f64::EPSILON);
    assert!(rouge1_f("", "hello") < f64::EPSILON);
}
