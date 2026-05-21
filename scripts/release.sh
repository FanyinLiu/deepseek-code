#!/usr/bin/env bash

set -euo pipefail

VERSION=""
REMOTE="origin"
BRANCH="main"
SKIP_CHECKS=false
DRY_RUN=false

usage() {
  cat <<'EOF'
Usage:
  ./scripts/release.sh [options] <version>

Options:
  --remote <name>      Git remote (default: origin)
  --branch <name>      Git branch (default: main)
  --skip-checks        Skip local verification commands
  --dry-run            Dry-run only, do not commit/tag/push
  -h, --help           Show help

Examples:
  ./scripts/release.sh 0.1.1
  ./scripts/release.sh --remote origin --branch main 0.1.1 --skip-checks
  ./scripts/release.sh 0.1.1 --dry-run
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --remote)
      REMOTE="$2"
      shift 2
      ;;
    --branch)
      BRANCH="$2"
      shift 2
      ;;
    --skip-checks)
      SKIP_CHECKS=true
      shift
      ;;
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      if [[ -n "$VERSION" ]]; then
        echo "Unexpected argument: $1"
        usage
        exit 1
      fi
      VERSION="$1"
      shift
      ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  echo "Error: version is required."
  usage
  exit 1
fi

if [[ ! "$VERSION" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z\.-]+)?$ ]]; then
  echo "Error: invalid version format. Use vX.Y.Z or X.Y.Z"
  exit 1
fi

VERSION_NO_V="${VERSION#v}"
TAG="v$VERSION_NO_V"

run_or_throw() {
  "$@"
}

ensure_unreleased_notes() {
  awk '
    /^## \[Unreleased\]/{in_section=1; next}
    /^## \[/{if (in_section) exit 0}
    in_section && /[^[:space:]]/ {found=1}
    END {exit found ? 0 : 1}
  ' CHANGELOG.md
}

bump_version_in_cargo_toml() {
  local version="$1"
  local tmp_file
  tmp_file="$(mktemp)"

  awk -v new_version="$version" '
    $0 ~ /^[[:space:]]*version[[:space:]]*=/ && !done {
      sub(/"[^"]*"/, "\"" new_version "\"")
      done=1
    }
    {print}
  ' Cargo.toml > "$tmp_file"
  mv "$tmp_file" Cargo.toml
}

bump_version_in_npm_package() {
  local version="$1"
  if [[ -f package.json ]]; then
    npm version "$version" --no-git-tag-version >/dev/null
  fi
}

if [[ "$(git rev-parse --abbrev-ref HEAD)" != "$BRANCH" ]]; then
  echo "Error: release must be run from '$BRANCH' (current: $(git rev-parse --abbrev-ref HEAD))."
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "Error: working tree is not clean."
  exit 1
fi

if [[ "$SKIP_CHECKS" != true ]]; then
  echo "Running pre-release checks..."
  run_or_throw cargo fmt --all --check
  run_or_throw cargo check --all-targets --all-features
  run_or_throw cargo clippy --all-targets --all-features -- -D warnings
  run_or_throw cargo test --all-features
  run_or_throw cargo run -- --help
  run_or_throw cargo run --bin octo -- --help
  run_or_throw cargo run --bin octo -- preview-tui --api ready --scenario welcome --width 80 --height 24
  run_or_throw node scripts/npm-bootstrap.js --dry-run
  run_or_throw npm pack --dry-run
fi

if [[ ! -f CHANGELOG.md ]] || ! grep -q '^## \[Unreleased\]' CHANGELOG.md; then
  echo "Error: CHANGELOG.md is missing or missing [Unreleased] section."
  exit 1
fi
if ! ensure_unreleased_notes; then
  echo "Error: CHANGELOG.md [Unreleased] section has no release notes."
  exit 1
fi

if ! grep -q '^version *= ' Cargo.toml; then
  echo "Error: version field not found in Cargo.toml."
  exit 1
fi

bump_version_in_cargo_toml "$VERSION_NO_V"
bump_version_in_npm_package "$VERSION_NO_V"
cargo generate-lockfile

if [[ "$DRY_RUN" == true ]]; then
  echo "Dry-run mode: version updated locally only. No commit/tag/push performed."
  echo "Re-run without --dry-run to finish release."
  exit 0
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "Error: tag $TAG already exists."
  exit 1
fi

git add Cargo.toml Cargo.lock CHANGELOG.md package.json package-lock.json
if git diff --cached --quiet; then
  echo "Error: no changes were staged after version bump."
  exit 1
fi

git commit -m "chore(release): $TAG"
git tag -a "$TAG" -m "$TAG"
git push "$REMOTE" "$BRANCH"
git push "$REMOTE" "$TAG"

echo "Release prep done. GitHub Actions will publish assets for $TAG."
