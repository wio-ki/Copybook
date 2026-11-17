"""Exercise release-metadata failures without downloading or executing an installer."""
import base64
from contextlib import redirect_stdout
import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest

from validate_updater_release import ROOT, validate


def envelope(packet, *, signature=False):
    lines = ["untrusted comment: test fixture", base64.b64encode(packet).decode()]
    if signature:
        lines.extend(["trusted comment: timestamp:1\tversion:1.3.0",
                      base64.b64encode(bytes(64)).decode()])
    return base64.b64encode("\n".join(lines).encode()).decode()


class ReleaseValidationTests(unittest.TestCase):
    def setUp(self):
        (ROOT / ".review").mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / ".review")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.name = "Copybook_1.3.0_x64-setup.exe"
        self.data = b"MZinstaller fixture"
        (self.directory / self.name).write_bytes(self.data)
        # Synthetic envelopes test metadata matching; Tauri performs cryptographic verification.
        self.signature = envelope(b"ED" + b"key-id-1" + bytes(64), signature=True)
        (self.directory / f"{self.name}.sig").write_text(self.signature, encoding="utf-8")
        self.version_file = self.directory / "package.json"
        self.version_file.write_text('{"version":"1.3.0"}', encoding="utf-8")
        self.config_file = self.directory / "tauri.conf.json"
        self.config_file.write_text(json.dumps({"plugins": {"updater": {
            "pubkey": envelope(b"Ed" + b"key-id-1" + bytes(32)),
            "requireSignedVersion": True}}}), encoding="utf-8")
        self.api_url = "https://api.github.com/repos/wio-ki/Copybook/releases/assets/123"
        self.browser_url = f"https://github.com/wio-ki/Copybook/releases/download/v1.3.0/{self.name}"
        self.manifest = {"version": "1.3.0", "platforms": {
            name: {"url": self.api_url, "signature": self.signature}
            for name in ["windows-x86_64", "windows-x86_64-nsis"]}}
        self.release = {"tag_name": "v1.3.0",
                        "html_url": "https://github.com/wio-ki/Copybook/releases/tag/v1.3.0",
                        "assets": [{"id": 123, "name": self.name, "url": self.api_url,
                                    "browser_download_url": self.browser_url, "size": len(self.data),
                                    "digest": "sha256:" + hashlib.sha256(self.data).hexdigest()}]}

    def run_validation(self, manifest=None, release=None):
        (self.directory / "latest.json").write_text(json.dumps(
            self.manifest if manifest is None else manifest), encoding="utf-8")
        (self.directory / "release.json").write_text(json.dumps(
            self.release if release is None else release), encoding="utf-8-sig")
        with redirect_stdout(io.StringIO()):
            validate(self.directory, "wio-ki/Copybook", "v1.3.0", self.version_file, self.config_file)

    def test_accepts_api_and_canonical_urls_with_both_platform_aliases(self):
        self.run_validation()
        for platform in self.manifest["platforms"].values():
            platform["url"] = self.browser_url
        self.run_validation()

    def test_accepts_github_draft_routes_bound_to_the_correct_tag(self):
        self.release["draft"] = True
        self.release["html_url"] = "https://github.com/wio-ki/Copybook/releases/tag/untagged-123abc"
        self.release["assets"][0]["browser_download_url"] = self.browser_url.replace(
            "/v1.3.0/", "/untagged-123abc/")
        self.run_validation()
        self.release["draft"] = False
        with self.assertRaises(ValueError):
            self.run_validation()

    def test_rejects_wrong_manifest_version_repository_installer_or_signature(self):
        cases = []
        wrong = copy.deepcopy(self.manifest)
        wrong["version"] = "1.2.0"
        cases.append(wrong)
        for value in ["https://example.com/installer.exe",
                      self.browser_url.replace("v1.3.0", "v1.2.0"),
                      self.api_url.replace("/123", "/456")]:
            wrong = copy.deepcopy(self.manifest)
            wrong["platforms"]["windows-x86_64"]["url"] = value
            cases.append(wrong)
        wrong = copy.deepcopy(self.manifest)
        wrong["platforms"]["windows-x86_64"]["signature"] = "different"
        cases.append(wrong)
        wrong = copy.deepcopy(self.manifest)
        wrong["platforms"]["windows-x86_64-msi"] = wrong["platforms"]["windows-x86_64"]
        cases.append(wrong)
        for manifest in cases:
            with self.subTest(manifest=manifest), self.assertRaises(ValueError):
                self.run_validation(manifest)

    def test_rejects_wrong_release_tag_and_corrupted_download(self):
        wrong = copy.deepcopy(self.release)
        wrong["tag_name"] = "v1.2.0"
        with self.assertRaises(ValueError):
            self.run_validation(release=wrong)
        (self.directory / self.name).write_bytes(b"MZcorrupted fixture")
        with self.assertRaises(ValueError):
            self.run_validation()

    def test_rejects_missing_signature_and_different_public_key(self):
        signature_file = self.directory / f"{self.name}.sig"
        signature_file.write_text("", encoding="utf-8")
        with self.assertRaises(ValueError):
            self.run_validation()
        signature_file.write_text(self.signature, encoding="utf-8")
        self.config_file.write_text(json.dumps({"plugins": {"updater": {
            "pubkey": envelope(b"Ed" + b"key-id-2" + bytes(32))}}}), encoding="utf-8")
        with self.assertRaises(ValueError):
            self.run_validation()

    def test_rejects_legacy_signature_without_required_signed_version(self):
        decoded = base64.b64decode(self.signature).decode().replace("\tversion:1.3.0", "")
        signature = base64.b64encode(decoded.encode()).decode()
        (self.directory / f"{self.name}.sig").write_text(signature, encoding="utf-8")
        for platform in self.manifest["platforms"].values():
            platform["signature"] = signature
        with self.assertRaisesRegex(ValueError, "declare its version"):
            self.run_validation()


if __name__ == "__main__":
    unittest.main()
