General:
* If there are decisions to be made related to user interaction, ask before making a decision
* If you're unsure what's happening when debugging an issue because of lack of context, ask the user
* If you need data to validate a result, add logging and execute a test where feasible, if not, ask the user

Frontend:
* Prefer to use MUI layout (Stack, Grid) instead of Box and divs
* Use react-query for storage and mutation of any state backed by the backend
* Don't use sx tags for style, update the theme if new controls need adding (theme.ts), else create and use local app re-usable controls if custom styling is needed
* Use minimal css. Only use local styles (.module.css) where completely appropriate and is truly a one off local change. Prefer using variants and modify the MUI theme if this is a reusable style, or preferably choose an appropriate existing style

Backend:
* Ensure permissions are checked for each operation
* Use standard asp.net authorization/authentication checks for generated api calls
* Any schema change to EF Core entities must be accompanied by a new migration: `dotnet ef migrations add <MigrationName>` from the `backend/` directory
