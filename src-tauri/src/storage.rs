use crate::{detect, model::*, search, settings::Settings};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

const COLUMNS: &str="id,kind,sub_kind,substr(text,1,2000),text_length,line_count,html IS NOT NULL,rtf IS NOT NULL,image_json,paths_json,source_json,favorite,pinned,note,use_count,created_at,last_used_at,group_name";

pub struct Storage {
    conn: Connection,
    pub dir: PathBuf,
    undo: Option<(Vec<i64>, i64)>,
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
        group: row.get(17)?,
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
        if version > 2 {
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
        if version < 2 {
            let tx = conn.transaction()?;
            tx.execute_batch(include_str!("../migrations/002_organize.sql"))?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        let mut storage = Self {
            conn,
            dir: dir.to_owned(),
            undo: None,
        };
        let abandoned = storage.ids("SELECT id FROM items WHERE deleted_at IS NOT NULL", [])?;
        storage.delete(&abandoned)?;
        Ok(storage)
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
            self.conn.execute("UPDATE items SET html=?,rtf=?,source_json=?,last_used_at=?,use_count=use_count+1,deleted_at=NULL WHERE id=?",params![capture.html,capture.rtf,source,now,id])?;
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
                &format!("SELECT {COLUMNS} FROM items WHERE id=? AND deleted_at IS NULL"),
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
            "favorite" => "UPDATE items SET favorite=?,group_name=CASE WHEN ?1 THEN group_name ELSE NULL END WHERE id=?2 AND deleted_at IS NULL",
            "pinned" => "UPDATE items SET pinned=? WHERE id=? AND deleted_at IS NULL",
            _ => return Err(CmdError::internal()),
        };
        if self.conn.execute(sql, params![value, id])? == 0 {
            return Err(CmdError::new("not_found", "Item does not exist"));
        }
        self.get(id)
    }
    pub fn note(&self, id: i64, note: Option<String>) -> Result<ClipItem> {
        self.get(id)?;
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
    pub fn matching_ids(&self, query: &ListQuery) -> Result<Vec<i64>> {
        let (clause, args) = search::clause(query);
        let ids = self.ids(&format!("SELECT id FROM items WHERE {clause} ORDER BY pinned DESC,last_used_at DESC,id DESC LIMIT 100001"), params_from_iter(args))?;
        if ids.len() > 100000 {
            return Err(CmdError::new("too_large", "Too many matching records"));
        }
        Ok(ids)
    }
    pub fn soft_delete(&mut self, ids: &[i64]) -> Result<usize> {
        validate_ids(ids, 100000)?;
        if self.undo.is_some() {
            let previous = self.ids("SELECT id FROM items WHERE deleted_at IS NOT NULL", [])?;
            self.delete(&previous)?;
            self.undo = None;
        }
        let deadline = now_ms() + 15000;
        let tx = self.conn.transaction()?;
        let mut removed = Vec::new();
        for id in ids {
            if tx.execute(
                "UPDATE items SET deleted_at=? WHERE id=? AND deleted_at IS NULL",
                params![deadline, id],
            )? > 0
            {
                removed.push(*id);
            }
        }
        tx.commit()?;
        let count = removed.len();
        if count > 0 {
            self.undo = Some((removed, deadline));
        }
        Ok(count)
    }
    pub fn undo_delete(&mut self) -> Result<usize> {
        let Some((ids, deadline)) = self.undo.take() else {
            return Ok(0);
        };
        if now_ms() >= deadline {
            self.purge_expired()?;
            return Ok(0);
        }
        let tx = self.conn.transaction()?;
        let mut count = 0;
        for id in ids {
            count += tx.execute(
                "UPDATE items SET deleted_at=NULL WHERE id=? AND deleted_at=?",
                params![id, deadline],
            )?;
        }
        tx.commit()?;
        Ok(count)
    }
    pub fn purge_expired(&mut self) -> Result<()> {
        let ids = self.ids(
            "SELECT id FROM items WHERE deleted_at IS NOT NULL AND deleted_at<=?",
            [now_ms()],
        )?;
        self.delete(&ids)?;
        Ok(())
    }
    pub fn merged_text(&self, ids: &[i64], separator: &str) -> Result<String> {
        validate_ids(ids, 1000)?;
        if separator.chars().count() > 100 {
            return Err(CmdError::invalid("Separator exceeds 100 characters"));
        }
        let mut result = String::new();
        for (index, id) in ids.iter().enumerate() {
            let content = self.content(*id)?;
            if content.item.kind != ClipKind::Text {
                return Err(CmdError::invalid("Only text records can be merged"));
            }
            if result.len() + content.text.len() + separator.len() > 5_000_000 {
                return Err(CmdError::new("too_large", "Merged text exceeds 5 MB"));
            }
            if index > 0 {
                result.push_str(separator);
            }
            result.push_str(&content.text);
        }
        Ok(result)
    }
    pub fn groups(&self) -> Result<Vec<String>> {
        Ok(self.conn.prepare("SELECT DISTINCT group_name FROM items WHERE group_name IS NOT NULL AND favorite=1 AND deleted_at IS NULL ORDER BY group_name COLLATE NOCASE")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }
    pub fn set_group(&mut self, ids: &[i64], group: Option<String>) -> Result<usize> {
        validate_ids(ids, 100000)?;
        let group = group_name(group)?;
        let tx = self.conn.transaction()?;
        let mut count = 0;
        for id in ids {
            let found: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE id=? AND deleted_at IS NULL)",
                [id],
                |r| r.get(0),
            )?;
            if !found {
                return Err(CmdError::new("not_found", "Item does not exist"));
            }
            count += tx.execute("UPDATE items SET group_name=?,favorite=CASE WHEN ?1 IS NULL THEN favorite ELSE 1 END WHERE id=?2", params![group,id])?;
        }
        tx.commit()?;
        Ok(count)
    }
    pub fn rename_group(&self, name: String, replacement: Option<String>) -> Result<usize> {
        let name =
            group_name(Some(name))?.ok_or_else(|| CmdError::invalid("Group name required"))?;
        let replacement = group_name(replacement)?;
        Ok(self.conn.execute(
            "UPDATE items SET group_name=? WHERE group_name=?",
            params![replacement, name],
        )?)
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
        let pending = self.ids("SELECT id FROM items WHERE deleted_at IS NOT NULL", [])?;
        self.delete(&pending)?;
        self.undo = None;
        let ids = self.ids(
            if keep_marked {
                "SELECT id FROM items WHERE favorite=0 AND pinned=0 AND deleted_at IS NULL"
            } else {
                "SELECT id FROM items WHERE deleted_at IS NULL"
            },
            [],
        )?;
        self.delete(&ids)
    }
    pub fn cleanup(&mut self, s: &Settings) -> Result<usize> {
        self.purge_expired()?;
        let mut ids = Vec::new();
        if s.retention_days > 0 {
            ids.extend(self.ids(
                "SELECT id FROM items WHERE favorite=0 AND pinned=0 AND deleted_at IS NULL AND last_used_at<?",
                [now_ms() - i64::from(s.retention_days) * 86_400_000],
            )?);
        }
        if s.history_limit > 0 {
            ids.extend(self.ids("SELECT id FROM items WHERE favorite=0 AND pinned=0 AND deleted_at IS NULL ORDER BY last_used_at DESC,id DESC LIMIT -1 OFFSET ?",[s.history_limit])?);
        }
        self.delete(&ids)
    }
    pub fn stats(&self) -> Result<Stats> {
        let size = |p: PathBuf| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let (total,text,image,files,favorite,pinned,marked)=self.conn.query_row("SELECT count(*),coalesce(sum(kind='text'),0),coalesce(sum(kind='image'),0),coalesce(sum(kind='files'),0),coalesce(sum(favorite),0),coalesce(sum(pinned),0),coalesce(sum(favorite OR pinned),0) FROM items WHERE deleted_at IS NULL",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
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
        let mut stmt=self.conn.prepare("SELECT source_json,count(*) AS n FROM items WHERE source_json IS NOT NULL AND deleted_at IS NULL GROUP BY json_extract(source_json,'$.exe') COLLATE NOCASE ORDER BY n DESC,json_extract(source_json,'$.exe')")?;
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

    pub fn restore_records(
        &mut self,
        records: Vec<crate::backup::PreparedRecord>,
    ) -> Result<crate::backup::RestoreResult> {
        let tx = self.conn.transaction()?;
        let mut written = Vec::new();
        let result = (|| -> Result<crate::backup::RestoreResult> {
            let mut imported = 0;
            let mut skipped = 0;
            for prepared in records {
                let r = prepared.record;
                let text = if r.kind == ClipKind::Files {
                    r.paths.join("\n")
                } else {
                    r.text
                };
                let mut hash = Sha256::new();
                if let Some(pixels) = &prepared.pixels {
                    hash.update(pixels.width().to_le_bytes());
                    hash.update(pixels.height().to_le_bytes());
                    hash.update(pixels.as_raw());
                } else {
                    hash.update(text.as_bytes());
                }
                let hash = hash.finalize().to_vec();
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM items WHERE kind=? AND hash=?)",
                    params![r.kind.as_str(), hash],
                    |row| row.get(0),
                )?;
                if exists {
                    skipped += 1;
                    continue;
                }
                let sub = if r.kind == ClipKind::Text {
                    detect::sub_kind(&text)
                } else {
                    None
                };
                let source = r.source.as_ref().map(json).transpose()?;
                let paths = if r.kind == ClipKind::Files {
                    Some(json(&r.paths)?)
                } else {
                    None
                };
                let search = format!(
                    "{}\n{}",
                    text.to_lowercase(),
                    r.note.as_deref().unwrap_or("").to_lowercase()
                );
                tx.execute("INSERT INTO items(kind,hash,sub_kind,text,text_length,line_count,html,rtf,paths_json,source_json,favorite,pinned,note,group_name,use_count,created_at,last_used_at,search_text) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                    params![r.kind.as_str(),hash,sub,text,text.chars().count() as u32,if text.is_empty(){0}else{text.split('\n').count() as u32},r.html,prepared.rtf,paths,source,r.favorite||r.group.is_some(),r.pinned,r.note,r.group,r.use_count,r.created_at,r.last_used_at,search])?;
                if let Some(pixels) = prepared.pixels {
                    let id = tx.last_insert_rowid();
                    let path = self.dir.join("images").join(format!("{id}.png"));
                    let thumb = self.dir.join("images").join(format!("{id}.thumb.png"));
                    written.extend([path.clone(), thumb.clone()]);
                    pixels
                        .save(&path)
                        .map_err(|_| CmdError::new("io", "Image restore failed"))?;
                    image::DynamicImage::ImageRgba8(pixels.clone())
                        .thumbnail(480, 480)
                        .save(&thumb)
                        .map_err(|_| CmdError::new("io", "Thumbnail restore failed"))?;
                    let info = ImageInfo {
                        path: path.to_string_lossy().into_owned(),
                        thumb: thumb.to_string_lossy().into_owned(),
                        width: pixels.width(),
                        height: pixels.height(),
                        bytes: fs::metadata(&path)?.len(),
                    };
                    tx.execute(
                        "UPDATE items SET image_json=? WHERE id=?",
                        params![json(&info)?, id],
                    )?;
                }
                imported += 1;
            }
            Ok(crate::backup::RestoreResult {
                imported,
                skipped,
                settings_restored: None,
            })
        })();
        match result {
            Ok(result) => match tx.commit() {
                Ok(()) => Ok(result),
                Err(e) => {
                    for path in written {
                        let _ = fs::remove_file(path);
                    }
                    Err(e.into())
                }
            },
            Err(e) => {
                drop(tx);
                for path in written {
                    let _ = fs::remove_file(path);
                }
                Err(e)
            }
        }
    }
}

pub fn validate_ids(ids: &[i64], max: usize) -> Result<()> {
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    if ids.is_empty()
        || ids.len() > max
        || ids.iter().any(|id| *id <= 0)
        || unique.len() != ids.len()
    {
        return Err(CmdError::invalid(
            "Expected distinct positive item IDs within the operation limit",
        ));
    }
    Ok(())
}
pub fn group_name(group: Option<String>) -> Result<Option<String>> {
    let group = group.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    if group
        .as_ref()
        .is_some_and(|s| s.chars().count() > 40 || s.chars().any(char::is_control))
    {
        return Err(CmdError::invalid(
            "Group must have at most 40 characters without control characters",
        ));
    }
    Ok(group)
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
            ..ListQuery::default()
        }
    }
    #[test]
    fn batch_undo_filters_groups_and_full_text_merge() {
        let (_dir, mut db, s) = setup();
        let full = format!("{}尾部", "完整文字".repeat(800));
        let mut capture = Capture::text(&full);
        capture.source = Some(SourceApp {
            name: "Editor".into(),
            exe: "Code.exe".into(),
            icon: None,
        });
        let (a, _) = db.ingest(capture, &s).unwrap();
        let (b, _) = db.ingest(Capture::text("第二条"), &s).unwrap();
        db.set_group(&[a, b], Some("工作".into())).unwrap();
        db.note(a, Some("备注".into())).unwrap();
        assert_eq!(
            db.merged_text(&[b, a], "\n").unwrap(),
            format!("第二条\n{full}")
        );
        assert!(db.merged_text(&[a, a], " ").is_err());
        let mut q = query("", ListFilter::All, 0, 1);
        q.group = Some("工作".into());
        q.source_exe = Some("code.EXE".into());
        q.since = Some(0);
        q.until = Some(now_ms() + 1000);
        assert_eq!(db.matching_ids(&q).unwrap(), vec![a]);
        q.until = Some(0);
        assert_eq!(db.list(q).unwrap().total, 0);
        assert_eq!(db.soft_delete(&[a, b]).unwrap(), 2);
        assert_eq!(db.stats().unwrap().total, 0);
        assert!(db.get(a).is_err());
        assert!(db.content(a).is_err());
        assert!(db.groups().unwrap().is_empty());
        assert!(db.seen_apps().unwrap().is_empty());
        assert_eq!(db.undo_delete().unwrap(), 2);
        assert_eq!(db.get(a).unwrap().group.as_deref(), Some("工作"));
        assert_eq!(db.get(a).unwrap().note.as_deref(), Some("备注"));
        db.rename_group("工作".into(), Some("项目".into())).unwrap();
        assert_eq!(db.groups().unwrap(), vec!["项目"]);
        db.mark(a, "favorite", false).unwrap();
        assert!(db.get(a).unwrap().group.is_none());
    }
    #[test]
    fn image_undo_expiry_recapture_and_restart() {
        let (dir, mut db, s) = setup();
        let mut capture = Capture::text("");
        capture.kind = ClipKind::Image;
        capture.pixels = Some(image::RgbaImage::new(2, 2));
        let (id, _) = db.ingest(capture.clone(), &s).unwrap();
        let path = db.get(id).unwrap().image.unwrap().path;
        db.soft_delete(&[id]).unwrap();
        assert!(Path::new(&path).exists());
        assert!(db.merged_text(&[id], " ").is_err());
        assert_eq!(db.undo_delete().unwrap(), 1);
        assert!(Path::new(&path).exists());
        db.soft_delete(&[id]).unwrap();
        db.ingest(capture, &s).unwrap();
        assert_eq!(db.undo_delete().unwrap(), 0);
        assert!(db.get(id).is_ok());
        db.soft_delete(&[id]).unwrap();
        db.conn
            .execute("UPDATE items SET deleted_at=0 WHERE id=?", [id])
            .unwrap();
        db.undo.as_mut().unwrap().1 = 0;
        assert_eq!(db.undo_delete().unwrap(), 0);
        assert!(!Path::new(&path).exists());
        let (id, _) = db.ingest(Capture::text("pending"), &s).unwrap();
        db.soft_delete(&[id]).unwrap();
        drop(db);
        let db = Storage::open(dir.path()).unwrap();
        assert_eq!(db.stats().unwrap().total, 0);
    }
    #[test]
    fn migrate_v1_and_portable_backup_merge() {
        let old = tempfile::tempdir_in(crate::test_dir()).unwrap();
        let conn = Connection::open(old.path().join("copybook.db")).unwrap();
        conn.execute_batch(include_str!("../migrations/001_initial.sql"))
            .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        drop(conn);
        let mut original = Storage::open(old.path()).unwrap();
        let s = Settings {
            history_limit: 0,
            ..Settings::default()
        };
        let (a, _) = original.ingest(Capture::text("portable text"), &s).unwrap();
        original.set_group(&[a], Some("Group".into())).unwrap();
        original.note(a, Some("note".into())).unwrap();
        let mut image = Capture::text("");
        image.kind = ClipKind::Image;
        image.pixels = Some(image::RgbaImage::new(3, 4));
        original.ingest(image, &s).unwrap();
        let archive = crate::backup::snapshot(&original, s.clone()).unwrap();
        let bytes = crate::backup::encrypt(&archive, "test password").unwrap();
        let archive = crate::backup::decrypt(&bytes, "test password").unwrap();
        let (new_dir, mut restored, _) = setup();
        let result = restored
            .restore_records(crate::backup::prepare(archive.records).unwrap())
            .unwrap();
        assert_eq!(result.imported, 2);
        let result = restored
            .restore_records(
                crate::backup::prepare(crate::backup::snapshot(&original, s).unwrap().records)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(result.skipped, 2);
        assert_eq!(result.imported, 0);
        let result = restored.list(query("", ListFilter::All, 0, 50)).unwrap();
        let image = result
            .items
            .iter()
            .find(|i| i.kind == ClipKind::Image)
            .unwrap()
            .image
            .as_ref()
            .unwrap();
        assert!(Path::new(&image.path).starts_with(new_dir.path()));
        assert_eq!(image::open(&image.path).unwrap().width(), 3);
        assert_eq!(restored.groups().unwrap(), vec!["Group"]);
        assert_eq!(
            result
                .items
                .iter()
                .find(|i| i.kind == ClipKind::Text)
                .unwrap()
                .note
                .as_deref(),
            Some("note")
        );
    }
    #[test]
    fn restore_rolls_back_all_new_rows_when_an_image_cannot_be_written() {
        let (_dir, mut source, s) = setup();
        let mut image = Capture::text("");
        image.kind = ClipKind::Image;
        image.pixels = Some(image::RgbaImage::new(2, 2));
        source.ingest(image, &s).unwrap();
        source
            .ingest(Capture::text("first record must roll back"), &s)
            .unwrap();
        let records =
            crate::backup::prepare(crate::backup::snapshot(&source, s).unwrap().records).unwrap();
        let (target_dir, mut target, _) = setup();
        // The text record sorts first; force the following image write to fail.
        fs::create_dir(target_dir.path().join("images/2.png")).unwrap();
        assert!(target.restore_records(records).is_err());
        assert_eq!(target.stats().unwrap().total, 0);
        assert!(!target_dir.path().join("images/2.thumb.png").exists());
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
