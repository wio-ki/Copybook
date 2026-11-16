use regex::Regex;
use std::sync::LazyLock;

fn matches(pattern: &'static str, text: &str) -> bool {
    // Patterns are constant application code, never clipboard input.
    static PATTERNS: LazyLock<std::sync::Mutex<std::collections::HashMap<&'static str, Regex>>> =
        LazyLock::new(Default::default);
    let Ok(mut patterns) = PATTERNS.lock() else {
        return false;
    };
    if !patterns.contains_key(pattern) {
        let Ok(regex) = Regex::new(pattern) else {
            return false;
        };
        patterns.insert(pattern, regex);
    }
    patterns.get(pattern).is_some_and(|r| r.is_match(text))
}

pub fn sub_kind(text: &str) -> Option<&'static str> {
    let text = text.trim();
    if !text.chars().any(char::is_whitespace) {
        let candidate = if text.starts_with("www.") {
            format!("https://{text}")
        } else {
            text.to_owned()
        };
        if url::Url::parse(&candidate).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty())
        }) {
            return Some("url");
        }
        if matches(
            r"^[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)+$",
            text,
        ) {
            return Some("email");
        }
    }
    if color(text) {
        return Some("color");
    }
    let path = text
        .strip_prefix('"')
        .and_then(|p| p.strip_suffix('"'))
        .unwrap_or(text);
    if !path.contains(['\n', '\r', '\0', '"', '<', '>', '|', '?', '*'])
        && (matches(r"^[A-Za-z]:[\\/]", path) || matches(r"^\\\\[^\\/]+\\[^\\/]+", path))
    {
        return Some("path");
    }
    None
}

fn number(value: &str, min: f64, max: f64) -> bool {
    value
        .parse::<f64>()
        .is_ok_and(|n| n.is_finite() && (min..=max).contains(&n))
}
fn component(value: &str, max: f64) -> bool {
    value
        .strip_suffix('%')
        .map_or_else(|| number(value, 0.0, max), |s| number(s, 0.0, 100.0))
}
fn color(text: &str) -> bool {
    if let Some(hex) = text.strip_prefix('#') {
        return [3, 4, 6, 8].contains(&hex.len()) && hex.bytes().all(|c| c.is_ascii_hexdigit());
    }
    let lower = text.to_ascii_lowercase();
    let Some((name, inner)) = lower.split_once('(') else {
        return false;
    };
    let Some(inner) = inner.strip_suffix(')') else {
        return false;
    };
    if !["rgb", "rgba", "hsl", "hsla"].contains(&name) {
        return false;
    }
    let comma = inner.contains(',');
    let fields: Vec<&str> = if comma {
        if inner.contains('/') {
            return false;
        }
        inner.split(',').map(str::trim).collect()
    } else {
        let (channels, alpha) = inner
            .split_once('/')
            .map_or((inner, None), |(c, a)| (c, Some(a.trim())));
        let mut fields: Vec<_> = channels.split_whitespace().collect();
        if fields.len() != 3 {
            return false;
        }
        if let Some(alpha) = alpha {
            if alpha.contains('/') || !component(alpha, 1.0) {
                return false;
            }
            fields.push(alpha);
        }
        fields
    };
    if fields.len() < 3
        || fields.len() > 4
        || (comma && fields.len() != if name.ends_with('a') { 4 } else { 3 })
    {
        return false;
    }
    if fields.len() == 4 && !component(fields[3], 1.0) {
        return false;
    }
    if name.starts_with("rgb") {
        if comma
            && fields[..3]
                .iter()
                .any(|s| s.ends_with('%') != fields[0].ends_with('%'))
        {
            return false;
        }
        return fields[..3].iter().all(|s| component(s, 255.0));
    }
    let hue = ["deg", "grad", "rad", "turn"]
        .iter()
        .find_map(|u| fields[0].strip_suffix(u))
        .unwrap_or(fields[0]);
    number(hue, f64::MIN, f64::MAX)
        && fields[1..3]
            .iter()
            .all(|s| s.strip_suffix('%').is_some_and(|v| number(v, 0.0, 100.0)))
}

pub fn secret(text: &str) -> bool {
    let text = text.trim();
    if matches(
        r"(?s)^-----BEGIN (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY-----\s+[A-Za-z0-9+/=\s]+\s+-----END (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY-----$",
        text,
    ) {
        return true;
    }
    matches(
        r"^(?:(?:gh[pousr]_|github_pat_)[A-Za-z0-9_]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16}|xox[abprs]-[A-Za-z0-9-]{10,}|sk-(?:ant-)?[A-Za-z0-9_-]{20,})$",
        text,
    ) || (matches(
        r"^eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$",
        text,
    ) && jwt_json(text))
}
fn jwt_json(text: &str) -> bool {
    // Decode the two base64url JSON segments to avoid matching ordinary dotted words.
    fn segment(part: &str) -> Option<serde_json::Value> {
        let mut bytes = Vec::new();
        let mut bits = 0u32;
        let mut count = 0u32;
        for c in part.bytes() {
            let n = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'-' => 62,
                b'_' => 63,
                _ => return None,
            };
            bits = (bits << 6) | u32::from(n);
            count += 6;
            if count >= 8 {
                count -= 8;
                bytes.push((bits >> count) as u8);
            }
        }
        serde_json::from_slice(&bytes).ok()
    }
    let mut parts = text.split('.');
    parts
        .next()
        .and_then(segment)
        .is_some_and(|v| v.is_object() && v.get("alg").is_some())
        && parts
            .next()
            .and_then(segment)
            .is_some_and(|v| v.is_object())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detection() {
        for s in [
            "https://example.org/a?q=b",
            "http://localhost:1420/",
            "www.example.com",
            "https://example.org/end.",
        ] {
            assert_eq!(sub_kind(s), Some("url"), "{s}");
        }
        for s in [
            "example.com",
            "https://a.test space",
            "a prose sentence",
            "./relative",
            "foo\\bar",
            "C:relative",
        ] {
            assert_eq!(sub_kind(s), None, "{s}");
        }
        assert_eq!(sub_kind("person+tag@domain.example"), Some("email"));
        for s in [
            "#abc",
            "#abcd",
            "#123456",
            "#12345678",
            "rgb(255, 0, 5)",
            "rgba(1,2,3,0.5)",
            "hsl(360,100%,50%)",
            "hsla(12, 5%, 2%, 50%)",
            "rgb(10 20 30 / 50%)",
        ] {
            assert_eq!(sub_kind(s), Some("color"), "{s}");
        }
        for s in [
            "#ab",
            "#abcde",
            "#ggg",
            "rgb(256,0,0)",
            "rgba(1,2,3,2)",
            "hsl(1,2,3)",
            "rgb(a,b,c)",
        ] {
            assert_eq!(sub_kind(s), None, "{s}");
        }
        for s in [
            r"C:\Users\a b\file.txt",
            r"\\server\share\file",
            r#""D:\some file""#,
        ] {
            assert_eq!(sub_kind(s), Some("path"), "{s}");
        }
    }
    #[test]
    fn secrets_are_whole_tokens() {
        for prefix in [
            "ghp_",
            "gho_",
            "ghu_",
            "ghs_",
            "ghr_",
            "github_pat_",
            "sk-",
            "sk-ant-",
            "xoxa-",
            "xoxb-",
            "xoxp-",
            "xoxr-",
            "xoxs-",
        ] {
            let token = format!("{prefix}{}", "a".repeat(40));
            assert!(secret(&token), "{prefix}");
            assert!(!secret(&format!("Here is {token} for testing")));
        }
        for prefix in ["AKIA", "ASIA"] {
            assert!(secret(&format!("{prefix}1234567890ABCDEF")));
        }
        let pem = format!(
            "-----BEGIN {}-----\nYWJj\n-----END {}-----",
            "PRIVATE KEY", "PRIVATE KEY"
        );
        assert!(secret(&pem));
        use base64::Engine;
        let encode = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        let jwt = [
            encode(r#"{"alg":"HS256"}"#),
            encode(r#"{"sub":"test"}"#),
            encode("test-signature"),
        ]
        .join(".");
        assert!(secret(&jwt));
        for s in [
            "mentions sk- here",
            "eyJnormal.eyJword.signature",
            "-----BEGIN PUBLIC KEY-----\nYWJj\n-----END PUBLIC KEY-----",
        ] {
            assert!(!secret(s), "{s}");
        }
    }
}
