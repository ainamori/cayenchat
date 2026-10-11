#!/usr/bin/env bash
# Checks that line 1 of every Rust source file under crates/ is the SPDX
# license identifier of its crate (D042): MPL-2.0 for the core crates,
# GPL-3.0-only for `ui`, and Apache-2.0 for crates/ui/src/input.rs, which is
# adapted from GPUI. vendor/ keeps its upstream licenses.
set -euo pipefail

cd "$(dirname "$0")/.."
fail=0

while IFS= read -r -d '' file; do
  case $file in
    crates/ui/src/input.rs) want=Apache-2.0 ;;
    crates/ui/*) want=GPL-3.0-only ;;
    *) want=MPL-2.0 ;;
  esac
  first=$(head -n1 "$file")
  if [[ $first != "// SPDX-License-Identifier: $want" ]]; then
    echo "$file: line 1 must be '// SPDX-License-Identifier: $want'"
    fail=1
  fi
done < <(find crates -name '*.rs' -not -path '*/target/*' -print0 | sort -z)

exit $fail
