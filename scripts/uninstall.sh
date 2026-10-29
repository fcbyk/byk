#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="${HOME}/.byk/bin"
REMOVED=false

# Remove ~/.byk
if [[ -d "${HOME}/.byk" ]]; then
  echo "Removing ${HOME}/.byk ..."
  rm -rf "${HOME}/.byk"
  REMOVED=true
else
  echo "${HOME}/.byk not found, skip."
fi

# Remove PATH and completion entries from shell configs
for RC in "${HOME}/.zshrc" "${HOME}/.bashrc"; do
  if [[ -f "$RC" ]]; then
    if grep -qE 'byk (__api__ )?completion' "$RC" 2>/dev/null; then
      echo "Removing byk completion from ${RC} ..."
      if [[ "$(uname -s)" == "Darwin" ]]; then
        sed -i '' -E '/byk (__api__ )?completion/d' "$RC"
      else
        sed -i -E '/byk (__api__ )?completion/d' "$RC"
      fi
    fi

    if grep -qF '.byk/bin' "$RC" 2>/dev/null; then
      echo "Removing byk PATH from ${RC} ..."
      if [[ "$(uname -s)" == "Darwin" ]]; then
        sed -i '' '/# byk/,/export PATH.*\.byk\/bin/d' "$RC"
      else
        sed -i '/# byk/,/export PATH.*\.byk\/bin/d' "$RC"
      fi
    fi
  fi
done

if $REMOVED; then
  echo ""
  echo "byk uninstalled successfully."
else
  echo "byk was not installed."
fi
