-- Covers every column the catalog filters and facet options read, so those queries scan
-- this narrow index instead of the model rows (which carry the hull geometry).
-- effective_supported is virtual, so the index holds the two columns it is computed from;
-- SQLite does not treat an indexed virtual column as covered.
CREATE INDEX ix_models_facets ON models (
    calculated_creator,
    calculated_collection,
    calculated_subcollection,
    calculated_category,
    calculated_type,
    calculated_material,
    file_type,
    calculated_supported,
    detected_supported,
    calculated_model_name,
    file_name,
    id
);
