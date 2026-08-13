#!/usr/bin/env python3
"""Generate a consistent ed25519 minisign keypair for Tauri v2 updates.

Outputs:
  1) The value to paste into tauri.conf.json `plugins.updater.pubkey`
     (base64 of the two-line public key text).
  2) The secret value to store in GitHub secret TAURI_SIGNING_PRIVATE_KEY
     (base64 of keynum[8] + ed25519_seed[32], 40 bytes).

No key encryption is used; the secret is supplied directly to sign_updater.py.
"""

import base64
import os
import sys

from nacl.signing import SigningKey


def main() -> int:
    sk = SigningKey.generate()
    seed = sk.encode()[:32]
    pub = sk.verify_key.encode()
    keynum = os.urandom(8)

    body = keynum + pub
    keyid = int.from_bytes(keynum, "little")
    pub_block = (
        f"untrusted comment: minisign public key: {keyid:016X}\n"
        + base64.b64encode(body).decode()
        + "\n"
    )
    pub_conf = base64.b64encode(pub_block.encode()).decode()
    secret = base64.b64encode(keynum + seed).decode()

    print("=== tauri.conf.json  plugins.updater.pubkey ===")
    print(pub_conf)
    print()
    print("=== GitHub secret  TAURI_SIGNING_PRIVATE_KEY ===")
    print(secret)
    print()
    print(f"(public key id: {keyid:016X})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
