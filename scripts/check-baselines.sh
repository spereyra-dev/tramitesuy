#!/usr/bin/env bash
# S14 task 46 (operations delta "Golden baselines hold through optimization",
# OPT-07/OPT-08, R6): assert that no recorded baseline was lowered anywhere
# in the diff.
#
# Guarded records:
# - tests/search/golden_dataset.yaml  → the golden gate's recorded baselines:
#   Top1 / Top3 must never decrease, and the max_no_result_rate /
#   max_ambiguous_rate ceilings must never rise (either direction weakens
#   the gate). A legitimate re-record (e.g. a deliberate, justified ranking
#   change) must land as an explicit spec amendment, never silently.
# - tests/load/BASELINE.md            → the pre-optimization baseline is
#   recorded evidence (task 4); any change to it must be an explicit,
#   reviewed decision.
#
# Usage: check-baselines.sh [BASE_REF]  (default: origin/master; CI passes
# the PR base sha). Read-only: compares git objects, never the worktree.
set -euo pipefail

BASE_REF="${1:-${BASE_REF:-origin/master}}"
base="$(git merge-base "$BASE_REF" HEAD)"

fail() {
  echo "FAIL(baselines): $*" >&2
  exit 1
}

value_of() { # contents, key → the recorded value
  printf '%s\n' "$1" | awk -v key="$2" '$1 == key ":" { print $2; found=1; exit } END { if (!found) print "" }'
}

GOLDEN=tests/search/golden_dataset.yaml
if ! git diff --quiet "$base" HEAD -- "$GOLDEN"; then
  old=$(git show "$base:$GOLDEN")
  new=$(git show "HEAD:$GOLDEN")
  for key in top1 top3; do
    old_value=$(value_of "$old" "$key")
    new_value=$(value_of "$new" "$key")
    if [ -z "$old_value" ] || [ -z "$new_value" ]; then
      fail "$GOLDEN: baseline '$key' missing in the diffed versions (old='$old_value' new='$new_value')"
    fi
    if awk "BEGIN { exit !($new_value < $old_value) }"; then
      fail "$GOLDEN: recorded baseline '$key' was lowered ($old_value → $new_value); \
no baseline may be lowered — an intended change needs an explicit spec amendment"
    fi
  done
  for key in max_no_result_rate max_ambiguous_rate; do
    old_value=$(value_of "$old" "$key")
    new_value=$(value_of "$new" "$key")
    if [ -z "$old_value" ] || [ -z "$new_value" ]; then
      fail "$GOLDEN: baseline '$key' missing in the diffed versions (old='$old_value' new='$new_value')"
    fi
    if awk "BEGIN { exit !($new_value > $old_value) }"; then
      fail "$GOLDEN: recorded ceiling '$key' was raised ($old_value → $new_value); \
a raised ceiling weakens the golden gate"
    fi
  done
  echo "OK(baselines): $GOLDEN baselines not weakened by this diff"
fi

BASELINE_MD=tests/load/BASELINE.md
if ! git diff --quiet "$base" HEAD -- "$BASELINE_MD"; then
  fail "$BASELINE_MD was modified: the pre-optimization baseline is recorded \
evidence (task 4); any change is an explicit, reviewed decision"
fi
echo "OK(baselines): no recorded baseline was lowered in the diff"
