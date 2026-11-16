use crate::{detect, model::*, search, settings::Settings};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

const COLUMNS: &str="id,kind,sub_kind,substr(text,1,2000),text_length,line_count,html IS NOT NULL,rtf IS NOT NULL,image_json,paths_json,source_json,favorite,pinned,note,use_count,created_at,last_used_at";

pub struct Storage {
    conn: Connection,
    pub dir: PathBuf,
}
pub struct StoredContent {
    pub item: ClipItem,
    pub text: String,
    pub html: Option<String>,
    pub rtf: Option<Vec<u8>>,
}
fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| CmdError::internal())
}
fn from_json<T: serde::de::DeserializeOwned>(value: Option<String>) -> rusqlite::Result<Option<T>> {
    value
        .map(|s| {
            serde_json::from_str(&s).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })
        .transpose()
}
fn file_entry(path: String) -> FileEntry {
    let metadata = fs::metadata(&path).ok();
    FileEntry {
        name: Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone()),
        is_dir: metadata.as_ref().is_some_and(|m| m.is_dir()),
        exists: metadata.is_some(),
        bytes: metadata.filter(|m| m.is_file()).map(|m| m.len()),
        path,
    }
}
fn item_row(row: &Row<'_>) -> rusqlite::Result<ClipItem> {
    let kind: String = row.get(1)?;
    let paths: Option<Vec<String>> = from_json(row.get(9)?)?;
    Ok(ClipItem {
        id: row.get(0)?,
        kind: match kind.as_str() {
            "image" => ClipKind::Image,
            "files" => ClipKind::Files,
            _ => ClipKind::Text,
        },
        sub_kind: row.get(2)?,
        text: row.get(3)?,
        text_length: row.get(4)?,
        line_count: row.get(5)?,
        has_html: row.get(6)?,
        has_rtf: row.get(7)?,
        image: from_json(row.get(8)?)?,
        files: paths.map(|p| p.into_iter().map(file_entry).collect()),
        source: from_json(row.get(10)?)?,
        favorite: row.get(11)?,
        pinned: row.get(12)?,
        note: row.get(13)?,
        use_count: row.get(14)?,
        created_at: row.get(15)?,
        last_used_at: row.get(16)?,
    })
}
impl Storage {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir.join("images"))?;
        fs::create_dir_all(dir.join("icons"))?;
        let mut conn = Connection::open(dir.join("copybook.db"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(CmdError::new(
                "db",
                "Database is newer than this application",
            ));
        }
        if version == 0 {
            let tx = conn.transaction()?;
            tx.execute_batch(include_str!("../migrations/001_initial.sql"))?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        Ok(Self {
            conn,
            dir: dir.to_owned(),
        })
    }
    pub fn ingest(&mut self, mut capture: Capture, settings: &Settings) -> Result<(i64, bool)> {
        if capture.kind == ClipKind::Files {
            capture.text = capture.paths.join("\n");
        }
        let mut hash = Sha256::new();
        if let Some(pixels) = &capture.pixels {
            hash.update(pixels.width().to_le_bytes());
            hash.update(pixels.height().to_le_bytes());
            hash.update(pixels.as_raw());
        } else {
            hash.update(capture.text.as_bytes());
        }
        let hash = hash.finalize().to_vec();
        let kind = capture.kind.as_str();
        let now = now_ms();
        let source = capture.source.as_ref().map(json).transpose()?;
        let sub = if capture.kind == ClipKind::Text {
            detect::sub_kind(&capture.text)
        } else {
            None
        };
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM items WHERE kind=? AND hash=?",
                params![kind, hash],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            self.conn.execute("UPDATE items SET html=?,rtf=?,source_json=?,last_used_at=?,use_count=use_count+1 WHERE id=?",params![capture.html,capture.rtf,source,now,id])?;
            self.cleanup(settings)?;
            return Ok((id, false));
        }
        let tx = self.conn.transaction()?;
        let paths = if capture.kind == ClipKind::Files {
            Some(json(&capture.paths)?)
        } else {
            None
        };
        tx.execute("INSERT INTO items(kind,hash,sub_kind,text,text_length,line_count,html,rtf,paths_json,source_json,created_at,last_used_at,search_text) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",params![kind,hash,sub,capture.text,capture.text.chars().count() as u32,if capture.text.is_empty(){0}else{capture.text.split('\n').count() as u32},capture.html,capture.rtf,paths,source,now,now,capture.text.to_lowercase()])?;
        let id = tx.last_insert_rowid();
        if let Some(pixels) = capture.pixels {
            let path = self.dir.join("images").join(format!("{id}.png"));
            let thumb = self.dir.join("images").join(format!("{id}.thumb.png"));
            let save = (|| -> Result<ImageInfo> {
                pixels
                    .save(&path)
                    .map_err(|_| CmdError::new("io", "Image encoding failed"))?;
                image::DynamicImage::ImageRgba8(pixels.clone())
                    .thumbnail(480, 480)
                    .save(&thumb)
                    .map_err(|_| CmdError::new("io", "Thumbnail encoding failed"))?;
                Ok(ImageInfo {
                    path: path.to_string_lossy().into_owned(),
                    thumb: thumb.to_string_lossy().into_owned(),
                    width: pixels.width(),
                    height: pixels.height(),
                    bytes: fs::metadata(&path)?.len(),
                })
            })();
            let info = match save {
                Ok(info) => info,
                Err(e) => {
                    let _ = fs::remove_file(&path);
                    let _ = fs::remove_file(&thumb);
                    return Err(e);
                }
            };
            if let Err(e) = tx
                .execute(
                    "UPDATE items SET image_json=? WHERE id=?",
                    params![json(&info)?, id],
                )
                .and_then(|_| tx.commit())
            {
                let _ = fs::remove_file(path);
                let _ = fs::remove_file(thumb);
                return Err(e.into());
            }
        } else {
            tx.commit()?;
        }
        self.cleanup(settings)?;
        Ok((id, true))
    }
    pub fn list(&self, query: ListQuery) -> Result<ListResult> {
        if query.offset < 0 || !(1..=200).contains(&query.limit) {
            return Err(CmdError::invalid(
                "offset must be nonnegative and limit must be 1..200",
            ));
        }
        let (clause, mut args) = search::clause(&query);
        let total = self.conn.query_row(
            &format!("SELECT count(*) FROM items WHERE {clause}"),
            params_from_iter(&args),
            |r| r.get(0),
        )?;
        args.push(query.limit.into());
        args.push(query.offset.into());
        let mut stmt=self.conn.prepare(&format!("SELECT {COLUMNS} FROM items WHERE {clause} ORDER BY pinned DESC,last_used_at DESC,id DESC LIMIT ? OFFSET ?"))?;
        let items = stmt
            .query_map(params_from_iter(&args), item_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ListResult { items, total })
    }
    pub fn get(&self, id: i64) -> Result<ClipItem> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM items WHERE id=?"),
                [id],
                item_row,
            )
            .optional()?
            .ok_or_else(|| CmdError::new("not_found", "Item does not exist"))
    }
    pub fn content(&self, id: i64) -> Result<StoredContent> {
        let item = self.get(id)?;
        let (text, html, rtf) =
            self.conn
                .query_row("SELECT text,html,rtf FROM items WHERE id=?", [id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?;
        Ok(StoredContent {
            item,
            text,
            html,
            rtf,
        })
    }
    pub fn item_content(&self, id: i64, max_chars: i64) -> Result<ItemContent> {
        if !(1..=5_000_000).contains(&max_chars) {
            return Err(CmdError::invalid("maxChars must be 1..5000000"));
        }
        let content = self.content(id)?;
        let text: String = content.text.chars().take(max_chars as usize).collect();
        let truncated = text.len() < content.text.len();
        Ok(ItemContent {
            text,
            truncated,
            html: content.html,
        })
    }
    pub fn mark(&self, id: i64, field: &str, value: bool) -> Result<ClipItem> {
        let sql = match field {
            "favorite" => "UPDATE items SET favorite=? WHERE id=?",
            "pinned" => "UPDATE items SET pinned=? WHERE id=?",
            _ => return Err(CmdError::internal()),
        };
        if self.conn.execute(sql, params![value, id])? == 0 {
            return Err(CmdError::new("not_found", "Item does not exist"));
        }
        self.get(id)
    }
    pub fn note(&self, id: i64, note: Option<String>) -> Result<ClipItem> {
        let note = note.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        if note.as_ref().is_some_and(|s| s.chars().count() > 500) {
            return Err(CmdError::invalid("Note exceeds 500 characters"));
        }
        // Rust lowercasing also covers non-ASCII text; SQLite's built-in lower() only covers ASCII.
        let text: String = self
            .conn
            .query_row("SELECT text FROM items WHERE id=?", [id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| CmdError::new("not_found", "Item does not exist"))?;
        self.conn.execute(
            "UPDATE items SET note=?,search_text=? WHERE id=?",
            params![
                note,
                format!(
                    "{}\n{}",
                    text.to_lowercase(),
                    note.as_deref().unwrap_or("").to_lowercase()
                ),
                id
            ],
        )?;
        self.get(id)
    }
    pub fn bump(&self, id: i64, move_to_top: bool) -> Result<()> {
        self.get(id)?;
        self.conn.execute(
            "UPDATE items SET use_count=use_count+1,last_used_at=CASE WHEN ? THEN ? ELSE last_used_at END WHERE id=?",
            params![move_to_top,now_ms(),id],
        )?;
        Ok(())
    }
    pub fn delete(&mut self, ids: &[i64]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut removed = Vec::new();
        let mut count = 0;
        for id in ids {
            let image: Option<String> = tx
                .query_row("SELECT image_json FROM items WHERE id=?", [id], |r| {
                    r.get(0)
                })
                .optional()?
                .flatten();
            if let Some(image) = image {
                if let Ok(info) = serde_json::from_str::<ImageInfo>(&image) {
                    removed.push(info);
                }
            }
            count += tx.execute("DELETE FROM items WHERE id=?", [id])?;
        }
        tx.commit()?;
        for image in removed {
            for path in [image.path, image.thumb] {
                // Only delete managed images, even if a database was edited externally.
                if Path::new(&path).parent() == Some(self.dir.join("images").as_path()) {
                    if let Err(e) = fs::remove_file(path) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            log::warn!("Image removal failed: io");
                        }
                    }
                }
            }
        }
        Ok(count)
    }
    fn ids(&self, sql: &str, args: impl rusqlite::Params) -> Result<Vec<i64>> {
        Ok(self
            .conn
            .prepare(sql)?
            .query_map(args, |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn clear(&mut self, keep_marked: bool) -> Result<usize> {
        let ids = self.ids(
            if keep_marked {
                "SELECT id FROM items WHERE favorite=0 AND pinned=0"
            } else {
                "SELECT id FROM items"
            },
            [],
        )?;
        self.delete(&ids)
    }
    pub fn cleanup(&mut self, s: &Settings) -> Result<usize> {
        let mut ids = Vec::new();
        if s.retention_days > 0 {
            ids.extend(self.ids(
                "SELECT id FROM items WHERE favorite=0 AND pinned=0 AND last_used_at<?",
                [now_ms() - i64::from(s.retention_days) * 86_400_000],
            )?);
        }
        if s.history_limit > 0 {
            ids.extend(self.ids("SELECT id FROM items WHERE favorite=0 AND pinned=0 ORDER BY last_used_at DESC,id DESC LIMIT -1 OFFSET ?",[s.history_limit])?);
        }
        self.delete(&ids)
    }
    pub fn stats(&self) -> Result<Stats> {
        let size = |p: PathBuf| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let (total,text,image,files,favorite,pinned,marked)=self.conn.query_row("SELECT count(*),coalesce(sum(kind='text'),0),coalesce(sum(kind='image'),0),coalesce(sum(kind='files'),0),coalesce(sum(favorite),0),coalesce(sum(pinned),0),coalesce(sum(favorite OR pinned),0) FROM items",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
        let image_bytes = fs::read_dir(self.dir.join("images"))?
            .filter_map(|e| e.ok())
            .map(|e| size(e.path()))
            .sum();
        Ok(Stats {
            total,
            text,
            image,
            files,
            favorite,
            pinned,
            marked,
            db_bytes: ["copybook.db", "copybook.db-wal", "copybook.db-shm"]
                .iter()
                .map(|n| size(self.dir.join(n)))
                .sum(),
            image_bytes,
        })
    }
    pub fn seen_apps(&self) -> Result<Vec<SeenApp>> {
        let mut stmt=self.conn.prepare("SELECT source_json,count(*) AS n FROM items WHERE source_json IS NOT NULL GROUP BY json_extract(source_json,'$.exe') COLLATE NOCASE ORDER BY n DESC,json_extract(source_json,'$.exe')")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (source, count) = row?;
            out.push(SeenApp {
                source: serde_json::from_str(&source)
                    .map_err(|_| CmdError::new("db", "Invalid source record"))?,
                count,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Storage, Settings) {
        let d = tempfile::tempdir_in(crate::test_dir()).unwrap();
        let db = Storage::open(d.path()).unwrap();
        let s = Settings {
            history_limit: 0,
            ..Settings::default()
        };
        (d, db, s)
    }
    fn query(text: &str, filter: ListFilter, offset: i64, limit: i64) -> ListQuery {
        ListQuery {
            query: text.into(),
            filter,
            offset,
            limit,
        }
    }
    #[test]
    fn dedupe_marks_content_and_deletion() {
        let (_dir, mut db, s) = setup();
        let mut a = Capture::text("https://example.org");
        a.html = Some("<b>one</b>".into());
        let (id, new) = db.ingest(a.clone(), &s).unwrap();
        assert!(new);
        db.mark(id, "favorite", true).unwrap();
        db.mark(id, "pinned", true).unwrap();
        db.note(id, Some(" note ".into())).unwrap();
        a.html = Some("<b>two</b>".into());
        a.rtf = Some(b"rtf".to_vec());
        assert_eq!(db.ingest(a, &s).unwrap(), (id, false));
        let item = db.get(id).unwrap();
        assert_eq!(item.use_count, 2);
        assert!(item.favorite && item.pinned && item.has_rtf);
        assert_eq!(item.note.as_deref(), Some("note"));
        assert!(item.last_used_at >= item.created_at);
        assert_eq!(db.content(id).unwrap().html.as_deref(), Some("<b>two</b>"));
        assert!(db.note(id, Some("中".repeat(501))).is_err());
        assert!(db.note(id, Some("中".repeat(500))).is_ok());
        assert_eq!(db.note(id, Some(" ".into())).unwrap().note, None);
        assert_eq!(db.list(query("", ListFilter::Url, 0, 50)).unwrap().total, 1);
        assert_eq!(
            db.list(query("", ListFilter::Favorite, 0, 50))
                .unwrap()
                .total,
            1
        );
        assert_eq!(db.clear(true).unwrap(), 0);
        assert_eq!(db.clear(false).unwrap(), 1);
        assert!(db.get(id).is_err());
        let mut capture = Capture::text("");
        capture.kind = ClipKind::Image;
        capture.pixels = Some(image::RgbaImage::new(600, 300));
        let id = db.ingest(capture, &s).unwrap().0;
        let image = db.get(id).unwrap().image.unwrap();
        assert!(Path::new(&image.path).exists());
        assert!(image::open(&image.thumb).unwrap().width() <= 480);
        db.delete(&[id]).unwrap();
        assert!(!Path::new(&image.path).exists());
        assert!(!Path::new(&image.thumb).exists());
    }
    #[test]
    fn search_paging_order_files_stats_sources_and_limits() {
        let (dir, mut db, mut s) = setup();
        for i in 0..65 {
            let mut c = Capture::text(format!("HELLO 中文 {i} % _ \\"));
            c.source = Some(SourceApp {
                name: "Test".into(),
                exe: "Test.exe".into(),
                icon: None,
            });
            db.ingest(c, &s).unwrap();
        }
        db.conn
            .execute("UPDATE items SET last_used_at=1234", [])
            .unwrap();
        db.mark(1, "pinned", true).unwrap();
        db.mark(2, "favorite", true).unwrap();
        let page = db
            .list(query("中文 HELLO", ListFilter::Text, 0, 10))
            .unwrap();
        assert_eq!(page.total, 65);
        assert_eq!(page.items[0].id, 1);
        assert_eq!(page.items[1].id, 65);
        let next = db
            .list(query("中文 HELLO", ListFilter::Text, 10, 10))
            .unwrap();
        assert_eq!(next.items[0].id, 56);
        assert!(!next
            .items
            .iter()
            .any(|i| page.items.iter().any(|j| j.id == i.id)));
        for term in ["中", "中文", "%", "_", r"\", "hello", "HELLO", "hello 中"] {
            assert_eq!(
                db.list(query(term, ListFilter::All, 0, 50)).unwrap().total,
                65,
                "{term}"
            );
        }
        db.note(2, Some("UniqueNote ÉCOLE".into())).unwrap();
        assert_eq!(
            db.list(query("uniquenote école", ListFilter::All, 0, 10))
                .unwrap()
                .total,
            1
        );
        let file = dir.path().join("a.txt");
        fs::write(&file, b"test").unwrap();
        let mut c = Capture::text("");
        c.kind = ClipKind::Files;
        c.paths = vec![
            file.to_string_lossy().into_owned(),
            dir.path().join("missing").to_string_lossy().into_owned(),
        ];
        let id = db.ingest(c, &s).unwrap().0;
        let entries = db.get(id).unwrap().files.unwrap();
        assert!(entries[0].exists);
        assert!(!entries[1].exists);
        assert_eq!(entries[0].bytes, Some(4));
        assert_eq!(
            db.list(query("a.txt", ListFilter::Files, 0, 10))
                .unwrap()
                .total,
            1
        );
        assert_eq!(db.seen_apps().unwrap()[0].count, 65);
        let stats = db.stats().unwrap();
        assert_eq!((stats.pinned, stats.marked), (1, 2));
        assert_eq!(
            (
                stats.total,
                stats.text,
                stats.files,
                stats.image,
                stats.favorite
            ),
            (66, 65, 1, 0, 1)
        );
        assert!(stats.db_bytes > 0);
        s.history_limit = 50;
        assert_eq!(db.cleanup(&s).unwrap(), 14);
        assert_eq!(db.stats().unwrap().total, 52);
        s.retention_days = 1;
        db.conn
            .execute("UPDATE items SET last_used_at=0", [])
            .unwrap();
        assert_eq!(db.cleanup(&s).unwrap(), 50);
        assert_eq!(db.stats().unwrap().total, 2);
        assert_eq!(db.clear(true).unwrap(), 0);
        assert_eq!(db.clear(false).unwrap(), 2);
    }
    #[test]
    fn long_text_is_previewed_and_ten_thousand_rows_are_fast() {
        let (_dir, mut db, s) = setup();
        for (offset, limit) in [(-1, 50), (0, 0), (0, 201)] {
            assert_eq!(
                db.list(query("", ListFilter::All, offset, limit))
                    .unwrap_err()
                    .code,
                "invalid"
            );
        }
        let id = db.ingest(Capture::text("中".repeat(3000)), &s).unwrap().0;
        for limit in [-1, 0, 5_000_001] {
            assert_eq!(db.item_content(id, limit).unwrap_err().code, "invalid");
        }
        assert!(!db.item_content(id, 5_000_000).unwrap().truncated);
        assert_eq!(db.get(id).unwrap().text.chars().count(), 2000);
        assert_eq!(
            db.item_content(id, 2500).unwrap().text.chars().count(),
            2500
        );
        assert!(db.item_content(id, 2500).unwrap().truncated);
        let tx = db.conn.transaction().unwrap();
        for i in 0..10000 {
            tx.execute("INSERT INTO items(kind,hash,text,text_length,line_count,created_at,last_used_at,search_text) VALUES('text',?, ?,1,1,0,?,'benchmark')",params![format!("hash{i}"),format!("row {i}"),i]).unwrap();
        }
        tx.commit().unwrap();
        let start = std::time::Instant::now();
        let page = db.list(query("", ListFilter::All, 0, 50)).unwrap();
        let elapsed = start.elapsed();
        println!("list_items 10001 rows / 50 items: {elapsed:?}");
        assert_eq!(page.total, 10001);
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "{elapsed:?}"
        );
    }
}
