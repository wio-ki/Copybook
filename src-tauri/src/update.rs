//! User-triggered update check against the GitHub Releases API.
//!
//! Nothing here downloads or installs anything: the result only tells the settings window which
//! release is newest so it can offer the release page and, when the release carries one, the
//! installer asset. WinHTTP is used so the request honours the system proxy and certificate
//! store without adding an HTTP stack to the build.

use crate::model::{CmdError, Result, UpdateInfo};
use serde_json::Value;
use std::ptr::null_mut;
use windows::{core::PCWSTR, Win32::Networking::WinHttp::*};

const HOST: &str = "api.github.com";
const PATH: &str = "/repos/wio-ki/Copybook/releases/latest";
const MAX_BODY: usize = 1024 * 1024;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn network(message: impl Into<String>) -> CmdError {
    CmdError::new("network", message)
}

/// Wraps a WinHTTP handle so every early return still releases it.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was created by WinHTTP and is closed exactly once here.
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// `GET` the release JSON. Blocks for at most the timeouts set below; call it off the UI thread.
fn get(path: &str) -> Result<Vec<u8>> {
    let agent = wide("Copybook");
    let verb = wide("GET");
    let object = wide(path);
    // WinHttpSendRequest counts the slice length itself, so the terminating NUL must stay out.
    let headers: Vec<u16> = "Accept: application/vnd.github+json\r\nUser-Agent: Copybook\r\n"
        .encode_utf16()
        .collect();
    let host = wide(HOST);
    // SAFETY: every pointer below outlives its call and every handle is checked before use.
    unsafe {
        let session = Handle(WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(network("Could not start an HTTP session"));
        }
        if WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 8000).is_err() {
            return Err(network("Could not set HTTP timeouts"));
        }
        let connection = Handle(WinHttpConnect(
            session.0,
            PCWSTR(host.as_ptr()),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ));
        if connection.0.is_null() {
            return Err(network("Could not reach GitHub"));
        }
        let request = Handle(WinHttpOpenRequest(
            connection.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(object.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            null_mut(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err(network("Could not prepare the request"));
        }
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0)
            .map_err(|_| network("Could not send the request"))?;
        WinHttpReceiveResponse(request.0, null_mut())
            .map_err(|_| network("No response from GitHub"))?;
        let mut body = Vec::new();
        loop {
            let mut buffer = vec![0u8; 16 * 1024];
            let mut read = 0u32;
            let chunk = buffer.as_mut_slice();
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )
            .map_err(|_| network("Could not read the response"))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > MAX_BODY {
                return Err(network("The response was unexpectedly large"));
            }
        }
        if body.is_empty() {
            // A missing release (or a rate limit) answers with an error status and a JSON error
            // body; without a tag there is nothing to compare, so report it as no answer.
            return Err(network("GitHub returned no release"));
        }
        Ok(body)
    }
}

/// True when `latest` is a strictly higher dotted version than `current`. A pre-release suffix
/// (`-beta.1`) sorts below the same release, and missing components count as zero.
pub fn newer(current: &str, latest: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches(['v', 'V']);
        let (numbers, prerelease) = match v.split_once('-') {
            Some((n, _)) => (n, true),
            None => (v, false),
        };
        (
            numbers
                .split('.')
                .map(|p| {
                    p.chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                        .parse()
                        .unwrap_or(0)
                })
                .collect(),
            prerelease,
        )
    }
    let (a, a_pre) = parts(current);
    let (b, b_pre) = parts(latest);
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return y > x;
        }
    }
    // Same numbers: a release outranks a pre-release of it, and never the reverse.
    a_pre && !b_pre
}

fn installer_of(release: &Value) -> Option<String> {
    release
        .get("assets")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            let url = asset.get("browser_download_url")?.as_str()?;
            name.ends_with("-setup.exe").then(|| url.to_owned())
        })
        .next()
}

pub fn check(current: &str) -> Result<UpdateInfo> {
    let body = get(PATH)?;
    let release: Value =
        serde_json::from_slice(&body).map_err(|_| network("GitHub's answer could not be read"))?;
    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| network("GitHub returned no release"))?;
    let url = release
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or("https://github.com/wio-ki/Copybook/releases")
        .to_owned();
    let latest = tag.trim().trim_start_matches(['v', 'V']).to_owned();
    Ok(UpdateInfo {
        current: current.to_owned(),
        newer: newer(current, &latest),
        latest,
        url,
        installer: installer_of(&release),
    })
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn compares_dotted_versions() {
        assert!(newer("1.0.0", "1.0.1"));
        assert!(newer("1.0.0", "1.1"));
        assert!(newer("1.9.9", "2.0.0"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("1.2.0", "1.2"));
        assert!(newer("0.1.0", "1"));
    }

    /// Needs the network; run with `cargo test -- --ignored` to check WinHTTP against the
    /// real Releases API.
    #[test]
    #[ignore]
    fn reaches_github() {
        let info = super::check("0.0.1").unwrap();
        assert!(!info.latest.is_empty(), "{info:?}");
        assert!(info.newer, "{info:?}");
        assert!(
            info.url.starts_with("https://github.com/wio-ki/Copybook"),
            "{info:?}"
        );
    }

    #[test]
    fn prerelease_and_prefix_rules() {
        assert!(!newer("1.0.0", "1.0.0-beta.1"));
        assert!(newer("1.0.0-beta.1", "1.0.0"));
        assert!(newer("1.0.0", "v1.0.1"));
        assert!(newer("V1.0.0", "1.0.1"));
        assert!(!newer("1.0.1", "1.0.0"));
    }
}
