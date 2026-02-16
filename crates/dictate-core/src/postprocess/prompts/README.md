# Post-Processing Prompt Evaluation

## Overview

Post-processing in `dictate` transforms raw voice transcriptions into clean, professional text by:

- Removing filler words (um, uh, like, you know)
- Correcting punctuation and capitalization
- Preserving technical terminology
- Maintaining semantic meaning

This directory contains the prompts, test cases, and evaluation infrastructure for measuring post-processing quality.

## Directory Structure

```
prompts/
├── README.md                  # This file - methodology and guidance
├── RESULTS-YYYY-MM-DD.md      # Dated evaluation results
├── cleanup.txt                # Primary production prompt
├── golden_cases.json          # 14 test scenarios with expected outputs
└── candidates/                # Experimental prompt variations
```

## Evaluation Results

| Date       | Models | Prompts | Best Config                           | Pass Rate   | Details                                          |
| ---------- | ------ | ------- | ------------------------------------- | ----------- | ------------------------------------------------ |
| 2026-02-17 | 3      | 2       | llama-3.3-70b-versatile + cleanup.txt | 12/14 (86%) | [RESULTS-2026-02-17.md](./RESULTS-2026-02-17.md) |

## Evaluation Methodology

### Test Cases

`golden_cases.json` contains 14 scenarios across multiple categories:

- **filler_removal**: Basic filler word removal (um, uh, like, so)
- **technical_terms**: Proper capitalization (Kubernetes, NGINX, PostgreSQL)
- **punctuation**: Comma placement, sentence structure
- **mixed**: Combined challenges (fillers + technical terms + punctuation)
- **edge_case**: Boundary conditions (all-filler input, empty strings, conversational inputs)
- **meaning_preservation**: Context-dependent words (e.g., "like" as verb vs. filler)

Each test case includes:

- `input`: Raw transcription text
- `expected`: Ideal cleaned output
- `category`: Test category
- `note`: What the test validates

### Metrics

Quality is measured using two complementary metrics:

1. **Levenshtein similarity** (0.0 - 1.0): Character-level edit distance
   - 1.0 = perfect match
   - Sensitive to small differences
   - Good for catching punctuation errors

2. **ROUGE-1 F1** (0.0 - 1.0): Unigram overlap
   - Measures word-level similarity
   - More forgiving of paraphrasing
   - Better for semantic equivalence

**Pass threshold**: `lev >= 0.85 AND rouge1 >= 0.70`

### Running Evaluations

```bash
# Test single prompt against all golden cases
just eval-prompt

# Test all models × all prompts (full matrix)
just eval-matrix
```

Both require `GROQ_API_KEY` environment variable.

## Adding Test Cases

To add a new golden case, edit `golden_cases.json`:

```json
{
  "input": "your test transcription here",
  "expected": "the ideal cleaned output",
  "category": "filler_removal|technical_terms|punctuation|mixed|edge_case|meaning_preservation",
  "note": "What this test validates"
}
```

Guidelines:

- Keep inputs realistic (actual dictation patterns)
- Expected outputs should be unambiguous
- Add notes explaining what makes the case challenging
- Test one concept per case when possible
- Edge cases are valuable—don't shy away from tricky inputs

After adding cases:

1. Run `just eval-prompt` to see how the current prompt performs
2. If failures occur, consider if the prompt or expected output needs adjustment
3. Document any new failure patterns in the dated results file

## Prompt Development Workflow

When creating new prompts or modifying existing ones:

1. **Test early**: Run `just eval-prompt` during development
2. **Address edge cases**: Focus on tests #12 and #14 (conversational handling)
3. **Preserve meaning**: Ensure context-dependent words are handled correctly
4. **Avoid meta-commentary**: Models should output cleaned text only, no explanations
5. **Run full matrix**: Use `just eval-matrix` before committing prompt changes
6. **Document results**: Create a new `RESULTS-YYYY-MM-DD.md` file and update the results table above
