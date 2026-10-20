#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

CORE=(
  src/model src/services src/adapters src/app src/util
  src/storage.rs src/i18n.rs src/build_info.rs src/logging.rs src/metrics.rs
  src/trusted_command.rs src/rar_extraction.rs
  src/media.rs src/media src/sandbox.rs src/sandbox
  src/sandbox_helper.rs src/sandbox_helper
)
PATTERNS=(gtk:: gdk:: gsk:: sourceview5:: gdk4_x11 gdk4_wayland crate::ui gtk_test)

existing=()
for path in "${CORE[@]}"; do
  [[ -e $path ]] && existing+=("$path")
done

violations=0
for pattern in "${PATTERNS[@]}"; do
  while IFS= read -r hit; do
    [[ -z $hit ]] && continue
    echo "VIOLATION [$pattern] $hit" >&2
    violations=1
  done < <(grep -rnF --include='*.rs' -e "$pattern" "${existing[@]}" || [[ $? -eq 1 ]])
done

exit "$violations"
