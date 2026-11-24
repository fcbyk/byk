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
  */zsh)  SHELL_RC="$HOME/.zshrc" ;;
  */bash) SHELL_RC="$HOME/.bashrc" ;;
  *)      SHELL_RC="" ;;
esac

if [[ -n "$SHELL_RC" ]]; then
  if grep -qF '.byk/bin' "$SHELL_RC" 2>/dev/null; then
    echo "PATH already configured in ${SHELL_RC}"
  else
    echo "" >> "$SHELL_RC"
    echo "" >> "$SHELL_RC"
    echo "# byk" >> "$SHELL_RC"
    echo "${PATH_LINE}" >> "$SHELL_RC"
    echo "Added PATH to ${SHELL_RC}"
  fi
  echo "Run 'source ${SHELL_RC}' or restart your shell, then:"
  echo "  byk --version"
else
  echo "Could not detect your shell config. Add this to your shell profile:"
  echo "  ${PATH_LINE}"
fi

# 补全不自动写：安装脚本只负责把二进制放好（与 pip 一样），
# 改用户的 rc 由 byk 自己做，能探测、能撤销、能看见落到哪个文件。
echo ""
echo "Shell completion is not enabled. Turn it on with:"
echo "  byk @@@shell add completion"