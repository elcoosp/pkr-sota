#!/usr/bin/env bash
set -euo pipefail

TASK_ID="$1"
if [[ -z "$TASK_ID" ]]; then
    echo "Usage: $0 <task-id> (e.g., W1-T1)"
    exit 1
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# Find task file
shopt -s nullglob
task_files=(docs/tasks/"$TASK_ID"-*.md)
if [[ ${#task_files[@]} -eq 0 ]]; then
    echo "No task file found matching docs/tasks/$TASK_ID-*.md"
    exit 1
fi
TASK_FILE="${task_files[0]}"

# Read task body
TASK_BODY="$(cat "$TASK_FILE")"

# Read root workspace Cargo.toml
CARGO_WORKSPACE="$(cat Cargo.toml)"

# Read contracts crate (always include this as it's the universal API)
CONTRACTS_SRC=""
if [ -f "crates/pkr-contracts/src/lib.rs" ]; then
    CONTRACTS_SRC=$(cat crates/pkr-contracts/src/lib.rs)
fi

# --- Auto-discover inter-crate dependencies ---
TARGET_CRATES=$(grep -oE '(crates|binaries)/(pkr-[a-z-]+)/' "$TASK_FILE" | sort -u | sed 's|/$||')

DEP_CONTEXT=""
for crate_path in $TARGET_CRATES; do
    if [ -f "$crate_path/Cargo.toml" ]; then
        # Find pkr-* dependencies in this crate's Cargo.toml
        deps=$(grep -oE 'pkr-[a-z-]+' "$crate_path/Cargo.toml" | sort -u)

        for dep in $deps; do
            dep_path="crates/$dep"
            if [ -d "$dep_path/src" ]; then
                # Append all .rs files from the dependency crate
                while IFS= read -r -d '' rs_file; do
                    relative_path="${rs_file#"$REPO_ROOT/"}"
                    DEP_CONTEXT+=$'\n\n### Dependency Context: '"$dep"$' ('"$relative_path"$')\n'
                    DEP_CONTENT=$(cat "$rs_file")
                    DEP_CONTEXT+=$'```rust\n'"$DEP_CONTENT"$'\n```'
                done < <(find "$dep_path/src" -name "*.rs" -print0)
            fi
        done
    fi
done
# --- End auto-discover ---

cat <<EOF
You are an expert Rust developer specializing in high-performance poker solvers and Apple Silicon optimization.

Your goal is to implement the task below. You may produce **multiple bash scripts across multiple messages** to complete the task. The user will run your script and paste back the terminal output. If there are errors, you will produce a new bash script in your next message to fix them.

**Output format:**
- Output **exactly one bash script per message** enclosed in a \`\`\`bash code block.
- No text outside the code block (except optional analysis inside \`# comments\` inside the script).
- If the task requires multiple steps, output the first script, and wait for the user to run it and paste the output. Then output the next script.
- When the task is fully complete and tests pass, your final script should create the PR.

## Workspace Context & Contracts
You are working in the \`pkr-sota\` Rust workspace. You must code against the predefined traits and existing implementations.

### Root Cargo.toml
 $CARGO_WORKSPACE

### Universal Contracts (crates/pkr-contracts/src/lib.rs)
\`\`\`rust
 $CONTRACTS_SRC
\`\`\`
 $DEP_CONTEXT

## Task
 $TASK_BODY

## ⚠️ CRITICAL RULES – FOLLOW EXACTLY (Zero Merge Conflict Strategy)

### 1. STRICT FILE BOUNDARIES (Non-negotiable)
- You MUST ONLY create or modify files listed in the "Exclusive File Paths" section of the Task.
- DO NOT touch any other files. Do not add modules to parent \`lib.rs\` files unless explicitly listed in your allowed paths.
- If you need a dependency, assume it exists or add it to your crate's \`Cargo.toml\` (if allowed), but NEVER modify the root \`Cargo.toml\`.

### 2. Never overwrite existing files without reading first
- Before creating or modifying any file, check if it already exists in \`main\`:
  \`git cat-file -e main:relative/path 2>/dev/null\`
- If it exists, **read its current content** with \`git show main:relative/path\`.
- Make only the **necessary changes** – do not rewrite the whole file unless it's the only safe way.
- Prefer \`sed -i '' 's/old/new/g'\` for small changes. If \`sed\` fails, then fallback to a full rewrite using \`cat > file << 'EOF'\`.

### 3. Workspace dependencies must be centralized
- When adding a new dependency to a crate, reference it as \`dep = { workspace = true }\` in that crate's \`Cargo.toml\`.
- DO NOT add new dependencies to the root \`Cargo.toml\`. If a dependency is missing from the workspace, halt and output an error comment in the script.

### 4. Worktree setup (Idempotent, no automatic rebase)
\`\`\`bash
WORKTREE_DIR="../pkr-sota-worktrees/task-$TASK_ID"
BRANCH="task/$TASK_ID"
mkdir -p ../pkr-sota-worktrees
if [ -d "\$WORKTREE_DIR" ]; then
    cd "\$WORKTREE_DIR"
    # DO NOT fetch or rebase – the worktree may have uncommitted changes.
else
    git worktree add -b "\$BRANCH" "\$WORKTREE_DIR" main
    cd "\$WORKTREE_DIR"
fi
\`\`\`

### 5. Reading existing files (always do this)
\`\`\`bash
git show main:relative/path          # to read content
git cat-file -e main:relative/path   # to check existence
\`\`\`

### 6. Testing only affected areas
- Only run tests and lints for the specific crate you are working on to save time.
- **Always use \`cargo nextest run\` instead of \`cargo test\` for faster, more reliable test execution.**
\`\`\`bash
# Example for pkr-core
cargo fmt -p pkr-core
cargo clippy -p pkr-core -- -D warnings
cargo nextest run -p pkr-core
\`\`\`

### 7. Error handling
\`\`\`bash
set -euo pipefail
trap 'echo "ERROR on line \$LINENO"; git checkout -- .; exit 1' ERR
DEBUG=\${DEBUG:-0}; [ "\$DEBUG" = "1" ] && set -x
\`\`\`

### 8. Incremental commits – one logical change per commit
- Commit after successful compilation and tests pass.
- Use a new commit for a new logical change.
- Push after every commit with \`git push origin "\$BRANCH" --force-with-lease\`.

### 9. PR creation (only when task is fully solved)
- Before creating the PR, **explicitly verify all acceptance criteria** from the Task body with \`if\` checks.
- Use the task ID (e.g., W1-T1):
\`\`\`bash
gh pr create --title "feat(core): implement W1-T1 card primitives" --body "Implements task $TASK_ID" --base main
\`\`\`

Now produce the first bash script to start implementing the task. Follow every rule above. No exceptions.
EOF
