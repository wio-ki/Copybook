use crate::model::{ListFilter, ListQuery};
use rusqlite::types::Value;

pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}
pub fn escape_like(term: &str) -> String {
    term.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
pub fn clause(query: &ListQuery) -> (String, Vec<Value>) {
    let mut sql = match query.filter {
        ListFilter::All => "1=1",
        ListFilter::Text => "kind='text'",
        ListFilter::Image => "kind='image'",
        ListFilter::Files => "kind='files'",
        ListFilter::Url => "kind='text' AND sub_kind='url'",
        ListFilter::Favorite => "favorite=1",
    }
    .to_owned();
    sql.push_str(" AND deleted_at IS NULL");
    let mut params = Vec::new();
    if let Some(exe) = query.source_exe.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND json_extract(source_json,'$.exe') = ? COLLATE NOCASE");
        params.push(Value::Text(exe.clone()));
    }
    if let Some(since) = query.since {
        sql.push_str(" AND last_used_at >= ?");
        params.push(Value::Integer(since));
    }
    if let Some(until) = query.until {
        sql.push_str(" AND last_used_at < ?");
        params.push(Value::Integer(until));
    }
    if let Some(group) = &query.group {
        sql.push_str(" AND favorite=1 AND coalesce(group_name,'') = ?");
        params.push(Value::Text(group.clone()));
    }
    for term in terms(&query.query) {
        if term.chars().count() >= 3 {
            sql.push_str(" AND id IN (SELECT rowid FROM items_fts WHERE items_fts MATCH ?)");
            params.push(Value::Text(format!("\"{}\"", term.replace('"', "\"\""))));
        } else {
            sql.push_str(" AND search_text LIKE ? ESCAPE '\\'");
            params.push(Value::Text(format!("%{}%", escape_like(&term))));
        }
    }
    (sql, params)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_escape_and_mix() {
        assert_eq!(terms("  HELLO \n 中 中文  "), vec!["hello", "中", "中文"]);
        assert_eq!(escape_like(r"a%_\"), r"a\%\_\\");
        let (sql, params) = clause(&ListQuery {
            query: "中文 HELLO".into(),
            filter: ListFilter::All,
            offset: 0,
            limit: 50,
            ..ListQuery::default()
        });
        assert!(sql.contains("LIKE"));
        assert!(sql.contains("MATCH"));
        assert_eq!(params.len(), 2);
    }
}
