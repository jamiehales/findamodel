# GitHub Copilot Instructions

Use [AGENTS.md](../AGENTS.md) as the canonical project instruction set.
If this file and AGENTS.md differ, AGENTS.md is authoritative.

## Required reminders

- Preserve existing architecture and coding style unless asked to refactor.
- Ask before making user-facing behavior decisions.
- Add logging/tests when debugging and validating changes.
- For frontend edits, ensure output matches Prettier config and run formatting before finalizing (format touched files or run `yarn --cwd frontend format`).
- Backend database schema changes must include a new SQL migration in `backend/migrations/`.
- Monitor query performance: filter, sort and paginate model queries in SQL and avoid loading full rows for the whole catalog.

## Restricted tasks

**DO NOT RUN THESE TASKS** - they will destroy data or require explicit user permission:
- `data: clear database` - Destructive operation. Only run if user explicitly requests it.
- `data: clear cache` - Only run if user explicitly requests it.
