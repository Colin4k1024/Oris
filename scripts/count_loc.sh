#!/usr/bin/env bash
# count_loc.sh — 统计项目总代码行数和每位贡献者的提交量
# Usage: ./scripts/count_loc.sh [author]
#   author  Git author name or email pattern (default: all authors)
#
# Example:
#   ./scripts/count_loc.sh                  # full project stats
#   ./scripts/count_loc.sh "Colin4k1024"    # per-author git contribution
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ── 1. Static line counts (current working tree, excluding target/) ──────────

echo "========================================"
echo "  Oris — 项目代码行数统计 (Project LOC)"
echo "========================================"

count_loc() {
  local ext="$1"
  local result
  result=$(find . -name "*.$ext" -not -path "*/target/*" -exec wc -l {} + 2>/dev/null \
             | tail -1 | awk '{print $1}')
  echo "${result:-0}"
}

count_files() {
  local ext="$1"
  find . -name "*.$ext" -not -path "*/target/*" 2>/dev/null | wc -l | tr -d ' '
}

rs_loc=$(count_loc  "rs")
toml_loc=$(count_loc "toml")
md_loc=$(count_loc  "md")

printf "  %-30s %7s 行\n" "Rust 源码 (*.rs)"           "$rs_loc"
printf "  %-30s %7s 行\n" "Cargo / TOML 配置 (*.toml)" "$toml_loc"
printf "  %-30s %7s 行\n" "Markdown 文档 (*.md)"       "$md_loc"

total=$((rs_loc + toml_loc + md_loc))
printf "\n  %-30s %7s 行\n" "合计 (Rust + TOML + Markdown)" "$total"

rs_files=$(count_files "rs")
printf "  %-30s %7s 个\n" "Rust 源文件数量" "$rs_files"

# ── 2. Git contribution stats ────────────────────────────────────────────────

echo ""
echo "========================================"
echo "  Git 提交贡献统计 (Commit Contribution)"
echo "========================================"

if [[ $# -ge 1 ]]; then
  target_author="$1"
  echo "  作者过滤: $target_author"
  echo ""
  git log --all --author="$target_author" --numstat --format="" \
    | awk 'NF==3 {added+=$1; deleted+=$2; files[$3]=1}
           END {
             printf "  %-30s %7d 行\n", "新增行数 (lines added)", added
             printf "  %-30s %7d 行\n", "删除行数 (lines deleted)", deleted
             printf "  %-30s %7d 行\n", "净增行数 (net lines)", added - deleted
             printf "  %-30s %7d 个\n", "涉及文件数 (files touched)", length(files)
           }'
  commit_count=$(git log --all --author="$target_author" --oneline | wc -l | tr -d ' ')
  printf "  %-30s %7s 次\n" "提交次数 (commits)" "$commit_count"
else
  echo "  所有贡献者 (all authors):"
  echo ""
  git log --all --format="%aN" | sort -u | while IFS= read -r author; do
    added=$(git log --all --author="$author" --numstat --format="" \
              | awk 'NF==3 {s+=$1} END {print s+0}')
    deleted=$(git log --all --author="$author" --numstat --format="" \
                | awk 'NF==3 {s+=$2} END {print s+0}')
    commits=$(git log --all --author="$author" --oneline | wc -l | tr -d ' ')
    printf "  %-30s +%-7s -%-7s  %s commits\n" "$author" "$added" "$deleted" "$commits"
  done
fi

echo ""
echo "========================================"
