use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct CmdError {
    pub code: &'static str,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, CmdError>;
impl CmdError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message)
    }
    pub fn internal() -> Self {
        Self::new("internal", "Operation failed")
    }
}
impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for CmdError {}
impl From<std::io::Error> for CmdError {
    fn from(_: std::io::Error) -> Self {
        Self::new("io", "File operation failed")
    }
}
impl From<rusqlite::Error> for CmdError {
    fn from(_: rusqlite::Error) -> Self {
        Self::new("db", "Database operation failed")
    }
}
impl From<windows::core::Error> for CmdError {
    fn from(_: windows::core::Error) -> Self {
        Self::new("internal", "Windows operation failed")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    Text,
    Image,
    Files,
}
impl ClipKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Files => "files",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceApp {
    pub name: String,
    pub exe: String,
    pub icon: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub path: String,
    pub thumb: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub exists: bool,
    pub bytes: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipItem {
    pub id: i64,
    pub kind: ClipKind,
    pub sub_kind: Option<String>,
    pub text: String,
    pub text_length: u32,
    pub line_count: u32,
    pub has_html: bool,
    pub has_rtf: bool,
    pub image: Option<ImageInfo>,
    pub files: Option<Vec<FileEntry>>,
    pub source: Option<SourceApp>,
    pub favorite: bool,
    pub pinned: bool,
    pub note: Option<String>,
    pub use_count: i64,
    pub created_at: i64,
    pub last_used_at: i64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListFilter {
    All,
    Text,
    Image,
    Files,
    Url,
    Favorite,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub query: String,
    pub filter: ListFilter,
    pub offset: i64,
    pub limit: i64,
}
#[derive(Debug, Serialize)]
pub struct ListResult {
    pub items: Vec<ClipItem>,
    pub total: i64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemContent {
    pub text: String,
    pub truncated: bool,
    pub html: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct PasteOutcome {
    pub pasted: bool,
    pub reason: Option<&'static str>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total: i64,
    pub text: i64,
    pub image: i64,
    pub files: i64,
    pub favorite: i64,
    pub pinned: i64,
    pub marked: i64,
    pub db_bytes: u64,
    pub image_bytes: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub elevated: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub url: String,
    pub installer: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct SeenApp {
    #[serde(flatten)]
    pub source: SourceApp,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub kind: ClipKind,
    pub text: String,
    pub html: Option<String>,
    pub rtf: Option<Vec<u8>>,
    pub pixels: Option<image::RgbaImage>,
    pub paths: Vec<String>,
    pub source: Option<SourceApp>,
}
impl Capture {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: ClipKind::Text,
            text: text.into(),
            html: None,
            rtf: None,
            pixels: None,
            paths: Vec::new(),
            source: None,
        }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
