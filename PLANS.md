# ExecPlans

## Requirements

An ExecPlan must be:

- self-contained
- outcome-focused
- updated as work progresses

Keep it short. Prefer prose over long checklists.

## Required Sections

Every ExecPlan should include:

- Purpose
- Progress
- Decision Log
- Context
- Plan of Work
- Validation

## Dictate-Specific Guidance

- Name the exact commands to run from the repo root.
- Describe the user-visible behavior after the change.
- Call out platform differences when Linux/macOS behavior may diverge.
- State how the change affects `dictate`, `dictate retry`, launchers, or editor integrations if relevant.

## Suggested Location

- Active work: `docs/exec-plans/active/`
- Finished work: `docs/exec-plans/completed/`

## Minimal Template

```md
# <Short title>

## Purpose

What user-visible behavior is being added or changed, and how to observe it.

## Progress

- [ ] Step one
- [ ] Step two

## Decision Log

- Decision: ...
  Rationale: ...

## Context

Name the files, modules, commands, and constraints that matter.

## Plan of Work

Describe the edits in repository-relative paths.

## Validation

List the exact commands to run and the expected behavior.
```
