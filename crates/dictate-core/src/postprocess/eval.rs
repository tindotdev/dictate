//! Evaluation harness for post-processing prompt quality.
//!
//! Provides golden-case evaluation: a set of known input/expected pairs
//! scored against the live API to measure prompt effectiveness.
//!
//! Run with: `just eval-prompt` or `cargo test -p dictate-core golden -- --ignored --nocapture`

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
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
/// Uses multiset (frequency-count) intersection so that repeated tokens
/// contribute correctly to precision and recall denominators.
///
/// Returns 0.0 when there is no word overlap and 1.0 for identical texts.
#[allow(clippy::cast_precision_loss)]
pub fn rouge1_f(actual: &str, expected: &str) -> f64 {
    let actual_counts = unigram_counts(actual);
    let expected_counts = unigram_counts(expected);

    let actual_len: usize = actual_counts.values().sum();
    let expected_len: usize = expected_counts.values().sum();

    if actual_len == 0 && expected_len == 0 {
        return 1.0;
    }
    if actual_len == 0 || expected_len == 0 {
        return 0.0;
    }

    let overlap: usize = actual_counts
        .iter()
        .filter_map(|(token, &count)| expected_counts.get(token).map(|&exp| count.min(exp)))
        .sum();

    let precision = overlap as f64 / actual_len as f64;
    let recall = overlap as f64 / expected_len as f64;

    if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    }
}

/// Count unigram frequencies in whitespace-tokenised text.
fn unigram_counts(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }
    counts
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
            system_prompt: None,
            temperature: None,
        };

        let result = pp.process(&case.input, config);

        match result {
            Ok(actual) => {
                let lev = similarity(&actual, &case.expected);
                let rouge = rouge1_f(&actual, &case.expected);
                total_lev += lev;
                total_rouge += rouge;

                let verdict = if lev >= PASS_THRESHOLD {
                    "PASS"
                } else {
                    "FAIL"
                };
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
    assert!(
        score > 0.7,
        "Expected decent ROUGE-1 after filler removal, got {score}"
    );
}

#[test]
fn rouge1_empty_strings() {
    assert!((rouge1_f("", "") - 1.0).abs() < f64::EPSILON);
    assert!(rouge1_f("hello", "") < f64::EPSILON);
    assert!(rouge1_f("", "hello") < f64::EPSILON);
}

#[test]
fn rouge1_repetition_penalises_precision() {
    // "foo foo foo" vs "foo": overlap = min(3,1) = 1, precision = 1/3, recall = 1/1, F1 = 0.5
    let score = rouge1_f("foo foo foo", "foo");
    assert!(
        (score - 0.5).abs() < f64::EPSILON,
        "Expected F1 = 0.5 for repeated hypothesis, got {score}"
    );
}

#[test]
fn rouge1_repetition_penalises_recall() {
    // "foo" vs "foo foo foo": overlap = min(1,3) = 1, precision = 1/1, recall = 1/3, F1 = 0.5
    let score = rouge1_f("foo", "foo foo foo");
    assert!(
        (score - 0.5).abs() < f64::EPSILON,
        "Expected F1 = 0.5 for repeated reference, got {score}"
    );
}

#[test]
fn rouge1_mixed_repetition() {
    // "a a b" vs "a b b": overlap = min(2,1)+min(1,2) = 1+1 = 2, prec = 2/3, rec = 2/3, F1 = 2/3
    let score = rouge1_f("a a b", "a b b");
    let expected = 2.0 / 3.0;
    assert!(
        (score - expected).abs() < 1e-10,
        "Expected F1 ≈ {expected:.4}, got {score}"
    );
}

// ──── Matrix evaluation ──────────────────────────────────────────────────

const PROMPT_CURRENT: &str = include_str!("prompts/cleanup.txt");

/// Models to evaluate in the matrix.
const MODELS: &[&str] = &[
    "llama-3.1-8b-instant",
    "meta-llama/llama-4-scout-17b-16e-instruct",
    "openai/gpt-oss-20b",
    "llama-3.3-70b-versatile",
    "openai/gpt-oss-120b",
];

/// Load candidate prompt files from the `prompts/candidates/` directory.
///
/// Returns an empty list when the directory is missing or empty — this is
/// expected in CI where candidates are gitignored.  Drop a `.txt` file
/// into `candidates/` and it will be automatically picked up by the next
/// `just eval-matrix` run.
fn load_candidate_prompts() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/postprocess/prompts/candidates");

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };

    let mut candidates: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "txt"))
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let text =
                std::fs::read_to_string(e.path()).expect("failed to read candidate prompt file");
            (name, text)
        })
        .collect();

    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    candidates
}

/// Per-combo aggregate scores.
struct ComboResult {
    model: String,
    prompt_name: String,
    pass: usize,
    fail: usize,
    avg_lev: f64,
    avg_rouge: f64,
}

fn print_matrix_combo_header(model: &str, prompt_name: &str) {
    eprintln!("\n╔═══════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║  Model: {model:<30}  Prompt: {prompt_name:<20} ║");
    eprintln!("╠═══════════════════════════════════════════════════════════════════════════╣");
}

fn print_matrix_combo_footer(pass: usize, fail: usize, avg_lev: f64, avg_rouge: f64) {
    eprintln!("╠═══════════════════════════════════════════════════════════════════════════╣");
    eprintln!(
        "║  Results: {pass} pass, {fail} fail — avg lev={avg_lev:.2}  avg rouge1={avg_rouge:.2}"
    );
    eprintln!("╚═══════════════════════════════════════════════════════════════════════════╝");
}

#[allow(clippy::cast_precision_loss)]
fn combo_averages(total_lev: f64, total_rouge: f64, case_count: usize) -> (f64, f64) {
    if case_count == 0 {
        (0.0, 0.0)
    } else {
        let n = case_count as f64;
        (total_lev / n, total_rouge / n)
    }
}

fn evaluate_combo(
    api_key: &str,
    cases: &[GoldenCase],
    model: &str,
    prompt_name: &str,
    prompt_text: &str,
) -> ComboResult {
    let pp = GroqPostProcessor;
    print_matrix_combo_header(model, prompt_name);

    let mut pass = 0;
    let mut fail = 0;
    let mut total_lev = 0.0;
    let mut total_rouge = 0.0;

    for (i, case) in cases.iter().enumerate() {
        let config = PostProcessConfig {
            api_key,
            base_url: None,
            model: Some(model),
            system_prompt: Some(prompt_text),
            temperature: Some(0.0),
        };

        let result = pp.process(&case.input, config);
        match result {
            Ok(actual) => {
                let lev = similarity(&actual, &case.expected);
                let rouge = rouge1_f(&actual, &case.expected);
                total_lev += lev;
                total_rouge += rouge;

                let passed = lev >= PASS_THRESHOLD;
                let verdict = if passed { "PASS" } else { "FAIL" };
                if passed {
                    pass += 1;
                } else {
                    fail += 1;
                }

                eprintln!(
                    "║ [{:>2}] {:<22} {verdict}  lev={lev:.2}  rouge1={rouge:.2}",
                    i + 1,
                    case.category
                );
                if !passed {
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

        // Rate-limit between cases.
        std::thread::sleep(Duration::from_millis(500));
    }

    let (avg_lev, avg_rouge) = combo_averages(total_lev, total_rouge, cases.len());
    print_matrix_combo_footer(pass, fail, avg_lev, avg_rouge);

    ComboResult {
        model: model.to_owned(),
        prompt_name: prompt_name.to_owned(),
        pass,
        fail,
        avg_lev,
        avg_rouge,
    }
}

fn run_matrix_eval(api_key: &str, cases: &[GoldenCase]) -> Vec<ComboResult> {
    let candidates = load_candidate_prompts();

    // Production prompt is always first.
    let mut prompts: Vec<(&str, &str)> = vec![("cleanup.txt", PROMPT_CURRENT)];
    for (name, text) in &candidates {
        prompts.push((name, text));
    }

    eprintln!(
        "Evaluating {} prompt(s) × {} model(s)\n",
        prompts.len(),
        MODELS.len()
    );

    let mut results = Vec::new();

    for &(prompt_name, prompt_text) in &prompts {
        for &model in MODELS {
            let result = evaluate_combo(api_key, cases, model, prompt_name, prompt_text);
            results.push(result);
            // Rate-limit between combos.
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    results
}

fn print_matrix_summary(results: &[ComboResult]) {
    eprintln!(
        "\n┌─────────────────────────────────────────────────────────────────────────────────────────┐"
    );
    eprintln!(
        "│  MATRIX SUMMARY                                                                         │"
    );
    eprintln!(
        "├──────────────────────────────────────┬────────────────┬──────┬──────┬────────┬───────────┤"
    );
    eprintln!(
        "│ Model                                │ Prompt         │ Pass │ Fail │ Avg Lev│ Avg ROUGE │"
    );
    eprintln!(
        "├──────────────────────────────────────┼────────────────┼──────┼──────┼────────┼───────────┤"
    );

    for r in results {
        eprintln!(
            "│ {:<36} │ {:<14} │ {:>4} │ {:>4} │ {:>5.2}  │ {:>8.2}  │",
            r.model, r.prompt_name, r.pass, r.fail, r.avg_lev, r.avg_rouge
        );
    }

    eprintln!(
        "└──────────────────────────────────────┴────────────────┴──────┴──────┴────────┴───────────┘\n"
    );
}

/// Run all golden cases across N models × M prompts and print a comparison table.
///
/// The production prompt (`cleanup.txt`) is always included.  Any `.txt` files
/// in `prompts/candidates/` are automatically discovered and added to the matrix.
///
/// Requires `GROQ_API_KEY` environment variable. Skipped in normal CI.
///
/// ```bash
/// just eval-matrix
/// ```
#[test]
#[ignore = "hits live Groq API — run with: just eval-matrix"]
fn matrix_eval_models_x_prompts() {
    let api_key = match std::env::var("GROQ_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            eprintln!("GROQ_API_KEY not set — skipping matrix eval");
            return;
        }
    };

    let cases = load_golden_cases();
    let results = run_matrix_eval(&api_key, &cases);
    print_matrix_summary(&results);
}
