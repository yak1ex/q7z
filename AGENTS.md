# AGENTS.md

Operating instructions for coding agents working in `q7z`. Read this file and `TASKS.md` before changing the repository.

## 1. Non-negotiable rules

1. Do not fabricate repository facts, 7-Zip behavior, IPC behavior, commands, paths, or verification results. Inspect or run them.
2. Make the smallest change that satisfies the requested outcome. Do not perform unrelated cleanup, redesign, reformatting, or feature work.
3. Preserve unrelated user changes in the working tree.
4. Do not add, remove, replace, upgrade, or materially change features of npm or Cargo dependencies without prior user approval.
5. Do not commit, rewrite history, push, package, or publish unless the user explicitly requests it.
6. Keep `TASKS.md` synchronized with meaningful implementation progress in the same work cycle.
7. Do not expose, log, copy, or commit credentials, archive passwords, private file contents, or personal data.

## 2. Project context

- Project: `q7z`
- Objective: Provide a Windows desktop application that queues 7-Zip extraction requests in one persistent worker and displays their progress and results.
- Primary languages: Rust and TypeScript
- Frameworks: Tauri 1, Tokio, Vite
- Runtime and deployment target: Windows desktop
- External runtime requirements: `7z.exe` must be installed separately and resolvable through `PATH`
- Current maturity: early prototype; consult `TASKS.md` before assuming a feature is complete

## 3. Repository orientation

Before editing:

1. Read `TASKS.md` and identify the affected stable task.
2. Inspect the files to be changed together with their callers, event counterparts, configuration, and adjacent conventions.
3. Inspect `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, and `src-tauri/Cargo.lock` before selecting commands or proposing dependencies.
4. Run `git status --short` and preserve unrelated changes, including generated lockfile changes that predate the task.
5. State a brief plan and verifiable success criteria for non-trivial work.
6. Resolve ambiguity from repository evidence when possible; ask only when materially different interpretations remain.

Repository layout:

- `src/`: plain TypeScript frontend and CSS
- `src-tauri/src/`: Rust application, local-socket IPC, and 7-Zip process integration
- `src-tauri/tauri.conf.json`: window, CLI, bundling, and Tauri configuration
- `public/` and `src-tauri/icons/`: static assets and application icons
- `agentic-coding-templates/`: reference templates; do not require agents to read them because this file is authoritative
- `node_modules/`, `dist/`, and `src-tauri/target/`: dependencies or generated output; do not edit manually
- Automated tests: none currently configured

## 4. Implementation constraints

- Treat the Rust backend and TypeScript frontend as cooperating systems. Keep event names, payload types, queue states, and UI handling synchronized.
- Preserve the single-worker design unless an accepted task explicitly changes it. Requests from later invocations must not execute concurrently by accident.
- Use structured, validated IPC data for any protocol evolution. Do not extend the current NUL-delimited format without considering framing, invalid input, compatibility, and acknowledgements.
- Treat archive paths, output paths, filters, decoded process output, and socket messages as untrusted boundary input.
- Construct `7z.exe` arguments so Windows paths containing spaces and non-ASCII characters remain single arguments. Do not interpolate a shell command.
- Keep blocking process and IPC work off the UI thread and avoid blocking Tokio runtime threads.
- Report recoverable I/O, IPC, parsing, encoding, process-launch, and process-exit errors rather than replacing them with `unwrap()`, `expect()`, or silent failure.
- When changing 7-Zip output parsing, cover carriage-return progress records, newline log records, malformed text, completion, and non-zero exit status.
- The current code uses the Windows OEM code page and Windows APIs. Do not claim or introduce cross-platform support without deliberate target gating and platform verification.
- Do not bundle or download `7z.exe` unless the user explicitly approves a redistribution and packaging plan.
- Preserve CLI compatibility unless the active task explicitly defines a migration.

## 5. Dependency policy

Prefer the standard library and dependencies already declared by the repository. Prior user approval is required before adding, removing, replacing, upgrading, downgrading, or materially changing features of a package or crate.

Before requesting approval, explain the need, existing or standard-library alternatives, expected maintenance and runtime impact, and the manifests and lockfiles that would change. Synchronizing dependencies already declared by the repository is allowed.

Use npm because `package-lock.json` is authoritative. Treat `Cargo.toml` and `Cargo.lock` as the Rust dependency and resolution sources.

## 6. Verification and completion

- Define success as observable behavior plus evidence, not merely a plausible diff.
- Run focused checks during iteration and the relevant broader checks before finalizing.
- Read all command output and report failures, timeouts, skipped checks, and environmental limitations accurately.
- A frontend production build must pass; a development server alone is insufficient.
- Run relevant Rust formatting, compilation, and test commands after backend changes.
- Do not claim Windows integration from compilation alone. Live local-socket forwarding, `7z.exe` invocation, OEM decoding, progress display, cancellation, file output, and packaging require Windows manual verification when they cannot be exercised adequately.
- UI appearance and usability remain `in progress` with `Awaiting user verification` until manually confirmed when adequate automated verification is unavailable.
- Documentation-only work may be marked `completed` after checking it against the repository and confirming that no template placeholders remain.

## 7. Task management

`TASKS.md` is the authoritative record of project objective, current state, accepted mainline work, verification evidence, and project decisions.

For each meaningful implementation step:

1. Update the affected stable task ID in the same work cycle.
2. Record factual implementation, verification, remaining work, blockers, and user-verification needs.
3. Append newly accepted work using the next unused `T` ID.
4. Preserve existing IDs; do not reuse, renumber, or substantially reorganize them without explicit user approval.
5. Use exactly one status: `pending`, `in progress`, `blocked`, `completed`, or `cancelled`.
6. Do not equate code written with completion.
7. Review `TASKS.md` for consistency before responding.

## 8. Git and workspace safety

- Inspect the working tree before and after work.
- Do not stage, discard, overwrite, or reformat unrelated user changes.
- Avoid destructive Git commands unless explicitly requested and their consequences are clear.
- Do not commit by default. If a commit is explicitly requested, include the corresponding `TASKS.md` update and follow the repository's existing concise imperative style.
- Pushing and publishing require separate explicit requests.
- Do not add AI attribution unless explicitly requested.

## 9. Commands

Run frontend commands from the repository root and Cargo commands from `src-tauri/` unless noted otherwise.

- Install/synchronize frontend dependencies: `npm ci`
- Run frontend development server: `npm run dev`
- Run Tauri development application: `npm run tauri dev`
- Build frontend: `npm run build`
- Build/package Tauri application: `npm run tauri build`
- Frontend tests: N/A
- Frontend lint: N/A
- Frontend format check: N/A
- Frontend type check: `npx tsc --noEmit`
- Rust format check: `cargo fmt --check`
- Rust static check: `cargo check --locked`
- Rust tests: `cargo test --locked`
- Rust lint: N/A; no project-specific Clippy command is configured

Do not invent missing scripts or introduce a quality-tool stack solely to satisfy these instructions.

## 10. Project learnings

- The production frontend target includes Safari 13 for non-Windows builds; top-level `await` currently makes `npm run build` fail. Preserve target compatibility or deliberately revise the supported-target policy.
- The first q7z process currently prints its CLI arguments but does not enqueue them; do not treat first-launch and forwarded-job behavior as equivalent until this is fixed and verified.
- The condition guarding progress values currently uses mutually exclusive comparisons and therefore rejects nothing; validate finite numeric values and the inclusive 0–100 range.
- A long-running or timed-out check is not a passing check. Record the timeout and rerun a focused command when practical.

## 11. Final response checklist

- List changed files.
- Report checks actually run and their results.
- State changes to `TASKS.md` statuses or evidence.
- Distinguish verified behavior from unverified Windows, UI, external-tool, and packaging behavior.
- Mention blockers, risks, and manual verification still required.
