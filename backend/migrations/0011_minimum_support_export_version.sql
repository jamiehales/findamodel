-- Installs from before export entries had their minimum support export version set to 0 by
-- 0010, so their next scan would not rebuild entries older than the first reader version. Raise
-- it to 1 (the reader version 0010 shipped with) where it is lower; fresh installs seed it at
-- the current version when their config row is created.
UPDATE app_config SET minimum_support_export_version = 1 WHERE minimum_support_export_version < 1;
