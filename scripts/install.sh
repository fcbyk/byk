#!/usr/bin/env bash
set -euo pipefail

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)   TARGET="darwin-arm64" ;;
  Darwin-x86_64)  TARGET="darwin-x86_64" ;;
  Linux-x86_64)   TARGET="linux-x86_64" ;;
  Linux-aarch64)  TARGET="linux-arm64" ;;
  *)              echo "Unsupported platform" >&2; exit 1 ;;
esac

URL="https://github.com/fcbyk/byk/releases/latest/download/byk-${TARGET}.tar.gz"

echo "Installing byk..."
echo "Detected platform: ${TARGET}"
echo "Downloading from ${URL}..."

curl -fsSL "${URL}" -o "${TMP_DIR}/byk.tar.gz"

echo "Extracting..."
tar xzf "${TMP_DIR}/byk.tar.gz" -C "${TMP_DIR}"

INSTALL_DIR="${HOME}/.byk/bin"
echo "Installing to ${INSTALL_DIR}/byk..."

mkdir -p "${INSTALL_DIR}"
cp "${TMP_DIR}/byk" "${INSTALL_DIR}/byk"
chmod +x "${INSTALL_DIR}/byk"

echo ""
echo "byk installed successfully!"
echo ""

# Auto-add to shell config
PATH_LINE='export PATH="$HOME/.byk/bin:$PATH"'

# Determine shell config based on current shell, not file existence
case "${SHELL-}" in
  */zsh)
    SHELL_RC="$HOME/.zshrc"
    COMPLETION_LINE='if command -v byk >/dev/null 2>&1; then source <(byk completion zsh); fi'
    ;;
  */bash)
    SHELL_RC="$HOME/.bashrc"
    COMPLETION_LINE='if command -v byk >/dev/null 2>&1; then source <(byk completion bash); fi'
    ;;
  *)
    SHELL_RC=""
    ;;
esac

if [[ -n "$SHELL_RC" ]]; then
  if grep -qF '.byk/bin' "$SHELL_RC" 2>/dev/null; then
    echo "PATH already configured in ${SHELL_RC}"
  else
    echo "" >> "$SHELL_RC"
    echo "" >> "$SHELL_RC"
    echo "# byk" >> "$SHELL_RC"
    echo "${PATH_LINE}" >> "$SHELL_RC"
    echo "${COMPLETION_LINE}" >> "$SHELL_RC"
    echo "Added PATH and completion to ${SHELL_RC}"
  fi
  echo "Run 'source ${SHELL_RC}' or restart your shell, then:"
  echo "  byk --version"
else
  echo "Could not detect your shell config. Add this to your shell profile:"
  echo "  ${PATH_LINE}"
  echo "  ${COMPLETION_LINE:-# completion not available for your shell}"
fi