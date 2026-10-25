-- Time each file spent in each indexing stage, in milliseconds. NULL when the stage didn't run.
ALTER TABLE index_run_files ADD COLUMN checksum_ms REAL;
ALTER TABLE index_run_files ADD COLUMN load_ms REAL;
ALTER TABLE index_run_files ADD COLUMN supports_ms REAL;
ALTER TABLE index_run_files ADD COLUMN preview_ms REAL;
ALTER TABLE index_run_files ADD COLUMN hull_ms REAL;
