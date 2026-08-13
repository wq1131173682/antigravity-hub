#!/bin/bash
# Local signing script for Tauri auto-updater
# Usage: ./scripts/sign-release.sh <version> <file1> [file2 ...]
#
# This script signs release assets with minisign and generates updater.json
# Run this locally when creating a new release, then upload the signed files

set -euo pipefail

VERSION="${1:?Usage: $0 <version> <file1> [file2 ...]}"
shift

if [ $# -eq 0 ]; then
    echo "Error: No files to sign"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MINISIGN_KEY="$PROJECT_ROOT/src-tauri/minisign/minisign.key"

if [ ! -f "$MINISIGN_KEY" ]; then
    echo "Error: Minisign key not found at $MINISIGN_KEY"
    echo "Generate it first: minisign -G -s src-tauri/minisign/minisign.key"
    exit 1
fi

# Install minisign if not present
if ! command -v minisign &> /dev/null; then
    echo "Installing minisign..."
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        wget -q "https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-linux.tar.gz"
        tar xzf minisign-0.12-linux.tar.gz
        sudo cp minisign /usr/local/bin/
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        wget -q "https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-macos.zip"
        unzip -o minisign-0.12-macos.zip
        sudo cp minisign /usr/local/bin/
    elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
        wget -q "https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-win64.zip"
        unzip -o minisign-0.12-win64.zip
        cp minisign.exe /usr/bin/ 2>/dev/null || cp minisign.exe ./
    fi
fi

echo "Signing version $VERSION..."
echo ""

# Sign each file
for file in "$@"; do
    if [ -f "$file" ]; then
        echo "Signing: $file"
        minisign -S -s "$MINISIGN_KEY" -m "$file" -x "${file}.minisig"
        echo "  -> ${file}.minisig created"
    else
        echo "Warning: File not found: $file"
    fi
done

echo ""
echo "Generating updater.json..."

# Generate updater.json
PUBLIC_KEY=$(cat "$PROJECT_ROOT/src-tauri/minisign/minisign.pub" | grep -v "untrusted comment" | tr -d '\n')

cat > "$PROJECT_ROOT/updater.json" <<EOF
{
  "version": "$VERSION",
  "notes": "Release $VERSION",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platforms": {
    "windows-x64": {
      "signature": "",
      "url": "https://github.com/wq1131173682/antigravity-hub/releases/download/v${VERSION}/Antigravity_Hub_${VERSION}_x64-setup.exe"
    },
    "linux-gnome": {
      "signature": "",
      "url": "https://github.com/wq1131173682/antigravity-hub/releases/download/v${VERSION}/Antigravity_Hub_${VERSION}_amd64.deb"
    },
    "linux-appimage": {
      "signature": "",
      "url": "https://github.com/wq1131173682/antigravity-hub/releases/download/v${VERSION}/Antigravity_Hub_${VERSION}_x86_64.AppImage"
    },
    "darwin": {
      "signature": "",
      "url": "https://github.com/wq1131173682/antigravity-hub/releases/download/v${VERSION}/Antigravity_Hub_${VERSION}_x64.dmg"
    }
  }
}
EOF

echo "updater.json generated at $PROJECT_ROOT/updater.json"
echo ""
echo "Next steps:"
echo "1. Upload all signed files (.exe, .deb, .AppImage, .dmg) and their .minisig files to GitHub Release"
echo "2. Upload updater.json to the same release"
echo "3. Publish the release"
echo ""
echo "Private key is stored in GitHub Secrets: MINISIGN_PRIVKEY"
echo "Public key is stored in GitHub Secrets: MINISIGN_PUBKEY"
