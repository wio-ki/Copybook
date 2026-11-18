//! Portable, authenticated, password-encrypted WebDAV backups. No plaintext is written to disk.
use crate::{
    credentials::{self, PasswordKind},
    model::*,
    settings::Settings,
    storage::Storage,
};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use quick_xml::{events::Event, Reader};
use rand_core::{OsRng, RngCore};
use reqwest::{
    blocking::{Client, Response},
    redirect::Policy,
    Method, StatusCode,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    sync::Mutex,
    time::Duration,
};
use url::Url;
use zeroize::Zeroizing;

pub static GATE: Mutex<()> = Mutex::new(());
pub const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"CPBK0001";
const HEADER: usize = 36;

#[derive(Deserialize)]
pub struct Connection {
    pub url: String,
    pub username: String,
    pub password: String,
}
impl Drop for Connection {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.password.zeroize();
    }
}
#[derive(Debug, Serialize)]
pub struct BackupEntry {
    pub name: String,
    pub bytes: Option<u64>,
    pub legacy: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBackup {
    #[serde(flatten)]
    pub entry: BackupEntry,
    pub password_saved: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub imported: usize,
    pub skipped: usize,
    #[serde(rename = "settingsRestored")]
    pub settings_restored: Option<bool>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Archive {
    pub schema: u32,
    pub settings: Settings,
    pub records: Vec<Record>,
    #[serde(default)]
    pub folders: Option<Vec<String>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub kind: ClipKind,
    pub text: String,
    pub html: Option<String>,
    pub rtf: Option<String>,
    pub image: Option<String>,
    pub paths: Vec<String>,
    pub source: Option<SourceApp>,
    pub favorite: bool,
    pub pinned: bool,
    pub note: Option<String>,
    pub group: Option<String>,
    pub use_count: i64,
    pub created_at: i64,
    pub last_used_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite_order: Option<i64>,
}
pub struct PreparedRecord {
    pub record: Record,
    pub rtf: Option<Vec<u8>>,
    pub pixels: Option<image::RgbaImage>,
}
fn too_large() -> CmdError {
    CmdError::new(
        "too_large",
        "Backup exceeds the supported size or record limit",
    )
}
fn invalid() -> CmdError {
    CmdError::invalid("Invalid or unsupported backup")
}
fn crypto_error() -> CmdError {
    CmdError::new("crypto", "Incorrect backup password or damaged backup")
}

fn key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    if !(8..=1024).contains(&password.len()) {
        return Err(CmdError::invalid(
            "Backup password must be 8..1024 UTF-8 bytes",
        ));
    }
    let params = Params::new(65536, 3, 1, Some(32)).map_err(|_| CmdError::internal())?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| CmdError::internal())?;
    Ok(key)
}
pub fn encrypt(archive: &Archive, password: &str) -> Result<Vec<u8>> {
    // Limit serialization while writing, before a second unbounded plaintext allocation.
    let mut json = Zeroizing::new(Vec::new());
    serde_json::to_writer((&mut *json).take_limit(MAX_BYTES), archive).map_err(|_| too_large())?;
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(&json)?;
    let compressed = Zeroizing::new(gzip.finish()?);
    let mut header = [0u8; HEADER];
    header[..8].copy_from_slice(MAGIC);
    OsRng
        .try_fill_bytes(&mut header[8..])
        .map_err(|_| CmdError::internal())?;
    let key = key(password, &header[8..24])?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| CmdError::internal())?;
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&header[24..]),
            Payload {
                msg: &compressed,
                aad: &header,
            },
        )
        .map_err(|_| crypto_error())?;
    let mut result = header.to_vec();
    result.extend(encrypted);
    if result.len() > MAX_BYTES {
        return Err(too_large());
    }
    Ok(result)
}
// A bounded writer for serde's streaming serializer.
struct LimitedWriter<W> {
    inner: W,
    remaining: usize,
}
trait LimitWrite: Write + Sized {
    fn take_limit(self, remaining: usize) -> LimitedWriter<Self> {
        LimitedWriter {
            inner: self,
            remaining,
        }
    }
}
impl<W: Write> LimitWrite for W {}
impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other("Size limit exceeded"));
        }
        let n = self.inner.write(bytes)?;
        self.remaining -= n;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
pub fn decrypt(bytes: &[u8], password: &str) -> Result<Archive> {
    if bytes.len() > MAX_BYTES {
        return Err(too_large());
    }
    if bytes.len() < HEADER + 16 || &bytes[..8] != MAGIC {
        return Err(invalid());
    }
    let key = key(password, &bytes[8..24])?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| CmdError::internal())?;
    let compressed = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&bytes[24..HEADER]),
                Payload {
                    msg: &bytes[HEADER..],
                    aad: &bytes[..HEADER],
                },
            )
            .map_err(|_| crypto_error())?,
    );
    let mut json = Zeroizing::new(Vec::new());
    GzDecoder::new(compressed.as_slice())
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut json)
        .map_err(|_| invalid())?;
    if json.len() > MAX_BYTES {
        return Err(too_large());
    }
    let mut archive: Archive = serde_json::from_slice(&json).map_err(|_| invalid())?;
    if archive.schema != 1 {
        return Err(invalid());
    }
    if archive.folders.is_none() {
        // Backups predating folders contain flat names, including literal slashes.
        let names: Vec<_> = archive
            .records
            .iter()
            .filter_map(|r| r.group.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mapping: std::collections::HashMap<_, _> = crate::storage::legacy_folder_names(&names)
            .into_iter()
            .collect();
        for record in &mut archive.records {
            if let Some(name) = &record.group {
                record.group = mapping.get(name).cloned();
            }
        }
        archive.folders = Some(mapping.into_values().collect());
    }
    Ok(archive)
}
pub fn prepare(records: Vec<Record>) -> Result<Vec<PreparedRecord>> {
    if records.len() > 100000 {
        return Err(too_large());
    }
    let mut total = 0usize;
    records
        .into_iter()
        .map(|mut record| {
            record.group = crate::storage::group_name(record.group)?;
            if let Some(id) = &record.snippet_id {
                if record.kind != ClipKind::Text
                    || id.len() != 32
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(invalid());
                }
                crate::storage::validate_snippet(&record.text)?;
            }
            if record.text.len() > 5_000_000
                || record
                    .note
                    .as_ref()
                    .is_some_and(|n| n.chars().count() > 500)
                || record.created_at < 0
                || record.last_used_at < 0
                || record.use_count < 0
                || record
                    .favorite_order
                    .is_some_and(|rank| rank < 0 || !record.favorite)
                || record.paths.len() > 10000
            {
                return Err(invalid());
            }
            if record.kind == ClipKind::Files
                && (record.paths.is_empty() || record.paths.iter().any(|p| p.contains('\0')))
            {
                return Err(invalid());
            }
            if let Some(source) = record.source.as_mut() {
                source.icon = None;
            }
            let rtf = record
                .rtf
                .as_ref()
                .map(|s| STANDARD.decode(s).map_err(|_| invalid()))
                .transpose()?;
            let pixels = match (record.kind, record.image.as_ref()) {
                (ClipKind::Image, Some(encoded)) => {
                    let png = STANDARD.decode(encoded).map_err(|_| invalid())?;
                    let mut reader = image::ImageReader::with_format(
                        std::io::Cursor::new(png),
                        image::ImageFormat::Png,
                    );
                    let mut limits = image::Limits::default();
                    limits.max_alloc = Some(MAX_BYTES as u64);
                    limits.max_image_width = Some(16384);
                    limits.max_image_height = Some(16384);
                    reader.limits(limits);
                    Some(reader.decode().map_err(|_| invalid())?.into_rgba8())
                }
                (ClipKind::Image, None) => return Err(invalid()),
                (_, Some(_)) => return Err(invalid()),
                _ => None,
            };
            total = total
                .saturating_add(record.text.len())
                .saturating_add(record.html.as_ref().map_or(0, |s| s.len()))
                .saturating_add(rtf.as_ref().map_or(0, Vec::len))
                .saturating_add(pixels.as_ref().map_or(0, |p| p.as_raw().len()));
            if total > MAX_BYTES {
                return Err(too_large());
            }
            Ok(PreparedRecord {
                record,
                rtf,
                pixels,
            })
        })
        .collect()
}
pub fn snapshot(db: &Storage, settings: Settings) -> Result<Archive> {
    let ids = db.matching_ids(&ListQuery::default())?;
    let mut records = Vec::new();
    let mut size = 0usize;
    for id in ids {
        let content = db.content(id)?;
        let item = content.item;
        let image = if let Some(info) = item.image {
            if std::path::Path::new(&info.path).parent() != Some(db.dir.join("images").as_path()) {
                return Err(invalid());
            }
            let data = read_limited(std::fs::File::open(info.path)?, MAX_BYTES)?;
            size = size.saturating_add(data.len());
            Some(STANDARD.encode(data))
        } else {
            None
        };
        size = size
            .saturating_add(content.text.len())
            .saturating_add(content.html.as_ref().map_or(0, |s| s.len()))
            .saturating_add(content.rtf.as_ref().map_or(0, Vec::len));
        if size > MAX_BYTES {
            return Err(too_large());
        }
        records.push(Record {
            kind: item.kind,
            text: content.text,
            html: content.html,
            rtf: content.rtf.map(|v| STANDARD.encode(v)),
            image,
            paths: item
                .files
                .unwrap_or_default()
                .into_iter()
                .map(|f| f.path)
                .collect(),
            source: item.source.map(|mut s| {
                s.icon = None;
                s
            }),
            favorite: item.favorite,
            pinned: item.pinned,
            note: item.note,
            group: item.group,
            use_count: item.use_count,
            created_at: item.created_at,
            last_used_at: item.last_used_at,
            snippet_id: content.snippet_id,
            favorite_order: content.favorite_order,
        });
    }
    Ok(Archive {
        schema: 1,
        settings,
        records,
        folders: Some(db.groups()?),
    })
}
fn read_limited(reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(too_large());
    }
    Ok(bytes)
}
fn network() -> CmdError {
    CmdError::new(
        "network",
        "WebDAV request failed; verify the server URL and connection",
    )
}
fn checked(response: Response) -> Result<Response> {
    match response.status() {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(CmdError::new(
            "auth",
            "WebDAV authentication or permission denied",
        )),
        status if status.is_success() => Ok(response),
        _ => Err(network()),
    }
}
pub(crate) fn base_endpoint(connection: &Connection) -> Result<Url> {
    let mut url =
        Url::parse(connection.url.trim()).map_err(|_| CmdError::invalid("Invalid WebDAV URL"))?;
    let loopback = url
        .host_str()
        .is_some_and(|h| h == "localhost" || h == "127.0.0.1" || h == "[::1]");
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err(CmdError::invalid(
            "Use an HTTPS collection URL without embedded credentials, query or fragment",
        ));
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}
fn endpoint(connection: &Connection) -> Result<Url> {
    let base = base_endpoint(connection)?;
    if base
        .path()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("copybook"))
    {
        return Ok(base);
    }
    base.join("copybook/").map_err(|_| invalid())
}
fn client() -> Result<Client> {
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .user_agent("Copybook/1.4")
        .build()
        .map_err(|_| network())
}
fn valid_name(name: &str) -> bool {
    name.starts_with("copybook-")
        && name.ends_with(".cpbk")
        && name.len() < 100
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
}
fn request(
    connection: &Connection,
    method: Method,
    name: Option<&str>,
    legacy: bool,
) -> Result<reqwest::blocking::RequestBuilder> {
    let mut url = if legacy {
        base_endpoint(connection)?
    } else {
        endpoint(connection)?
    };
    if let Some(name) = name {
        if !valid_name(name) {
            return Err(CmdError::invalid("Invalid backup filename"));
        }
        url = url.join(name).map_err(|_| invalid())?;
    }
    Ok(client()?
        .request(method, url)
        .basic_auth(&connection.username, Some(&connection.password)))
}
fn ensure_collection(connection: &Connection) -> Result<()> {
    let response = request(
        connection,
        Method::from_bytes(b"MKCOL").map_err(|_| CmdError::internal())?,
        None,
        false,
    )?
    .send()
    .map_err(|_| network())?;
    if matches!(
        response.status(),
        StatusCode::CREATED | StatusCode::METHOD_NOT_ALLOWED
    ) {
        return Ok(());
    }
    checked(response)?;
    Err(network())
}
pub fn test_connection(connection: &Connection) -> Result<()> {
    ensure_collection(connection)?;
    list_at(connection, false).map(|_| ())
}
pub fn list(connection: &Connection) -> Result<Vec<BackupEntry>> {
    ensure_collection(connection)?;
    let mut entries = list_at(connection, false)?;
    if endpoint(connection)? != base_endpoint(connection)? {
        // v1.4.0 wrote to the parent. Keep those backups readable without moving or
        // deleting cloud data; a parent that disallows listing must not block new backups.
        if let Ok(legacy) = list_at(connection, true) {
            entries.extend(legacy);
        }
    }
    entries.sort_by(|a, b| b.name.cmp(&a.name).then(a.legacy.cmp(&b.legacy)));
    entries.truncate(10000);
    Ok(entries)
}
fn list_at(connection: &Connection, legacy: bool) -> Result<Vec<BackupEntry>> {
    let response=checked(request(connection,Method::from_bytes(b"PROPFIND").map_err(|_|CmdError::internal())?,None,legacy)?
        .header("Depth","1").header("Content-Type","application/xml; charset=utf-8")
        .body("<?xml version=\"1.0\"?><d:propfind xmlns:d=\"DAV:\"><d:prop><d:getcontentlength/></d:prop></d:propfind>")
        .send().map_err(|_|network())?)?;
    let bytes = read_limited(response, 4 * 1024 * 1024)?;
    parse_listing(
        &bytes,
        &if legacy {
            base_endpoint(connection)?
        } else {
            endpoint(connection)?
        },
        legacy,
    )
}
fn parse_listing(bytes: &[u8], base: &Url, legacy: bool) -> Result<Vec<BackupEntry>> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut entries = Vec::new();
    let mut href = String::new();
    let mut length = None;
    let mut tag = Vec::new();
    let mut multistatus = false;
    loop {
        match reader.read_event().map_err(|_| network())? {
            Event::Start(e) => {
                let name = e.local_name();
                tag = name.as_ref().to_vec();
                if tag == b"multistatus" {
                    multistatus = true;
                }
                if tag == b"response" {
                    href.clear();
                    length = None;
                }
            }
            Event::Text(e) => {
                let decoded = e.xml_content().map_err(|_| network())?;
                let value = quick_xml::escape::unescape(&decoded).map_err(|_| network())?;
                if tag == b"href" {
                    href.push_str(&value);
                } else if tag == b"getcontentlength" {
                    length = value.parse().ok();
                }
            }
            Event::End(e) => {
                if e.local_name().as_ref() == b"response" {
                    if let Ok(url) = base.join(&href) {
                        if url.origin() == base.origin() && url.path().starts_with(base.path()) {
                            let name = &url.path()[base.path().len()..];
                            if valid_name(name) && entries.len() < 10000 {
                                entries.push(BackupEntry {
                                    name: name.to_owned(),
                                    bytes: length,
                                    legacy,
                                });
                            }
                        }
                    }
                }
                tag.clear();
            }
            Event::Eof => break,
            Event::DocType(_) => return Err(network()),
            _ => {}
        }
    }
    if !multistatus {
        return Err(network());
    }
    entries.sort_by(|a, b| b.name.cmp(&a.name));
    entries.dedup_by(|a, b| a.name == b.name);
    Ok(entries)
}
pub fn create(
    connection: &Connection,
    archive: &Archive,
    password: Zeroizing<String>,
) -> Result<CreatedBackup> {
    let supplied = !password.is_empty();
    let password = credentials::resolve_encryption(connection, password)?;
    let entry = upload(connection, archive, &password)?;
    let password_saved = supplied.then(|| {
        match credentials::save(connection, PasswordKind::Encryption, Some(password)) {
            Ok(()) => true,
            Err(error) => {
                log::warn!(
                    "Backup uploaded but its password could not be saved: {}",
                    error.code
                );
                false
            }
        }
    });
    Ok(CreatedBackup {
        entry,
        password_saved,
    })
}

fn upload(connection: &Connection, archive: &Archive, password: &str) -> Result<BackupEntry> {
    let bytes = encrypt(archive, password)?;
    let size = bytes.len() as u64;
    let mut random = [0u8; 8];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| CmdError::internal())?;
    let name = format!("copybook-{}-{}.cpbk", now_ms(), u64::from_le_bytes(random));
    ensure_collection(connection)?;
    checked(
        request(connection, Method::PUT, Some(&name), false)?
            .header("Content-Type", "application/octet-stream")
            .header("If-None-Match", "*")
            .body(bytes)
            .send()
            .map_err(|_| network())?,
    )?;
    Ok(BackupEntry {
        name,
        bytes: Some(size),
        legacy: false,
    })
}
pub fn download(
    connection: &Connection,
    name: &str,
    password: &str,
    legacy: bool,
) -> Result<Archive> {
    let response = checked(
        request(connection, Method::GET, Some(name), legacy)?
            .send()
            .map_err(|_| network())?,
    )?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(too_large());
    }
    decrypt(&read_limited(response, MAX_BYTES)?, password)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_roundtrip_wrong_password_and_tampering() {
        let archive = Archive {
            schema: 1,
            settings: Settings::default(),
            records: vec![],
            folders: Some(vec![]),
        };
        let a = encrypt(&archive, "test password 中文").unwrap();
        let b = encrypt(&archive, "test password 中文").unwrap();
        assert_ne!(a, b);
        assert_eq!(decrypt(&a, "test password 中文").unwrap().schema, 1);
        assert_eq!(decrypt(&a, "wrong password").err().unwrap().code, "crypto");
        for offset in [8, 24, 36, a.len() - 1] {
            let mut altered = a.clone();
            altered[offset] ^= 1;
            assert_eq!(
                decrypt(&altered, "test password 中文").err().unwrap().code,
                "crypto"
            );
        }
        assert!(decrypt(b"not a backup", "test password 中文").is_err());
        assert!(encrypt(&archive, "short").is_err());
    }
    #[test]
    fn old_backups_default_to_flat_folders() {
        let (_dir, mut db, settings) = {
            let dir = tempfile::tempdir_in(crate::test_dir()).unwrap();
            let db = Storage::open(dir.path()).unwrap();
            (dir, db, Settings::default())
        };
        let (id, _) = db.ingest(Capture::text("old backup"), &settings).unwrap();
        db.set_group(&[id], Some("旧组".into())).unwrap();
        let mut archive = snapshot(&db, settings).unwrap();
        archive.folders = None;
        archive.records[0].group = Some("旧/名称".into());
        let bytes = encrypt(&archive, "old backup password").unwrap();
        let archive = decrypt(&bytes, "old backup password").unwrap();
        assert_eq!(archive.records[0].group.as_deref(), Some("旧／名称"));
        assert_eq!(archive.folders.unwrap(), vec!["旧／名称"]);
    }
    #[test]
    fn restrict_urls_and_remote_names() {
        for url in ["https://cloud.test/dav/", "http://127.0.0.1:1234/dav/"] {
            assert!(endpoint(&Connection {
                url: url.into(),
                username: String::new(),
                password: String::new()
            })
            .is_ok());
        }
        for url in [
            "http://cloud.test/dav/",
            "https://user:secret@cloud.test/",
            "file:///C:/data",
            "https://cloud.test/?token=secret",
        ] {
            assert!(endpoint(&Connection {
                url: url.into(),
                username: String::new(),
                password: String::new()
            })
            .is_err());
        }
        assert!(!valid_name("../copybook-1.cpbk"));
        assert!(!valid_name("copybook-%2f.cpbk"));
        let xml=br#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/dav/copybook-1.cpbk</d:href><d:propstat><d:prop><d:getcontentlength>120</d:getcontentlength></d:prop></d:propstat></d:response><d:response><d:href>https://evil.test/dav/copybook-2.cpbk</d:href></d:response></d:multistatus>"#;
        let entries =
            parse_listing(xml, &Url::parse("https://cloud.test/dav/").unwrap(), false).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].bytes, Some(120));
    }

    #[test]
    fn backups_default_to_one_copybook_subdirectory() {
        for (url, expected) in [
            ("https://cloud.test/", "https://cloud.test/copybook/"),
            ("https://cloud.test/dav", "https://cloud.test/dav/copybook/"),
            (
                "https://cloud.test/dav/copybook/",
                "https://cloud.test/dav/copybook/",
            ),
            (
                "https://cloud.test/dav/Copybook",
                "https://cloud.test/dav/Copybook/",
            ),
            (
                "https://cloud.test/a%20b/",
                "https://cloud.test/a%20b/copybook/",
            ),
        ] {
            let connection = Connection {
                url: url.into(),
                username: String::new(),
                password: String::new(),
            };
            assert_eq!(endpoint(&connection).unwrap().as_str(), expected);
        }
    }

    #[test]
    fn webdav_upload_list_download_and_authentication() {
        use std::{io::BufRead, net::TcpListener, sync::Arc};
        struct ForgetPassword<'a>(&'a Connection);
        impl Drop for ForgetPassword<'_> {
            fn drop(&mut self) {
                let _ = credentials::save(self.0, PasswordKind::Encryption, None);
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let username = format!("copybook-test-{}", std::process::id());
        let authorization = format!(
            "Basic {}",
            STANDARD.encode(format!("{username}:super-secret"))
        );
        let stored = Arc::new(Mutex::new((String::new(), Vec::<u8>::new())));
        let cloud = stored.clone();
        let server = std::thread::spawn(move || {
            let mut created = false;
            for _ in 0..14 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut length = 0;
                let mut auth = false;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    let lower = line.to_lowercase();
                    if let Some(value) = lower.strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                    if line.split_once(':').is_some_and(|(name, value)| {
                        name.eq_ignore_ascii_case("authorization") && value.trim() == authorization
                    }) {
                        auth = true;
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let mut status = "200 OK";
                let response = if !auth {
                    status = "401 Unauthorized";
                    Vec::new()
                } else if first.starts_with("MKCOL ") {
                    assert!(first.starts_with("MKCOL /dav/copybook/ "));
                    status = if created {
                        "405 Method Not Allowed"
                    } else {
                        "201 Created"
                    };
                    created = true;
                    Vec::new()
                } else if first.starts_with("PUT ") {
                    let name = first
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .strip_prefix("/dav/copybook/")
                        .unwrap()
                        .to_owned();
                    assert!(body.starts_with(MAGIC));
                    assert!(!body.windows(15).any(|w| w == b"portable secret"));
                    *cloud.lock().unwrap() = (name, body);
                    status = "201 Created";
                    Vec::new()
                } else if first.starts_with("PROPFIND ") {
                    let stored = cloud.lock().unwrap();
                    status = "207 Multi-Status";
                    let (folder, name) = if first.starts_with("PROPFIND /dav/ ") {
                        ("/dav/", "copybook-legacy.cpbk")
                    } else {
                        assert!(first.starts_with("PROPFIND /dav/copybook/ "));
                        ("/dav/copybook/", stored.0.as_str())
                    };
                    format!("<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>{folder}{name}</d:href><d:propstat><d:prop><d:getcontentlength>{}</d:getcontentlength></d:prop></d:propstat></d:response></d:multistatus>",stored.1.len()).into_bytes()
                } else {
                    cloud.lock().unwrap().1.clone()
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.len()
                )
                .unwrap();
                stream.write_all(&response).unwrap();
            }
        });
        let connection = Connection {
            url: format!("http://{addr}/dav/"),
            username,
            password: "super-secret".into(),
        };
        let _forget = ForgetPassword(&connection);
        let archive = Archive {
            schema: 1,
            settings: Settings::default(),
            records: vec![],
            folders: Some(vec![]),
        };
        test_connection(&connection).unwrap();
        let created = create(
            &connection,
            &archive,
            Zeroizing::new("backup password".into()),
        )
        .unwrap();
        assert_eq!(created.password_saved, Some(true));
        let entry = created.entry;
        let entries = list(&connection).unwrap();
        assert_eq!(entries.iter().find(|e| !e.legacy).unwrap().name, entry.name);
        assert_eq!(
            entries.iter().find(|e| e.legacy).unwrap().name,
            "copybook-legacy.cpbk"
        );
        assert_eq!(
            download(&connection, "copybook-legacy.cpbk", "backup password", true)
                .unwrap()
                .schema,
            1
        );
        assert_eq!(
            download(&connection, &entry.name, "wrong password", false)
                .err()
                .unwrap()
                .code,
            "crypto"
        );
        assert_eq!(
            download(&connection, &entry.name, "backup password", false)
                .unwrap()
                .schema,
            1
        );
        let denied = Connection {
            url: connection.url.clone(),
            username: connection.username.clone(),
            password: "incorrect".into(),
        };
        assert_eq!(list(&denied).err().unwrap().code, "auth");
        assert_eq!(
            create(
                &denied,
                &archive,
                Zeroizing::new("would replace password".into())
            )
            .err()
            .unwrap()
            .code,
            "auth"
        );
        let remembered =
            credentials::resolve_encryption(&connection, Zeroizing::new(String::new())).unwrap();
        assert!(remembered.as_str() == "backup password");
        let reused = create(&connection, &archive, Zeroizing::new(String::new())).unwrap();
        assert_eq!(reused.password_saved, None);
        server.join().unwrap();
    }
}
