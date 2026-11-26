# CLI output fixtures

Output of real Codex and Claude runs, cut down for the failure tests (crates/uis-core/src/cli/errors.rs).
Paths, thread ids, session ids and request ids are replaced with made-up values.

- codex-image-ok.jsonl, codex-text-ok.jsonl: runs that went through
- codex-model.jsonl, codex-auth.jsonl, codex-no-tool.jsonl: an unknown model, no sign-in, no image runner
- codex-limit.jsonl: made up (a real usage-limit message has not been seen yet)
- claude-ok.json, claude-auth.json, claude-model.out/.err: Claude Code CLI with --output-format json
