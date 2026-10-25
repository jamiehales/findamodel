-- Supports found in a model's geometry, stored apart from calculated_supported (from
-- metadata) so re-applying metadata never clears it. Metadata wins when set.
ALTER TABLE models ADD COLUMN detected_supported INTEGER;
ALTER TABLE models ADD COLUMN effective_supported INTEGER
    GENERATED ALWAYS AS (COALESCE(calculated_supported, detected_supported)) VIRTUAL;
