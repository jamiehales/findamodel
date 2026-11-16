-- Remeshed twin lookup ([`crate::services::remesh_twins`]): the twin lookup's fallback for a
-- supported model whose twin was remeshed or decimated, so no exact twin is found.
--
-- Each model's shape descriptor (thick-point D2 distribution, thick area and spreads), written by
-- the scan from the loaded geometry with its twin sketch. `described_at` is when the descriptor
-- last changed: supported models looked up before it are looked up again against it.
CREATE TABLE model_remesh_descriptors (
    model_id BLOB PRIMARY KEY NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    descriptor BLOB NOT NULL,
    described_at TEXT NOT NULL
);
CREATE INDEX model_remesh_descriptors_described_at ON model_remesh_descriptors (described_at);
