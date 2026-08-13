#!/usr/bin/env python3
"""Sign Tauri v2 updater bundles with a minisign-ed25519 keypair.

This implements the *minisign-rs* signature format that Tauri's updater plugin
verifies against (rsign2 / jedisct1 minisign crate, v0.9.x). It does NOT rely on
the `tauri signer` CLI (which is broken on the local Windows build), and it does
NOT encrypt the secret key — the secret is supplied directly via the
TAURI_SIGNING_PRIVATE_KEY environment variable.

Signature format (per minisign-rs `SignatureBox::to_string`):
    untrusted comment: <untrusted_comment>
    <base64(b"ED" + keynum[8] + sig1[64])>
    trusted comment: <trusted_comment_payload>
    <base64(global_sig[64])>

  sig1       = ed25519( prehash(msg) )                       # prehash = BLAKE2b-512
  sig2_input = sig1_box(74) || trusted_comment_payload       # NO "trusted comment: " prefix
  global_sig = ed25519( sig2_input )

The updater.json `signature` field for each platform is the full 4-line block.
"""

import argparse
import base64
import hashlib
import json
import os
import sys
import time

from nacl.exceptions import BadSignatureError
from nacl.signing import SigningKey, VerifyKey

# ---------------------------------------------------------------------------
# Low-level primitives (must match minisign-rs exactly)
# ---------------------------------------------------------------------------

def prehash(data: bytes) -> bytes:
    # minisign-rs: Blake2b::new(64) -> BLAKE2b-512 with default params (no
    # personalization, no key). hashlib.blake2b default matches this.
    return hashlib.blake2b(data, digest_size=64).digest()


def b64encode(b: bytes) -> str:
    return base64.b64encode(b).decode("ascii")


def b64decode(s: str) -> bytes:
    return base64.b64decode(s.strip())


def load_secret(raw: str) -> tuple[bytes, bytes]:
    """Return (keynum[8], seed[32]) from base64(keynum[8] + seed[32]) = 40 bytes."""
    data = b64decode(raw)
    if len(data) != 40:
        raise ValueError(
            f"TAURI_SIGNING_PRIVATE_KEY must decode to 40 bytes "
            f"(keynum[8]+seed[32]); got {len(data)} bytes"
        )
    return data[:8], data[8:40]


def pubkey_block(keynum: bytes, pub: bytes) -> str:
    """Two-line minisign public key text."""
    body = keynum + pub
    keyid = int.from_bytes(keynum, "little")
    return (
        f"untrusted comment: minisign public key: {keyid:016X}\n"
        + b64encode(body)
        + "\n"
    )


def pubkey_conf_value(keynum: bytes, pub: bytes) -> str:
    """The value to write into tauri.conf.json `plugins.updater.pubkey`:
    base64 of the two-line public key block."""
    return b64encode(pubkey_block(keynum, pub).encode("utf-8"))


def sign_file(keynum: bytes, seed: bytes, path: str) -> str:
    sk = SigningKey(seed)
    msg = open(path, "rb").read()
    digest = prehash(msg)
    sig1 = sk.sign(digest).signature  # 64 bytes
    sig1_box = b"ED" + keynum + sig1
    ts = int(time.time())
    basename = os.path.basename(path)
    trusted = f"timestamp:{ts} file:{basename}"
    sig2_input = sig1_box + trusted.encode("utf-8")
    global_sig = sk.sign(sig2_input).signature  # 64 bytes
    return "\n".join(
        [
            "untrusted comment: signature from tauri secret key",
            b64encode(sig1_box),
            "trusted comment: " + trusted,
            b64encode(global_sig),
        ]
    ) + "\n"


def verify_file(keynum: bytes, pub: bytes, path: str, sig_text: str) -> None:
    """Raise BadSignatureError if the signature is invalid. Mirrors minisign-rs."""
    vk = VerifyKey(pub)
    lines = sig_text.strip().split("\n")
    if len(lines) < 4:
        raise BadSignatureError("signature block must have 4 lines")
    sig1_box = b64decode(lines[1])
    if sig1_box[:2] != b"ED":
        raise BadSignatureError("signature algorithm is not ED (prehashed)")
    sig_keynum = sig1_box[2:10]
    sig1 = sig1_box[10:74]
    if sig_keynum != keynum:
        raise BadSignatureError("signature keynum does not match public key keynum")
    trusted = lines[2][len("trusted comment: "):]
    global_sig = b64decode(lines[3])
    msg = open(path, "rb").read()
    digest = prehash(msg)
    # ed25519 verify: VerifyKey.verify(sig + message)
    vk.verify(sig1 + digest)
    vk.verify(global_sig + (sig1_box + trusted.encode("utf-8")))


# ---------------------------------------------------------------------------
# Bundle discovery
# ---------------------------------------------------------------------------

def classify(path: str) -> str | None:
    """Map a bundle path to its Tauri v2 platform key, or None to skip."""
    name = os.path.basename(path).lower()
    if name.endswith(".msi"):
        return "windows-x86_64"
    if name.endswith(".exe"):
        # NSIS installer
        return "windows-x86_64"
    if name.endswith(".dmg"):
        if "aarch64" in name:
            return "darwin-aarch64"
        if "x86_64" in name or "x64" in name:
            return "darwin-x86_64"
        # Default Unknown mac arch -> assume aarch64 (Apple Silicon runners)
        return "darwin-aarch64"
    if name.endswith(".appimage"):
        return "linux-x86_64"
    if name.endswith(".deb"):
        return "linux-x86_64"
    return None


# Preference when multiple bundles resolve to the same platform key.
_PREF = {".msi": 0, ".exe": 1, ".appimage": 0, ".deb": 1}


def discover(bundle_dir: str) -> dict[str, str]:
    """Return {platform_key: absolute_bundle_path} picking the preferred file."""
    found: dict[str, tuple[int, str]] = {}
    for root, _dirs, files in os.walk(bundle_dir):
        for f in files:
            path = os.path.join(root, f)
            key = classify(path)
            if key is None:
                continue
            ext = os.path.splitext(f.lower())[1]
            pref = _PREF.get(ext, 9)
            if key not in found or pref < found[key][0]:
                found[key] = (pref, path)
    return {k: v[1] for k, v in found.items()}


# ---------------------------------------------------------------------------
# updater.json generation
# ---------------------------------------------------------------------------

def build_updater_json(
    tag: str,
    repo: str,
    platforms: dict[str, str],
    signatures: dict[str, str],
    notes: str,
) -> dict:
    version = tag[1:] if tag.startswith("v") else tag
    base = f"https://github.com/{repo}/releases/download/{tag}"
    out_platforms = {}
    for key, path in platforms.items():
        basename = os.path.basename(path)
        out_platforms[key] = {
            "signature": signatures[key],
            "url": f"{base}/{basename}",
        }
    return {
        "version": version,
        "notes": notes,
        "pub_date": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platforms": out_platforms,
    }


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(description="Sign Tauri v2 updater bundles")
    ap.add_argument("--tag", required=True, help="release tag, e.g. v5.3.11")
    ap.add_argument("--bundle-dir", required=True, help="directory to scan for bundles")
    ap.add_argument(
        "--out",
        default="updater.json",
        help="output path for updater.json (default: ./updater.json)",
    )
    ap.add_argument("--repo", default="wq1131173682/antigravity-hub")
    ap.add_argument("--notes", default="")
    ap.add_argument(
        "--secret",
        default=None,
        help="base64(keynum[8]+seed[32]); defaults to $TAURI_SIGNING_PRIVATE_KEY",
    )
    ap.add_argument(
        "--verify",
        action="store_true",
        help="self-verify each signature before writing updater.json",
    )
    args = ap.parse_args()

    raw = args.secret or os.environ.get("TAURI_SIGNING_PRIVATE_KEY")
    if not raw:
        print("ERROR: TAURI_SIGNING_PRIVATE_KEY not set", file=sys.stderr)
        return 2
    keynum, seed = load_secret(raw)
    sk = SigningKey(seed)
    pub = sk.verify_key.encode()

    platforms = discover(args.bundle_dir)
    if not platforms:
        print(f"ERROR: no bundles found under {args.bundle_dir}", file=sys.stderr)
        return 3

    print(f"Discovered {len(platforms)} platform bundle(s):")
    for k, p in sorted(platforms.items()):
        print(f"  {k:20s} <- {p}")

    signatures: dict[str, str] = {}
    for key, path in platforms.items():
        sig = sign_file(keynum, seed, path)
        signatures[key] = sig
        if args.verify:
            verify_file(keynum, pub, path, sig)
            print(f"  verified OK: {key}")
        # Also write the standalone .sig file (useful for manual inspection).
        with open(path + ".sig", "w") as fh:
            fh.write(sig)

    data = build_updater_json(args.tag, args.repo, platforms, signatures, args.notes)
    with open(args.out, "w") as fh:
        json.dump(data, fh, indent=2)
        fh.write("\n")
    print(f"Wrote {args.out}")
    print("Platforms signed:")
    for k in sorted(data["platforms"]):
        print(f"  {k}: {data['platforms'][k]['url']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
