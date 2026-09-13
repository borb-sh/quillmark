#!/bin/bash
set -e
set -o pipefail

# Prints the changelog baseline release-prepare.yml seeds a version section
# from: the first strict semver tag (X.Y.Z, optional `v` prefix) on stdin,
# which the workflow feeds from `git tag -l --sort=-v:refname`. A pre-release
# tag (e.g. v0.85.0-rc.1) fails the pattern, so the commit range reaches back
# to the last final release and covers the RCs cut since it.
#
# Prints nothing and exits 0 when no tag qualifies: before the first release
# there is no baseline, and the caller reads the empty string as "since the
# first commit".

grep -m1 -E '^v?[0-9]+\.[0-9]+\.[0-9]+$' || true
