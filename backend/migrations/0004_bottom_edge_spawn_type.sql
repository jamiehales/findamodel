-- The "center" spawn type became "bottomEdge".
UPDATE printing_lists SET spawn_type = 'bottomEdge' WHERE spawn_type = 'center';
