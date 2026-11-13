-- Repaired (watertight) model meshes are files in the data folder, named by the content key of
-- the mesh each was repaired from and the repair version. This table indexes them for the scan's
-- sweep: the key of the mesh a model's repair was made from, and what that mesh was made from
-- (the model file's checksum, scan config and supported metadata). A row whose source no longer
-- matches its model is stale, and a file no current row names is deleted.
CREATE TABLE model_repaired_meshes (
    model_id BLOB PRIMARY KEY NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    mesh_key TEXT NOT NULL,
    source TEXT NOT NULL
);
