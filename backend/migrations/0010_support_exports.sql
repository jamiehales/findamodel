-- Pre-supported models' export entries (support mask and rebuilt supports) are files in the data
-- folder; the models row indexes each one for the scan: its path relative to the entries folder
-- and the support reader version it was written with. NULL until an entry is written. The file is
-- the authority: it holds the versions and inputs it was made from.
ALTER TABLE models ADD COLUMN support_export_path TEXT;
ALTER TABLE models ADD COLUMN support_export_version INTEGER;

-- The oldest export entry version the scan accepts; older entries are rewritten by the next scan.
ALTER TABLE app_config ADD COLUMN minimum_support_export_version INTEGER NOT NULL DEFAULT 0;

-- Time each file spent writing its export entry, in milliseconds. NULL when the stage didn't run.
ALTER TABLE index_run_files ADD COLUMN export_ms REAL;
