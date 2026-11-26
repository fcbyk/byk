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
# Completion block markers live in src/filecmd/special/shell/rc.rs: the two comment
# lines carry "byk cli completions", the `if ... fi` line carries
# "byk [__api__ ]completion". That pattern also covers the bare line older
# installers wrote.
COMPLETION_RE='byk (cli completions|(__api__ )?completion)'

for RC in "${HOME}/.zshrc" "${HOME}/.bashrc"; do
  if [[ -f "$RC" ]]; then
    if grep -qE "$COMPLETION_RE" "$RC" 2>/dev/null; then
      echo "Removing byk completion from ${RC} ..."
      if [[ "$(uname -s)" == "Darwin" ]]; then
        sed -i '' -E "/${COMPLETION_RE}/d" "$RC"
      else
        sed -i -E "/${COMPLETION_RE}/d" "$RC"
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
