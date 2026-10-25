-- Whether the geometry classifier found support parts in a model's mesh, confident or
-- not, so a model marked supported can tell if its supports can be split off. NULL until
-- the geometry is indexed or split.
ALTER TABLE models ADD COLUMN support_parts_found INTEGER;
