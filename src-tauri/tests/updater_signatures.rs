//! Offline verification of real, version-bound updater signatures.
//!
//! The payload is inert binary data, not an installer. Fixtures were signed using throwaway
//! keys kept outside the source tree; only the public key and signatures are checked in.
//! These tests never request a URL, open a window, install a package or exit the application.

use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::{Error, Update, UpdaterExt};

const PAYLOAD: &[u8] = include_bytes!("fixtures/updater-payload.bin");
const PUBLIC_KEY: &str = include_str!("fixtures/updater-test-public.txt");
const SIGNED_VERSION: &str = include_str!("fixtures/updater-signed-version.txt");
const UNTRUSTED_SIGNATURE: &str = include_str!("fixtures/updater-untrusted-signature.txt");
const UNVERSIONED_SIGNATURE: &str = include_str!("fixtures/updater-unversioned-signature.txt");

fn restored_update(version: &str, signature: &str) -> Update {
    restored_update_with_key(version, signature, PUBLIC_KEY)
}

fn restored_update_with_key(version: &str, signature: &str, public_key: &str) -> Update {
    let mut context = mock_context(noop_assets());
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".into(), json!({ "pubkey": public_key.trim() }));
    let app = mock_builder()
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(public_key.trim())
                .build(),
        )
        .build(context)
        .expect("mock application must initialize without a desktop");
    let updater = app
        .updater_builder()
        .endpoints(vec![
            "https://github.com/wio-ki/Copybook/releases/latest/download/latest.json"
                .parse()
                .expect("fixed official HTTPS endpoint must be valid"),
        ])
        .expect("fixed official endpoint must be accepted")
        .build()
        .expect("mock updater must initialize");
    updater
        .restore_update(json!({
            "version": version,
            "platforms": {
                "windows-x86_64": {
                    "url": "https://github.com/wio-ki/Copybook/releases/download/v1.3.0/Copybook_1.3.0_x64-setup.exe",
                    "signature": signature.trim()
                }
            }
        }))
        .expect("offline manifest must deserialize")
        .expect("fixture version must be newer than the mock application")
}

#[test]
fn accepts_the_original_payload_and_its_signed_version() {
    let update = restored_update("1.3.0", SIGNED_VERSION);
    assert_eq!(update.version, "1.3.0");
    update
        .verify(PAYLOAD)
        .expect("authentic fixture must pass package and version verification");
}

#[test]
fn rejects_a_modified_download_before_installation() {
    let update = restored_update("1.3.0", SIGNED_VERSION);
    let mut corrupted = PAYLOAD.to_vec();
    corrupted[0] ^= 1;
    assert!(matches!(update.verify(&corrupted), Err(Error::Minisign(_))));
}

#[test]
fn rejects_a_valid_signature_from_an_untrusted_key() {
    // The same payload and version were signed by another throwaway key. This is a
    // structurally valid signature, so rejection exercises trust rather than base64 parsing.
    let update = restored_update("1.3.0", UNTRUSTED_SIGNATURE);
    assert!(matches!(update.verify(PAYLOAD), Err(Error::Minisign(_))));
}

#[test]
fn rejects_manifest_version_substitution_for_an_authentic_older_package() {
    let update = restored_update("1.3.1", SIGNED_VERSION);
    match update.verify(PAYLOAD) {
        Err(Error::SignedVersionMismatch { signed, announced }) => {
            assert_eq!(signed, "1.3.0");
            assert_eq!(announced, "1.3.1");
        }
        other => panic!("version substitution must fail with SignedVersionMismatch: {other:?}"),
    }
}

#[test]
fn restored_metadata_requires_a_version_bound_signature() {
    // restore_update must enforce binding even though the mock plugin uses its defaults.
    let update = restored_update("1.3.0", UNVERSIONED_SIGNATURE);
    assert!(matches!(
        update.verify(PAYLOAD),
        Err(Error::MissingSignedVersion)
    ));
}

#[test]
#[ignore = "requires a locally built signed NSIS bundle"]
fn verifies_the_locally_built_nsis_with_the_shipped_public_key() {
    let package: serde_json::Value =
        serde_json::from_str(include_str!("../../package.json")).unwrap();
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let version = package["version"].as_str().unwrap();
    let public_key = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
    let bundle =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/release/bundle/nsis");
    let name = format!("Copybook_{version}_x64-setup.exe");
    let bytes = std::fs::read(bundle.join(&name)).expect("build the signed NSIS bundle first");
    let signature = std::fs::read_to_string(bundle.join(format!("{name}.sig")))
        .expect("signed NSIS bundle must include its .sig file");
    assert!(
        bytes.starts_with(b"MZ"),
        "bundle must be a Windows executable"
    );
    restored_update_with_key(version, &signature, public_key)
        .verify(&bytes)
        .expect(
            "shipped public key must verify the installer bytes and signed application version",
        );
    println!(
        "Verified signed NSIS {version}: {} bytes; no installer executed",
        bytes.len()
    );
}
