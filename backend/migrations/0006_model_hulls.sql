-- Hulls move out of models so catalog scans and listings don't read them. The staleness
-- columns (hull_generation_version, scan_config_checksum, ...) stay on models.
CREATE TABLE model_hulls (
    model_id BLOB PRIMARY KEY NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    convex_hull TEXT,
    concave_hull TEXT,
    convex_sans_raft_hull TEXT
);

INSERT INTO model_hulls (model_id, convex_hull, concave_hull, convex_sans_raft_hull)
SELECT id, convex_hull, concave_hull, convex_sans_raft_hull
FROM models
WHERE convex_hull IS NOT NULL OR concave_hull IS NOT NULL OR convex_sans_raft_hull IS NOT NULL;

ALTER TABLE models DROP COLUMN convex_hull;
ALTER TABLE models DROP COLUMN concave_hull;
ALTER TABLE models DROP COLUMN convex_sans_raft_hull;
