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
* Always add tests for new backend functionality - unit or integration tests in `backend.Tests/` using the xUnit + EF InMemory/SQLite pattern already present
* Always run the full backend test suite (`dotnet test backend.Tests/findamodel.Tests.csproj`) after completing any implementation and fix any failures before finishing

Frontend:
* Prefer usage of MUI layout (Stack, Grid) instead of Box and divs
* Use react-query for storage and mutation of any state backed by the backend
* Don't use sx tags for style, update the theme if new controls need adding (theme.ts), else create and use local app re-usable controls if custom styling is needed
* Use minimal css. Only use local styles (.module.css) where completely appropriate and is truly a one off local change. Prefer using variants and modify the MUI theme if this is a reusable style, or preferably choose an appropriate existing style
* Ensure frontend code matches Prettier formatting (`frontend/.prettierrc.json`). After frontend edits, run formatting on changed files (or `yarn --cwd frontend format`) before finalizing.

Backend:
* Ensure permissions are checked for each operation
* Use standard asp.net authorization/authentication checks for generated api calls
* If there are any changes to EF Core entities they must be accompanied by a new EF migration
* Performance monitor: `ToModelDto()` and similar extension-method mapping can cause full-entity materialization in EF Core queries. If model list/query endpoints become slow, prioritize SQL-translatable projection (`Select` with expression) and `AsNoTracking()` on read paths.
* Scan config checksum: `ScanConfig.Compute(float raftHeightMm)` in `backend/Services/ScanConfig.cs` is the single canonical source for the hull/scan staleness checksum. If any new input affects hull generation (e.g. a new algorithm version constant, mesh tolerance, additional raft parameter), add it to that method's format string AND bump `HullCalculationService.CurrentHullGenerationVersion`. Do not add staleness checks anywhere else.
