#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

PATTERNS=(gtk:: gdk:: gsk:: sourceview5:: gdk4_x11 gdk4_wayland crate::ui gtk_test)

violations=0
for pattern in "${PATTERNS[@]}"; do
  while IFS= read -r hit; do
    [[ -z $hit ]] && continue
    echo "VIOLATION [$pattern] $hit" >&2
    violations=1
  done < <(grep -rnF --include='*.rs' -e "$pattern" crates/strata-core/src || [[ $? -eq 1 ]])
done

while IFS= read -r crate; do
  [[ -z $crate ]] && continue
  echo "VIOLATION [cargo tree] $crate" >&2
  violations=1
done < <(cargo tree -p strata-core -e normal --all-features --locked --prefix none \
  | grep -E '^(gtk4|gdk4|gsk4|sourceview5|gdk4-x11|gdk4-wayland) ' || [[ $? -eq 1 ]])

exit "$violations"
