#!/bin/bash
set -e
set -o pipefail

# Prints the version release-prepare.yml bumps the workspace to. CURRENT is the
# version cargo metadata reports; BUMP, RELEASE_CANDIDATE ("true" or "false")
# and VERSION_OVERRIDE ("" when unset) are that workflow's dispatch inputs,
# verbatim. The arithmetic lives here rather than inline in the workflow so
# release-prepare.test.sh can cover every branch.

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
    echo "Usage: $0 CURRENT BUMP RELEASE_CANDIDATE [VERSION_OVERRIDE]" >&2
    exit 2
fi

CURRENT="$1"
BUMP="$2"
RC="$3"
OVERRIDE="${4:-}"

if [[ -n "$OVERRIDE" ]]; then
    # Explicit target (e.g. skip a minor): used verbatim; the checkbox
    # still decides whether to cut it as a fresh -rc.1.
    NEXT="$OVERRIDE"
    if [[ "$RC" == "true" && "$NEXT" != *-rc.* ]]; then
        NEXT="${NEXT}-rc.1"
    fi
elif [[ "$CURRENT" =~ ^([0-9]+\.[0-9]+\.[0-9]+)-rc\.([0-9]+)$ ]]; then
    # Already an RC: the target base version is fixed, so `bump` is
    # ignored. The checkbox decides whether to iterate or promote.
    RC_BASE="${BASH_REMATCH[1]}"
    RC_NUM="${BASH_REMATCH[2]}"
    if [[ "$RC" == "true" ]]; then
        NEXT="${RC_BASE}-rc.$((RC_NUM + 1))"
    else
        NEXT="${RC_BASE}"
    fi
else
    # Not an RC: apply the chosen bump, optionally as a fresh -rc.1.
    IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"
    case "$BUMP" in
        minor) NEXT="${MAJOR}.$((MINOR + 1)).0" ;;
        patch) NEXT="${MAJOR}.${MINOR}.$((PATCH + 1))" ;;
    esac
    if [[ "$RC" == "true" ]]; then
        NEXT="${NEXT}-rc.1"
    fi
fi

# Every route to no version at all lands here: a bump outside the two the
# `case` names, and a CURRENT none of the branches can shape — a
# `0.93.1-beta.1` an earlier override left behind, whose arithmetic fails to
# stderr without failing the script. What this prints is interpolated into the
# tag, the release branch name and the changelog heading.
if ! [[ "$NEXT" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
    echo "ERROR: '$CURRENT' with bump '$BUMP' and override '$OVERRIDE' yields '$NEXT'." >&2
    exit 1
fi

echo "$NEXT"
