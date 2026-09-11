#!/bin/bash
set -e
set -o pipefail

# Covers the two pieces of release-prepare.yml that only run when someone cuts
# a release: next-version.sh across every branch of the override / existing-RC
# / bump matrix, and last-release-tag.sh's skip of pre-release tags. ci.yml
# runs this on every pull request so a defect in either surfaces long before a
# release needs the path.

here="$(cd "$(dirname "$0")" && pwd)"
status=0
cases=0

expect_next() {
    local want="$1" current="$2" bump="$3" rc="$4" override="${5:-}"
    local got code=0
    cases=$((cases + 1))
    got=$("$here/next-version.sh" "$current" "$bump" "$rc" "$override") || code=$?
    if [ "$code" -ne 0 ]; then
        echo "FAIL: next-version.sh '$current' '$bump' '$rc' '$override' exited $code" >&2
        status=1
    elif [ "$got" != "$want" ]; then
        echo "FAIL: next-version.sh '$current' '$bump' '$rc' '$override' → '$got', want '$want'" >&2
        status=1
    fi
}

expect_refusal() {
    local current="$1" bump="$2" rc="$3" override="${4:-}"
    local got code=0
    cases=$((cases + 1))
    got=$("$here/next-version.sh" "$current" "$bump" "$rc" "$override" 2>/dev/null) || code=$?
    if [ "$code" -eq 0 ]; then
        echo "FAIL: next-version.sh '$current' '$bump' '$rc' '$override' → '$got', want a refusal" >&2
        status=1
    fi
}

expect_baseline() {
    local want="$1" tags="$2"
    local got code=0
    cases=$((cases + 1))
    got=$(printf '%s' "$tags" | "$here/last-release-tag.sh") || code=$?
    if [ "$code" -ne 0 ]; then
        echo "FAIL: last-release-tag.sh exited $code on [${tags//$'\n'/, }]" >&2
        status=1
    elif [ "$got" != "$want" ]; then
        echo "FAIL: last-release-tag.sh [${tags//$'\n'/, }] → '$got', want '$want'" >&2
        status=1
    fi
}

# An override is the target, whatever `bump` says; the checkbox only decides
# whether it is cut as a fresh -rc.1, and an override already carrying one is
# left alone rather than suffixed twice.
expect_next 0.94.0        0.93.1 patch false 0.94.0
expect_next 0.94.0-rc.1   0.93.1 patch true  0.94.0
expect_next 0.94.0-rc.3   0.93.1 patch true  0.94.0-rc.3
expect_next 0.94.0-rc.3   0.93.1 patch false 0.94.0-rc.3

# Iterating an existing RC keeps the base version and counts, rather than
# concatenating, so rc.9 is followed by rc.10.
expect_next 0.89.1-rc.2   0.89.1-rc.1 patch true
expect_next 0.89.1-rc.10  0.89.1-rc.9 patch true

# Promotion drops the suffix at whatever N the RC reached, and `bump` is
# ignored: the base version was fixed when the RC was cut.
expect_next 0.89.1        0.89.1-rc.1 patch false
expect_next 0.89.1        0.89.1-rc.4 minor false

# A plain bump counts the field it names and zeroes what sits below it.
expect_next 0.93.10       0.93.9 patch false
expect_next 0.94.0        0.93.1 minor false
expect_next 0.93.2-rc.1   0.93.1 patch true
expect_next 0.94.0-rc.1   0.93.1 minor true

# A version it cannot compute is a refusal, never an empty string: the caller
# interpolates what this prints into the tag, the branch name and the changelog
# heading, where an empty version reads as a release named nothing.
expect_refusal 0.93.1 major false
expect_refusal 0.93.1-beta.1 patch false
expect_refusal 0.93.1-beta.1 patch true

# The changelog baseline is the newest FINAL release in the list, so a
# pre-release sorted above it is passed over. Tags that are not strict semver
# belong to something else and never qualify.
expect_baseline v0.93.1 'v0.94.0-rc.2
v0.93.1
v0.93.0'
expect_baseline v0.93.1 'quillmark-v9.9.9
v1.2
1.2.3.4
v0.93.1'
expect_baseline 1.2.3 '1.2.3-rc.1
1.2.3'

# No baseline is a state, not an error: the caller seeds from the first commit.
expect_baseline "" ""
expect_baseline "" 'v0.94.0-rc.2
v0.94.0-rc.1'

if [ "$status" -ne 0 ]; then
    exit 1
fi
echo "OK: $cases release-prepare version cases"
