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

# "<file-prefix> <pattern>"; each entry is removed by P3.
ALLOW=(
  "src/util/mod.rs gtk::"
  "src/util/mod.rs crate::ui"
  "src/util/tests.rs gtk_test"
  "src/adapters/local_preview crate::ui"
  "src/adapters/local_preview gtk_test"
  "src/adapters/local_operations/tests/merge.rs gtk_test"
  "src/adapters/local_operations/tests/deletion.rs gtk_test"
  "src/adapters/local_operations/tests/undo.rs gtk_test"
  "src/adapters/volume/tests.rs gtk_test"
  "src/services/diagnostics.rs sourceview5::"
)

existing=()
for path in "${CORE[@]}"; do
  [[ -e $path ]] && existing+=("$path")
done

violations=0
for pattern in "${PATTERNS[@]}"; do
  while IFS= read -r hit; do
    [[ -z $hit ]] && continue
    file=${hit%%:*}
    allowed=0
    for entry in "${ALLOW[@]}"; do
      read -r prefix allowed_pattern <<<"$entry"
      if [[ $file == "$prefix"* && $pattern == "$allowed_pattern" ]]; then
        allowed=1
      fi
    done
    if ((allowed)); then
      echo "allowed   [$pattern] $hit"
    else
      echo "VIOLATION [$pattern] $hit" >&2
      violations=1
    fi
  done < <(grep -rnF --include='*.rs' -e "$pattern" "${existing[@]}" || [[ $? -eq 1 ]])
done

exit "$violations"
