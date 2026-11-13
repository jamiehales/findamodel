-- The threads a mesh repair (the remeshed VOXL download and plate export) runs on.
ALTER TABLE app_config ADD COLUMN repair_threads INTEGER NOT NULL DEFAULT 20;
