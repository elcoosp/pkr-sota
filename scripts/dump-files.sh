#!/bin/bash
# Usage: ./dump-for-llm.sh [-i ignored_dir ...] <directory>

set -euo pipefail

ignore_dirs=()

while getopts "i:" opt; do
    case "$opt" in
        i) ignore_dirs+=("$OPTARG") ;;
        *) echo "Usage: $0 [-i ignored_dir ...] <directory>" >&2; exit 1 ;;
    esac
done
shift $((OPTIND-1))

if [ $# -ne 1 ]; then
    echo "Usage: $0 [-i ignored_dir ...] <directory>" >&2
    exit 1
fi

target_dir="$1"
if [ ! -d "$target_dir" ]; then
    echo "Error: '$target_dir' is not a directory." >&2
    exit 1
fi

cd "$target_dir" || exit 1

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "Error: '$target_dir' is not inside a Git repository." >&2
    exit 1
fi

# Normalize ignored directories – only if the array is non‑empty
if [ ${#ignore_dirs[@]} -gt 0 ]; then
    for i in "${!ignore_dirs[@]}"; do
        ignore_dirs[i]="${ignore_dirs[i]%/}"
    done
fi

git ls-files --cached --others --exclude-standard -z | while IFS= read -r -d '' file; do
    [ -f "$file" ] || continue

    # Skip file if it lies inside an ignored directory – only if we have any
    if [ ${#ignore_dirs[@]} -gt 0 ]; then
        skip=0
        for ignored in "${ignore_dirs[@]}"; do
            if [[ "$file" == "$ignored" || "$file" == "$ignored/"* ]]; then
                skip=1
                break
            fi
        done
        [ "$skip" -eq 1 ] && continue
    fi

    # Always skip any file under a directory named "tests" (anywhere in the path)
    if [[ "$file" =~ (^|/)tests/ ]]; then
        continue
    fi

    # ---- NEW: Skip any file named Cargo.lock ----
    if [[ "$(basename "$file")" == "Cargo.lock" ]]; then
        continue
    fi

    mime_type=$(file -b --mime-type "$file" 2>/dev/null || echo "")
    if [[ ! "$mime_type" =~ ^text/ ]] && [[ "$mime_type" != "application/json" ]] && [[ "$mime_type" != "application/xml" ]]; then
        continue
    fi

    echo "===== FILE: $file ====="
    cat "$file"
    echo
done
