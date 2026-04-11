# GitHub Copilot Instructions

Use [AGENTS.md](../AGENTS.md) as the canonical project instruction set.
If this file and AGENTS.md differ, AGENTS.md is authoritative.

## Required reminders

- Preserve existing architecture and coding style unless asked to refactor.
- Ask before making user-facing behavior decisions.
- Add logging/tests when debugging and validating changes.
- For frontend edits, ensure output matches Prettier config and run formatting before finalizing (format touched files or run `yarn --cwd frontend format`).
- Backend changes to EF Core entities must include a migration.
- Monitor query performance: extension-method DTO mapping can trigger full-entity materialization. If list/query endpoints degrade, prefer SQL-translatable projection and `AsNoTracking()` on read paths.
