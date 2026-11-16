CREATE TABLE items (
 id INTEGER PRIMARY KEY,
 kind TEXT NOT NULL CHECK(kind IN ('text','image','files')),
 hash BLOB NOT NULL,
 sub_kind TEXT,
 text TEXT NOT NULL,
 text_length INTEGER NOT NULL,
 line_count INTEGER NOT NULL,
 html TEXT,
 rtf BLOB,
 image_json TEXT,
 paths_json TEXT,
 source_json TEXT,
 favorite INTEGER NOT NULL DEFAULT 0,
 pinned INTEGER NOT NULL DEFAULT 0,
 note TEXT,
 use_count INTEGER NOT NULL DEFAULT 1,
 created_at INTEGER NOT NULL,
 last_used_at INTEGER NOT NULL,
 search_text TEXT NOT NULL,
 UNIQUE(kind,hash)
);
CREATE INDEX items_order ON items(pinned DESC,last_used_at DESC,id DESC);
CREATE VIRTUAL TABLE items_fts USING fts5(search_text,content='items',content_rowid='id',tokenize='trigram');
CREATE TRIGGER items_ai AFTER INSERT ON items BEGIN
 INSERT INTO items_fts(rowid,search_text) VALUES(new.id,new.search_text);
END;
CREATE TRIGGER items_ad AFTER DELETE ON items BEGIN
 INSERT INTO items_fts(items_fts,rowid,search_text) VALUES('delete',old.id,old.search_text);
END;
CREATE TRIGGER items_au AFTER UPDATE OF search_text ON items BEGIN
 INSERT INTO items_fts(items_fts,rowid,search_text) VALUES('delete',old.id,old.search_text);
 INSERT INTO items_fts(rowid,search_text) VALUES(new.id,new.search_text);
END;
