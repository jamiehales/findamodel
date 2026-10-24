# AI instructions (shared)

This is the shared instruction file intended to be compatible across AI coding agents.

General:
* If there are decisions to be made related to how the user will interact with the application, you must ask before making a decision
* If you're unsure what's happening when debugging an issue because of lack of context, you must ask the user
* If you need data to validate a result or fix a bug, add logging and execute a test where feasible, if not, you must ask the user
* Never use emdash (—) in code comments or documentation; use hyphens (-) or parentheses instead
* API backward compatibility is not required - there are no external API users, and frontend and backend are always deployed in lockstep
* Scale assumptions: production catalog size is approximately 100,000 models
* Scale assumptions: a printing list usually contains approximately 20-30 models
* Given those assumptions, prefer server-side filtering/pagination/batched endpoints; avoid full-catalog fetches in UI flows and avoid N+1 per-model request patterns
* Always add tests for new backend functionality - unit tests in a `#[cfg(test)] mod tests` next to the code (using `test_support::TestDb` for a temporary SQLite database) or HTTP-level tests in `backend/tests/`
* Always run the full backend test suite (`cargo test -p findamodel`) after completing any implementation and fix any failures before finishing
* Backend code must pass `cargo fmt --all --check` and `cargo clippy -p findamodel --all-targets -- -D warnings`

Frontend:
* UI controls come from the mostlymaking ui-kit (`@jamiehales/ui-kit/mostlymaking`), a git submodule at `frontend/vendor/ui-kit` built on Radix. Use its controls before writing your own, and read its `DESIGN.md` and `CHANGELOG.md` before changing or updating it
* The design lives in the mostlymaking-design canvas artifact (https://claude.ai/artifact/QivSsJCFdYhjp1We4ekmzR): the "app-findamodel" page has the app's screens and the "next-kit" page the shared controls. A new shared control goes into ui-kit and onto that page
* Follow the design system's rules: dark only, one primary (oxblood) action per view, sentence case, one- or two-word buttons, one-sentence errors, no helper text under labels, no emoji, Lucide icons
* Style with CSS modules next to the component, using the design tokens (`var(--card)`, `var(--border)`, `var(--space-4)`, the kit's `--mm-*` edges and shadows) rather than raw colors. Use the `.mm-type-*` classes for type styles
* App CSS modules are unlayered and override the kit's `components` layer; if you set `display` on a kit element that Radix hides with `hidden` (e.g. a TabsContent), add a matching `[hidden] { display: none }` rule
* Global resets go in the `base` layer in `src/styles/global.css` so they never override kit controls
* Use react-query for storage and mutation of any state backed by the backend
* Ensure frontend code matches Prettier formatting (`frontend/.prettierrc.json`). After frontend edits, run formatting on changed files (or `yarn --cwd frontend format`) before finalizing.

Backend:
* The backend is a Rust crate (`backend/`, axum + sqlx/SQLite); the Tauri desktop app embeds it as a library
* Ensure permissions are checked for each operation: API handlers take the `CurrentUser` extractor, and printing list operations go through `Caller` ownership checks
* Any database schema change must be a new numbered SQL migration in `backend/migrations/` (never edit an existing migration), with matching updates to the row types in `backend/src/entities.rs`
* Performance monitor: model list/query endpoints should filter, sort and paginate in SQL and only load the columns they need; avoid loading full model rows for the whole catalog
* Scan config checksum: `scan_config::compute(raft_height_mm)` in `backend/src/services/scan_config.rs` is the single canonical source for the hull/scan staleness checksum. If any new input affects hull generation (e.g. a new algorithm version constant, mesh tolerance, additional raft parameter), add it to that function's format string AND bump `CURRENT_HULL_GENERATION_VERSION` in `backend/src/geometry/hull.rs`. Do not add staleness checks anywhere else.
* Bump `CURRENT_PREVIEW_GENERATION_VERSION` in `backend/src/preview/mod.rs` whenever preview output changes
