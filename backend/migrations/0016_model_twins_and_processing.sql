-- Twin lookup ([`crate::services::twins`]): a supported file whose supports are welded into the
-- model is split by its twin, the same sculpt without supports elsewhere in the catalogue.
--
-- Each model's twin sketch (its shape fingerprint), written by the scan from the loaded geometry
-- with its hulls. `sketched_at` is when the sketch last changed: supported models probed before
-- it are probed again against it.
CREATE TABLE model_twin_sketches (
    model_id BLOB PRIMARY KEY NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    sketch BLOB NOT NULL,
    sketched_at TEXT NOT NULL
);
CREATE INDEX model_twin_sketches_sketched_at ON model_twin_sketches (sketched_at);

-- The probe keys of each supported model waiting for a twin (nothing split off, or split by shape),
-- compacted ([`crate::geometry::twin::StoredProbes`]): it is probed again against new sketches
-- without loading its file. Written by the scan and the twin lookup for the model's content
-- (`model_checksum`); deleted once the model is split by its twins or stops waiting.
CREATE TABLE model_twin_probes (
    model_id BLOB PRIMARY KEY NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    model_checksum TEXT NOT NULL,
    probes BLOB NOT NULL,
    written_at TEXT NOT NULL
);

-- The twins a supported model's split was made from (several when its parts are separate
-- models), each with its content checksum then, its scale (supported over twin) and coverage. A
-- row whose twin has changed, is now supported or is at another twin version no longer splits
-- the model; a twin deleted takes its rows with it. Either way the model is looked up again.
CREATE TABLE model_twins (
    model_id BLOB NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    twin_id BLOB NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    twin_checksum TEXT NOT NULL,
    scale REAL NOT NULL,
    coverage REAL NOT NULL,
    version INTEGER NOT NULL,
    PRIMARY KEY (model_id, twin_id)
);
CREATE INDEX model_twins_twin_id ON model_twins (twin_id);

-- How each model was last processed, one row per kind, so problems can be traced back:
-- 'support_split' (how its supports were split off: none, shells, welded or twin), 'twin_lookup'
-- (the twin lookup's verdict) and 'repair' (the last watertight repair of its body). The method
-- and versions are columns; what else each kind records is JSON in `detail`. `source_checksum`
-- is the model file's checksum the processing was done on.
CREATE TABLE model_processing (
    model_id BLOB NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    method TEXT NOT NULL,
    source TEXT NOT NULL,
    source_checksum TEXT NOT NULL,
    detection_version INTEGER,
    reader_version INTEGER,
    twin_version INTEGER,
    repair_version INTEGER,
    duration_ms REAL,
    detail TEXT,
    processed_at TEXT NOT NULL,
    PRIMARY KEY (model_id, kind)
);
