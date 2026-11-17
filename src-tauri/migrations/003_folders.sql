-- A folder survives even when it contains no clipboard records.
CREATE TABLE favorite_folders (path TEXT PRIMARY KEY NOT NULL);
-- Flat group names are migrated in Rust so literal '/' names can be escaped
-- without merging them with existing full-width names.
