-- Each model's spin mesh (the reduced body the gallery turns while a tile is hovered): its file
-- in the renders folder, relative, and the spin mesh version it was written with. NULL until the
-- scan writes one, so existing models get theirs on the next scan without redrawing previews.
ALTER TABLE models ADD COLUMN spin_mesh_path TEXT;
ALTER TABLE models ADD COLUMN spin_mesh_version INTEGER;
