#!/usr/bin/env bash
set -euo pipefail

repo_url="https://github.com/Colin4k1024/Oris"
stale_repo_pattern='https://github\.com/fanjia1024/oris|https://github\.com/fanjia1024/Oris'
manifest="crates/oris-runtime/Cargo.toml"

if ! grep -q "^repository = \"$repo_url\"$" "$manifest"; then
  echo "Expected $manifest repository to be $repo_url"
  exit 1
fi

if ! grep -q "^homepage = \"$repo_url\"$" "$manifest"; then
  echo "Expected $manifest homepage to be $repo_url"
  exit 1
fi

if ! grep -q '^rust-version = "' "$manifest"; then
  echo "Expected $manifest to declare rust-version"
  exit 1
fi

release_files=(
  "README.md"
  "crates/oris-runtime/src/lib.rs"
  "docs/open-source-onboarding-zh.md"
  "examples/templates/README.md"
  "skills/oris-maintainer/references/release-notes.md"
)

while IFS= read -r release_note; do
  release_files+=("$release_note")
done < <(find . -maxdepth 1 -name 'RELEASE_v*.md' -print | sort)

if rg -n "$stale_repo_pattern" "${release_files[@]}"; then
  echo "Found stale GitHub URLs in release-facing metadata or docs"
  exit 1
fi

echo "Release metadata and public links look consistent."
