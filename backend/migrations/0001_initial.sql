-- Initial schema. Ids are UUID blobs; timestamps are RFC 3339 UTC text.

CREATE TABLE users (
    id BLOB PRIMARY KEY NOT NULL,
    username TEXT NOT NULL UNIQUE,
    is_admin INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE app_config (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    default_raft_height_mm REAL NOT NULL DEFAULT 2,
    theme TEXT NOT NULL DEFAULT 'nord',
    generate_previews_enabled INTEGER NOT NULL DEFAULT 1,
    minimum_preview_generation_version INTEGER NOT NULL DEFAULT 0,
    setup_completed INTEGER NOT NULL DEFAULT 0,
    models_directory_path TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE directory_configs (
    id BLOB PRIMARY KEY NOT NULL,
    directory_path TEXT NOT NULL UNIQUE,
    parent_id BLOB REFERENCES directory_configs (id) ON DELETE RESTRICT,
    raw_creator TEXT,
    raw_collection TEXT,
    raw_subcollection TEXT,
    raw_tags_json TEXT,
    raw_category TEXT,
    raw_type TEXT,
    raw_material TEXT,
    raw_supported INTEGER,
    raw_model_name TEXT,
    raw_part_name TEXT,
    raw_raft_height_mm REAL,
    creator TEXT,
    collection TEXT,
    subcollection TEXT,
    model_name TEXT,
    part_name TEXT,
    tags_json TEXT,
    category TEXT,
    item_type TEXT,
    material TEXT,
    supported INTEGER,
    raft_height_mm REAL,
    raw_rules_yaml TEXT,
    resolved_rules_yaml TEXT,
    local_config_file_hash TEXT,
    raw_model_metadata_json TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE models (
    id BLOB PRIMARY KEY NOT NULL,
    checksum TEXT NOT NULL,
    file_name TEXT NOT NULL,
    directory TEXT NOT NULL,
    file_type TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    file_modified_at TEXT NOT NULL,
    cached_at TEXT NOT NULL,
    preview_image_path TEXT,
    preview_generated_at TEXT,
    preview_generation_version INTEGER,
    directory_config_id BLOB REFERENCES directory_configs (id) ON DELETE SET NULL,
    calculated_creator TEXT,
    calculated_collection TEXT,
    calculated_subcollection TEXT,
    calculated_tags_json TEXT,
    calculated_category TEXT,
    calculated_type TEXT,
    calculated_material TEXT,
    calculated_supported INTEGER,
    calculated_model_name TEXT,
    calculated_part_name TEXT,
    convex_hull TEXT,
    concave_hull TEXT,
    convex_sans_raft_hull TEXT,
    hull_generation_version INTEGER,
    hull_raft_height_mm REAL,
    scan_config_checksum TEXT,
    hull_generated_at TEXT,
    dimension_x_mm REAL,
    dimension_y_mm REAL,
    dimension_z_mm REAL,
    sphere_centre_x REAL,
    sphere_centre_y REAL,
    sphere_centre_z REAL,
    sphere_radius REAL,
    geometry_calculated_at TEXT,
    UNIQUE (directory, file_name)
);

CREATE INDEX ix_models_checksum ON models (checksum);
CREATE INDEX ix_models_metadata ON models (calculated_creator, calculated_collection, calculated_subcollection, calculated_model_name);
CREATE INDEX ix_models_category ON models (calculated_category);
CREATE INDEX ix_models_file_type ON models (file_type);

-- One row per (model, tag) so tag filters and facet lists use an index instead of
-- scanning JSON. tag_key is the case-folded tag.
CREATE TABLE model_tags (
    model_id BLOB NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    tag_key TEXT NOT NULL,
    PRIMARY KEY (model_id, tag_key)
);

CREATE INDEX ix_model_tags_tag_key ON model_tags (tag_key, model_id);

CREATE TABLE printer_configs (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    bed_width_mm REAL NOT NULL,
    bed_depth_mm REAL NOT NULL,
    is_built_in INTEGER NOT NULL DEFAULT 0,
    is_default INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX ix_printer_configs_is_default ON printer_configs (is_default);

CREATE TABLE printing_lists (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    owner_id BLOB NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    is_active INTEGER NOT NULL DEFAULT 0,
    is_default INTEGER NOT NULL DEFAULT 0,
    spawn_type TEXT NOT NULL DEFAULT 'grouped',
    hull_mode TEXT NOT NULL DEFAULT 'convex',
    printer_config_id BLOB REFERENCES printer_configs (id) ON DELETE SET NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX ix_printing_lists_owner ON printing_lists (owner_id);

CREATE TABLE printing_list_items (
    id BLOB PRIMARY KEY NOT NULL,
    printing_list_id BLOB NOT NULL REFERENCES printing_lists (id) ON DELETE CASCADE,
    model_id BLOB NOT NULL,
    quantity INTEGER NOT NULL,
    UNIQUE (printing_list_id, model_id)
);

CREATE TABLE metadata_dictionary_values (
    id BLOB PRIMARY KEY NOT NULL,
    field TEXT NOT NULL,
    value TEXT NOT NULL,
    normalized_value TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (field, normalized_value)
);

CREATE INDEX ix_metadata_dictionary_values_field_value ON metadata_dictionary_values (field, value);

CREATE TABLE index_runs (
    id BLOB PRIMARY KEY NOT NULL,
    directory_filter TEXT,
    relative_model_path TEXT,
    flags INTEGER NOT NULL,
    requested_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    duration_ms REAL,
    status TEXT NOT NULL,
    outcome TEXT,
    error TEXT,
    total_files INTEGER,
    processed_files INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX ix_index_runs_requested_at ON index_runs (requested_at);
CREATE INDEX ix_index_runs_status ON index_runs (status);

CREATE TABLE index_run_files (
    id BLOB PRIMARY KEY NOT NULL,
    index_run_id BLOB NOT NULL REFERENCES index_runs (id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    file_type TEXT NOT NULL,
    status TEXT NOT NULL,
    is_new INTEGER NOT NULL DEFAULT 0,
    was_updated INTEGER NOT NULL DEFAULT 0,
    generated_preview INTEGER NOT NULL DEFAULT 0,
    generated_hull INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    duration_ms REAL,
    processed_at TEXT,
    UNIQUE (index_run_id, relative_path)
);

CREATE TABLE index_run_events (
    id BLOB PRIMARY KEY NOT NULL,
    index_run_id BLOB NOT NULL REFERENCES index_runs (id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    relative_path TEXT
);

CREATE INDEX ix_index_run_events_run_created ON index_run_events (index_run_id, created_at);

-- Seed data: built-in printers and default metadata dictionary values.
INSERT INTO printer_configs (id, name, bed_width_mm, bed_depth_mm, is_built_in, is_default) VALUES
    (X'8a04c3c1f5e04d95a7b82c9d1e3f4a5b', 'Uniformation GK2', 228, 128, 1, 0),
    (X'9b15d4d2a6f15e06b8c93d0e2f4e5b6c', 'Uniformation GK3', 312, 180, 1, 1);

INSERT INTO metadata_dictionary_values (id, field, value, normalized_value, created_at, updated_at) VALUES
    (X'70f5e4fb7d9e4f74aec82038c2780bb6', 'category', 'Bust', 'bust', '2026-04-11T00:00:00Z', '2026-04-11T00:00:00Z'),
    (X'487d2907245f4af9835357507c63bd0f', 'category', 'Miniature', 'miniature', '2026-04-11T00:00:00Z', '2026-04-11T00:00:00Z'),
    (X'cb0ea64cb5694301b8eb9534f4c7f395', 'category', 'Uncategorized', 'uncategorized', '2026-04-11T00:00:00Z', '2026-04-11T00:00:00Z'),
    (X'34a684ada67c45989d098a13d9036685', 'type', 'Whole', 'whole', '2026-04-11T00:00:00Z', '2026-04-11T00:00:00Z'),
    (X'f47d5f95c9954025bcb52ec644ca4d0a', 'type', 'Part', 'part', '2026-04-11T00:00:00Z', '2026-04-11T00:00:00Z'),
    (X'8e89c7c7f7c445de97f708765c83f905', 'material', 'FDM', 'fdm', '2026-04-11T00:00:00Z', '2026-04-11T00:00:00Z'),
    (X'e6034f5a49194ac094887f42e2ec4dcf', 'material', 'Resin', 'resin', '2026-04-11T00:00:00Z', '2026-04-11T00:00:00Z'),
    (X'71132eb0f1764513b44e88f7502675dc', 'material', 'All', 'all', '2026-04-11T00:00:00Z', '2026-04-11T00:00:00Z');
