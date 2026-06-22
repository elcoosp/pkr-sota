#!/bin/bash

# Usage: ./script.sh [target_directory]
# Default to current directory if none given
TARGET_DIR="${1:-.}"

# Find all regular files, skipping specific directories and files
find "$TARGET_DIR" -type d \( -name node_modules -o -name target -o -name dist -o -name .git \) -prune \
    -o -name pnpm-lock.yaml -prune \
    -o -name Cargo.lock -prune \
    -o -type f -print0 | while IFS= read -r -d '' file; do

    # Optional: print a separator with the filename (comment out if unwanted)
    echo "=== $file ==="
    cat "$file"
    echo  # Add a newline between files
done
