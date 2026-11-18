ALTER TABLE items ADD COLUMN snippet_id TEXT;
ALTER TABLE items ADD COLUMN favorite_order INTEGER;
CREATE INDEX favorite_folder_order ON items(
    coalesce(group_name, ''), pinned DESC, (favorite_order IS NULL), favorite_order,
    last_used_at DESC, id DESC
) WHERE favorite = 1 AND deleted_at IS NULL;
