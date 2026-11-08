-- Lets the catalog search's default sort (newest file first) read the first page from an
-- index instead of sorting every model.
CREATE INDEX ix_models_file_date ON models (file_modified_at, directory, file_name, id);
