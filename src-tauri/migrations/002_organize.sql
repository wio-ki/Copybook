ALTER TABLE items ADD COLUMN group_name TEXT;
ALTER TABLE items ADD COLUMN deleted_at INTEGER;
CREATE INDEX items_group ON items(group_name) WHERE deleted_at IS NULL;
CREATE INDEX items_deleted ON items(deleted_at) WHERE deleted_at IS NOT NULL;
