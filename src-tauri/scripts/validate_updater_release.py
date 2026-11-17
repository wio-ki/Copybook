"""Validate the Windows installer, detached signature and uploaded updater manifest."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[2]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def minisign_packet(encoded, size):
    lines = base64.b64decode(encoded.strip(), validate=True).decode("ascii").splitlines()
    require(len(lines) >= 2 and lines[0].startswith("untrusted comment:"),
            "Invalid Minisign envelope")
    packet = base64.b64decode(lines[1], validate=True)
    require(len(packet) == size, "Invalid Minisign packet length")
    return packet, lines


def validate(directory, repository, tag, version_file=ROOT / "package.json",
             config_file=ROOT / "src-tauri" / "tauri.conf.json"):
    version = read_json(version_file)["version"]
    require(tag == f"v{version}", "Release tag does not match package.json")
    require(re.fullmatch(r"[\w.-]+/[\w.-]+", repository) is not None,
            "Invalid repository name")
    asset_name = f"Copybook_{version}_x64-setup.exe"
    installer = directory / asset_name
    data = installer.read_bytes()
    require(data.startswith(b"MZ") and len(data) > 2, "Installer is not a Windows executable")

    release = read_json(directory / "release.json")
    require(release.get("tag_name") == tag, "Release API returned a different tag")
    release_prefix = f"https://github.com/{repository}/releases/tag/"
    release_url = release.get("html_url", "")
    require(isinstance(release_url, str), "Release URL is missing")
    draft_tag = release_url.removeprefix(release_prefix)
    draft_route = (release.get("draft") is True
                   and release_url.startswith(release_prefix)
                   and re.fullmatch(r"untagged-[\w-]+", draft_tag) is not None)
    require(release_url == release_prefix + tag or draft_route,
            "Release belongs to a different repository")
    matches = [asset for asset in release.get("assets", []) if asset.get("name") == asset_name]
    require(len(matches) == 1, "Expected exactly one versioned Windows installer")
    asset = matches[0]
    canonical_url = f"https://github.com/{repository}/releases/download/{tag}/{asset_name}"
    allowed_urls = {canonical_url}
    if draft_route:
        allowed_urls.add(f"https://github.com/{repository}/releases/download/{draft_tag}/{asset_name}")
    require(asset.get("browser_download_url") in allowed_urls,
            "Installer download URL does not match its tag and filename")
    asset_id = asset.get("id")
    require(isinstance(asset_id, int) and asset_id > 0, "Invalid release asset ID")
    api_url = f"https://api.github.com/repos/{repository}/releases/assets/{asset_id}"
    require(asset.get("url") == api_url, "Installer asset API URL targets another repository")
    require(asset.get("size") == len(data), "Installer size differs from GitHub metadata")
    digest = hashlib.sha256(data).hexdigest()
    require(asset.get("digest") == f"sha256:{digest}", "Installer SHA-256 differs from GitHub")

    signature = (directory / f"{asset_name}.sig").read_text(encoding="utf-8").strip()
    packet, lines = minisign_packet(signature, 74)
    require(len(lines) == 4 and lines[2].startswith("trusted comment:"),
            "Detached signature has no trusted comment")
    require(len(base64.b64decode(lines[3], validate=True)) == 64,
            "Detached signature has no global signature")
    updater_config = read_json(config_file)["plugins"]["updater"]
    public_key = updater_config["pubkey"]
    key_packet, _ = minisign_packet(public_key, 42)
    require(packet[2:10] == key_packet[2:10],
            "Detached signature uses a different key than the application")
    signed_version = re.search(r"(?:^|\t)version:([^\t]+)", lines[2])
    require(signed_version is not None or not updater_config.get("requireSignedVersion", False),
            "Application requires the detached signature to declare its version")
    if signed_version:
        require(signed_version.group(1).removeprefix("v") == version,
                "Detached signature declares a different version")

    manifest = read_json(directory / "latest.json")
    require(manifest.get("version") == version, "latest.json declares a different version")
    platforms = manifest.get("platforms", {})
    require(isinstance(platforms, dict) and "windows-x86_64" in platforms,
            "latest.json has no Windows x64 update")
    require(set(platforms) <= {"windows-x86_64", "windows-x86_64-nsis"},
            "latest.json contains an unexpected platform or installer")
    for name, platform in platforms.items():
        require(platform.get("url") in allowed_urls | {api_url},
                f"{name} does not target this release's NSIS installer")
        require(isinstance(platform.get("signature"), str)
                and platform["signature"].strip() == signature,
                f"{name} signature differs from the detached .sig asset")
    print(f"Windows updater assets validated: {tag}; {len(platforms)} platform entries")
    print(f"Installer: {asset_name}; bytes={len(data)}; sha256={digest}")
    print("Manifest, detached signature, application signing key and release metadata agree")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--repository", default="wio-ki/Copybook")
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    try:
        validate(args.directory, args.repository, args.tag)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"Updater release validation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
