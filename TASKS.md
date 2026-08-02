# TASKS.md

## Project objective

Develop a dependable Windows desktop application that accepts 7-Zip extraction requests from command-line invocations, queues them in one persistent worker, and presents current work, progress, logs, completion, and actionable failures in a compact UI.

## Scope and constraints

- In scope: single-worker extraction queueing, CLI-to-worker IPC, `7z.exe` process execution, progress and log reporting, job lifecycle UI, error handling, tests, and Windows packaging.
- Out of scope: bundling or downloading 7-Zip, archive creation, a general-purpose 7-Zip GUI, and cross-platform support unless separately approved.
- Deployment target: Windows desktop using Tauri 1.
- Important constraints: `7z.exe` is resolved through `PATH`; Windows paths and OEM-encoded output must be handled correctly; the current repository has no automated test suite; UI, IPC, external-process, and packaging behavior require Windows verification.

## Current status

The repository is an early prototype. A Tauri backend hosts a single serial extraction worker fed by an mpsc channel; a named-local-socket listener accepts forwarded jobs, and the first invocation enqueues its own request through the same channel. The IPC wire format is now line-delimited JSON (versionable via serde structs) with acknowledgement responses, replacing the prior NUL-delimited fire-and-forget protocol. Connection accepted by the worker are validated and either enqueued (positive ack with job id) or rejected (negative ack with an error string); the forwarder reads the ack and reports it. The listener is bound synchronously in `setup` via `tauri::async_runtime::block_on` to narrow the listener-start race, and on bind failure the process retries connect (another process must have just bound). `7z.exe` runs serially, percentage output is parsed and emitted to the TypeScript UI, and a `job` event plus a `current_job` command identify the active request. Process completion is awaited; "Everything is Ok" forces `percent: 100`. Out-of-scope for T003 but still pending in later tasks: full argument/IPC/path validation, exit-status-based success/failure (currently inferred from `Everything is Ok`), capturing 7z's stderr for the UI, job-state UI, automated tests for non-IPC logic, and Windows packaging.

As of 2026-08-02, `npm run build`, `cargo fmt --check`, `cargo check --locked` (no warnings), and `cargo test --locked` (13 tests) all pass on Windows.

## Active task

- Task: `T004` (robust 7-Zip execution) — next: derive success/failure from `7z.exe`'s process exit status rather than from `Everything is Ok` text matching, capture 7z stderr for the UI, ensure one failed job never terminates the worker, validate finite progress values 0–100, and verify Windows paths containing spaces and non-ASCII characters as single 7z arguments (currently constructed via `raw_arg`, which is correct but unverified for non-ASCII).
- Recently completed: `T002` restored the build baseline; `T001` prototype is complete; `T003` implemented and programmatically verified the structured IPC contract, acknowledgements, listener-start race narrowing, single validation/enqueue path, and automated parsing/validation/queue-ordering tests.

## Tasks

Use stable IDs. Append newly accepted work using the next unused ID. Never reuse an ID or renumber existing tasks without explicit user approval.

### T001 — Establish the extraction-queue prototype

- Status: completed
- Objective: Demonstrate that multiple q7z invocations can feed extraction work to one visible worker and report progress.
- Scope:
  - Define positional input, output, and optional filter arguments.
  - Forward later invocations through a Windows local socket.
  - Execute forwarded jobs serially with `7z.exe`.
  - Emit parsed percentage updates to a desktop progress bar.
- Acceptance criteria:
  - The first and subsequent invocations each enqueue exactly one valid request.
  - Requests execute one at a time in arrival order.
  - The UI identifies the current request and shows valid progress.
  - Failures do not terminate the persistent worker without a clear diagnostic.
- Implemented:
  - Added a hidden-on-start Tauri window, positional CLI configuration, named local-socket listener, and second-process forwarding.
  - Added sequential `7z.exe x` execution for forwarded requests with output directory, auto-rename, progress, and filter arguments.
  - Added OEM-code-page decoding, percentage parsing, a Tauri `percent` event, and a basic progress bar.
  - Added a single serial worker fed by a `tokio::sync::mpsc` channel; the listener enqueues forwarded jobs and the first invocation enqueues its own parsed request through the same channel, so first and later invocations follow one path and execute in arrival order.
  - Replaced panic paths in `listen_for_ipc` (accept, read, parse) and `run_7z` (spawn, read, emit) with `eprintln!` diagnostics and `continue`/early-return; a failed job no longer terminates the worker or listener.
  - Await `7z.exe` completion via `cmd.wait().await` (previously completion was not checked).
  - Added a `job` event carrying `(input, output)` and a frontend listener that writes it to `#input` so the UI identifies the current request.
- Verification:
  - Static source review completed on 2026-08-02.
  - 2026-08-02 (Windows): `npx tsc --noEmit`, `npm run build`, `cargo fmt --check`, `cargo check --locked` (no warnings), `cargo test --locked` (0 tests configured) all pass after the change.
  - 2026-08-02 (Windows, end-to-end via `npm run tauri build -- --debug` then `src-tauri\target\debug\q7z.exe`; `7z.exe` is 7-Zip 26.02 x64 on `PATH`, resolved at `C:\Users\atarashi\scoop\shims\7z.exe`):
    - **First-invocation enqueue + serial execution**: launching the worker with `archive.7z -> out1`, then a second process forwarding `archive.7z -> out2`, produced exactly one `Everything is Ok / Files: 2` per job and populated both `out1` and `out2` with `hello.txt` and `file two.txt`. Jobs ran serially in arrival order (the second `Extracting` block appears after the first `Everything is Ok`).
    - **Worker survives a failing job**: a corrupted `bad.7z` (hand-crafted invalid header) produced a clear diagnostic on the worker's stderr — `ERROR: ... Cannot open the file as [7z] archive / ERRORS: Is not archive` — and a subsequent valid invocation (`archive.7z -> out3b`) was still accepted and completed `Files: 2`. `run_7z` did not panic and the listener kept accepting.
    - **Percentage events**: a 200-file (~40 MiB) archive produced 7z percentage lines (`  0%`, ` 76% 73 - file165.bin`, ` 99% 161 - file64.bin`) captured by the worker's reader, so the `re.captures` → `emit_all("percent", ...)` path executed. All 200 files extracted.
  - 2026-08-02 (Windows, UI manual confirmation by user, `npm run tauri build -- --debug` binary):
    - **Case 1 (no prior process)**: window opens, `#input` shows `archive -> outdir`, progress bar advances and reaches full. (Initially failed — first-invocation `job` event fired before the webview's listener attached — fixed by adding a `current_job` Tauri command backed by `AppState.current_job: Mutex<Option<(String,String)>>` set at each job's start; the frontend invokes it after its listeners register and populates `#input` from the response.)
    - **Case 2 (forwarded to a running worker)**: `#input` updates to the new path and the bar resets and fills. Behavior preserved across the fix.
- Verification:
  - All four acceptance commands pass after the final edits: `npx tsc --noEmit`, `npm run build`, `cargo fmt --check`, `cargo check --locked` (no warnings), `cargo test --locked` (0 tests configured).
- Remaining:
  - Force 100% on `"Everything is Ok"` — **done** (the `percent: 100` emit on `Everything is Ok` is in place; fuller completion semantics belong to `T004`/`T005`).
  - Comprehensive argument/path/filter/IPC/progress validation — `T003` (job/IPC semantics) and `T004` (robust 7-Zip execution), including exit-status-based success/failure (currently inferred from `Everything is Ok`, not the exit code), capturing 7z's stderr, and spaces/non-ASCII argument safety.
- Notes:
  - T001 demonstration criteria are met. The listener-start race narrowed here (forwarded-vs-first-invocation path divergence) remains material to `T003`'s structured-IPC scope.

### T002 — Restore a reproducible build and check baseline

- Status: completed
- Objective: Make the current frontend and Rust application pass the repository's applicable static and build checks without changing intended behavior.
- Scope:
  - Remove the frontend's incompatible top-level-await usage or deliberately align the supported build target.
  - Run formatting, type, build, Cargo check, and Cargo test commands that apply.
  - Record environment-specific failures separately from source failures.
- Acceptance criteria:
  - `npm run build` succeeds.
  - `cargo fmt --check`, `cargo check --locked`, and `cargo test --locked` complete successfully.
  - No unrelated dependency or generated-file changes are introduced.
- Implemented:
  - Wrapped the top-level `await listen('percent', ...)` in `src/main.ts` in an `async` IIFE so ES2020/Safari 13 builds without top-level-await support; behavior preserved.
  - Applied `cargo fmt` to `src-tauri/build.rs` and `src-tauri/src/main.rs` to satisfy `cargo fmt --check`; no semantic changes.
- Verification (2026-08-02, Windows):
  - `npx tsc --noEmit`: passed (no output).
  - `npm run build`: passed (`tsc && vite build`, 9 modules transformed, 629 ms).
  - `cargo fmt --check` (from `src-tauri/`): exit 0.
  - `cargo check --locked` (from `src-tauri/`): `Finished dev profile` in 9.96 s.
  - `cargo test --locked` (from `src-tauri/`): `Finished test profile` in 2m 21s; `running 0 tests` — no automated tests are configured yet (see `T006`).
- Remaining:
  - None for `T002`. The historical `package-lock.json` modification noted in the prior task record is no longer present in the working tree; nothing to preserve or reconcile.
- Notes:
  - The build baseline is now reproducible. Subsequent tasks can use the four acceptance commands as the focused regression set before finalizing broader work.

### T003 — Define reliable job and IPC semantics

- Status: completed
- Objective: Represent every extraction request as a validated job that can be accepted, queued, rejected, and tracked predictably.
- Scope:
  - Define job identity, request fields, states, and ordering.
  - Replace or version the NUL-delimited IPC message with structured framing and acknowledgements.
  - Handle initial-launch requests, listener-start races, malformed messages, and unavailable existing workers.
- Acceptance criteria:
  - Every valid invocation receives an accepted or rejected result.
  - First and later invocations follow the same validation and enqueue path.
  - Malformed requests cannot crash or desynchronize the listener.
  - Automated tests cover message parsing and queue ordering.
- Implemented:
  - Job model: `JobRequest { input, output, filter }` (serde), `Job { id, input, output, filter }` (internal), `JobResponse { accepted, id?, error? }` (serde). IDs assigned via an `AtomicU64` in `AppState`.
  - IPC protocol v1: line-delimited JSON `{"input":..,"output":..,"filter":..}\n` for requests and `{"accepted":..,"id":..,"error":..}`\n for responses. No size on disk for the version; forward-compatible via serde (optional fields default to empty); malformed JSON parse returns `None` and the worker replies with a negative ack instead of crashing.
  - Acknowledgements: `forward_and_report` writes the request line synchronously through the generic `Stream` (sync `std::io::Write`), reads the response line synchronously (`std::io::BufRead::read_line`), parses it, prints/discusses the result, spawns an async task to dismiss the window (release shows a blocking dialog via `tauri::api::dialog::blocking::message`).
  - Listener: accepts connections via the tokio `Listener`, then spawns a per-connection `handle_connection` task that reads the request async (`AsyncBufReadExt::read_line`), validates, enqueues into the shared mpsc channel, and writes the ack back async (`AsyncWriteExt::write_all`). EOF (`Ok(0)`) on the first read returns silently so a forwarder that exited before writing any bytes is not treated as a malformed request.
  - Listener-start race: the listener is bound synchronously in `setup` via `tauri::async_runtime::block_on(async { ... create_tokio() })` (which runs on the singleton Tokio runtime that Tauri initializes before calling setup). On bind failure the process immediately retries `Stream::connect` (another process must have just bound) and, if that also fails, surfaces a single combined error instead of spawning a second worker or hanging.
  - Single validation/enqueue path: `validate_request` (rejects empty input or output after trim) and the `AtomicU64` ID assignment are shared between the first-invocation enqueue (`tx.try_send`) and the forwarded-listener enqueue (`jobs_tx.send().await`). The forwarder and the worker both use `matches_to_request` to build the `JobRequest` from Tauri's `Matches`.
- Verification (2026-08-02, Windows, `npm run tauri build -- --debug` binary; `7-Zip 26.02 x64`):
  - `cargo test --locked`: 13 tests pass — `parse_request_valid`, `parse_request_default_filter`, `parse_request_with_filter`, `parse_request_malformed_json`, `parse_response_accept`, `parse_response_reject`, `validate_accepts_nonempty`, `validate_rejects_empty_input`, `validate_rejects_empty_output`, `validate_accepts_with_filter`, `response_serialize_roundtrip`, `response_reject_roundtrip`, `queue_preserves_arrival_order`.
  - `cargo fmt --check`, `cargo check --locked` (no warnings), `npx tsc --noEmit`, `npm run build`: all pass.
  - End-to-end: launching the worker with the first invocation (`archive.7z -> outA`) and then three more invocations in sequence (valid `outB`, corrupt `bad.7z -> outC_bad`, valid `outD`) produced `outA=2`, `outB=2`, `outD=2` (all `Everything is Ok` / `Files: 2`), the corrupt archive surfaced `ERROR: Cannot open the file as [7z] archive / ERRORS: Is not archive` on the worker's stderr, the worker survived and ran the subsequent valid job, and the listener logged no spurious "failed to send ack" after the EOF fix.
  - EOF connection from a forwarder that exited before writing: the listener returns silently (no crash, no spurious ack).
  - Race narrowing: `create_listener` called via `block_on` from `setup` ensures the socket is bound before `setup` returns; a second invocation reaching the same code will fail to bind, retry connect, and forward to the first instead of spawning a second listener/worker (verified by code path, not by a deliberate two-process race scenario).
- Verification remaining (manual):
  - Release-mode forwarder dialog: the `#[cfg(not(debug_assertions))]` branch spawns `message(...)` to display a dialog and close the window. The debug binary path was exercised; release-mode behavior was not programmatically verified on Windows (needs a manual `npm run tauri build`).
- Remaining:
  - None intrinsic to T003's stated scope. Exit-status-based success/failure, 7z stderr capture, full path/argument validation, and the forwarder's `Builder::run().expect()` panic on bad CLI args belong to later tasks (`T004`, `T006`).
- Notes:
  - The CLI was unchanged — `q7z.exe <input> <output> [filter]` still works. Only the internal IPC wire format changed (NUL-delimited → line-delimited JSON); this is an internal protocol between two q7z instances and does not affect external users.

### T004 — Make 7-Zip execution robust

- Status: pending
- Objective: Execute each accepted job safely and report a complete, accurate outcome.
- Scope:
  - Construct arguments safely for Windows paths, optional filters, and non-ASCII text.
  - Detect missing `7z.exe`, launch failures, decoded output, success, non-zero exit status, and abnormal termination.
  - Ensure one failed job does not terminate or permanently stall the worker.
- Acceptance criteria:
  - Paths containing spaces and representative non-ASCII characters are preserved as single arguments.
  - Success and failure are derived from process completion and exit status, not only progress text.
  - The queue proceeds after a failed job and emits an actionable error.
  - Progress remains within 0–100 and successful completion reaches 100%.
- Implemented:
  - None beyond the prototype process runner and parser recorded in `T001`.
- Verification:
  - Not run.
- Remaining:
  - Implement process lifecycle handling, tests, and Windows integration verification.
- Notes:
  - Keep `7z.exe` external and resolved through `PATH`.

### T005 — Present queue state, progress, logs, and errors

- Status: pending
- Objective: Give the user an accurate view of the active job and enough context to understand completed and failed work.
- Scope:
  - Display the current input and output, bounded progress, and streamed log text.
  - Show pending, running, completed, and failed job states.
  - Keep event payloads and frontend state synchronized with the backend job model.
- Acceptance criteria:
  - The existing input and log elements display real job data.
  - Invalid progress payloads do not corrupt the UI.
  - Job transitions and error messages remain visible long enough to diagnose outcomes.
  - Production frontend build passes and Windows UI behavior is manually confirmed.
- Implemented:
  - The prototype contains empty input and log elements and a percentage-driven progress bar.
- Verification:
  - Static review found that input and log elements are never populated.
  - The current progress range condition can never be true because it uses `&&` between mutually exclusive comparisons.
- Remaining:
  - Implement frontend state and backend lifecycle events; add automated logic tests where practical; obtain user verification of appearance and usability.
- Notes:
  - Keep the UI compact; avoid redesign unrelated to job visibility.

### T006 — Add an automated regression baseline

- Status: pending
- Objective: Protect parsing, validation, queue ordering, and frontend progress behavior with repeatable tests.
- Scope:
  - Add Rust unit tests for IPC/request parsing, 7-Zip progress parsing, and lifecycle decisions.
  - Add the smallest appropriate frontend test approach only if existing TypeScript checks cannot cover extracted pure logic; dependency additions require approval.
  - Document manual Windows integration scenarios that automation cannot adequately cover.
- Acceptance criteria:
  - `cargo test --locked` exercises normal, malformed, boundary, and failure cases for core backend logic.
  - Frontend progress validation is covered by an executable check or test.
  - A reproducible manual scenario verifies two queued archives, a failing archive, spaces/non-ASCII paths, and worker survival.
- Implemented:
  - None.
- Verification:
  - No automated tests are currently configured or present.
- Remaining:
  - Refactor testable boundaries as needed, implement tests, and record Windows manual results.
- Notes:
  - Do not add a frontend testing dependency without prior approval.

### T007 — Verify Windows packaging and installation

- Status: pending
- Objective: Produce and manually validate a Windows package that behaves like the development application.
- Scope:
  - Build the configured Tauri package.
  - Verify launch visibility, second-invocation forwarding, external `7z.exe` discovery, icons, and clean shutdown.
  - Record supported Windows and 7-Zip versions actually exercised.
- Acceptance criteria:
  - `npm run tauri build` succeeds from a synchronized checkout.
  - An installed build accepts and completes at least two sequential extraction requests.
  - Missing `7z.exe` produces an actionable message without crashing silently.
  - Packaging verification records the tested environment.
- Implemented:
  - Tauri bundling metadata and Windows icons are present.
- Verification:
  - No successful package build or installed-app verification is recorded.
- Remaining:
  - Complete after `T003` through `T006` provide reliable behavior and checks.
- Notes:
  - Packaging success alone does not complete Windows integration verification.

## Decisions

### D001 — Target Windows desktop first

- Date: 2026-08-02
- Status: accepted
- Decision: Treat Windows desktop as the supported deployment target until cross-platform work is separately approved and implemented.
- Rationale: The implementation directly uses the Windows OEM code page API, Windows namespaced local sockets, and `7z.exe`.
- Consequences:
  - Do not claim macOS or Linux support from Tauri bundle configuration.
  - Platform-specific behavior must be verified on Windows.
- Related tasks: `T001`, `T004`, `T007`

### D002 — Keep 7-Zip external

- Date: 2026-08-02
- Status: accepted
- Decision: Resolve `7z.exe` through `PATH`; do not bundle or automatically download it.
- Rationale: The current application delegates extraction to an independently installed 7-Zip runtime and has no redistribution design.
- Consequences:
  - Startup and job errors must clearly identify a missing executable.
  - Development and package verification require an environment with 7-Zip installed.
- Related tasks: `T004`, `T007`

### D003 — Preserve a single serial extraction worker

- Date: 2026-08-02
- Status: accepted
- Decision: Route requests to one persistent worker and execute extraction jobs one at a time in arrival order.
- Rationale: Serial extraction is the defining purpose of q7z and avoids competing archive jobs consuming disk and CPU simultaneously.
- Consequences:
  - IPC acceptance, queue state, and worker recovery must be explicit.
  - Changes must not introduce accidental concurrent extraction.
- Related tasks: `T001`, `T003`, `T004`

## Blocked items

- None.

## Cancelled items

- None.

## Backlog

- Queue reordering, removal, cancellation, and retry controls.
- Persistent queue recovery after application restart.
- Drag-and-drop archive submission and Windows shell integration.
- Configurable 7-Zip executable location, extraction options, overwrite policy, and password workflow.
- Deliberate cross-platform support with target-specific process and encoding implementations.
