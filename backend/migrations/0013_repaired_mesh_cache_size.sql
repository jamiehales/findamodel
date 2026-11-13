-- The most disk space kept repaired model meshes may take up, in MB (default 5 GB). When the
-- store grows past it, the entries read longest ago are deleted.
ALTER TABLE app_config ADD COLUMN repaired_mesh_cache_size_mb INTEGER NOT NULL DEFAULT 5120;
