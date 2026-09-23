#!/usr/bin/env bash
# Runs as an unprivileged user before installing even the Python test runner.
set -euo pipefail
artifact=$1
cd /tmp
set +e
APPIMAGE_EXTRACT_AND_RUN=1 "$artifact" --no-open --non-interactive > first-run.log 2>&1
code=$?
set -e
test "$code" = 1
grep -q GEMINI_API_KEY first-run.log
test -f /test/.env && test -f /test/voices.json
sed -i 's/your_api_key_here/offline-test-key/' /test/.env
set +e
timeout --signal=INT 3 env APPIMAGE_EXTRACT_AND_RUN=1 "$artifact" --no-open --non-interactive > healthy.log 2>&1
code=$?
set -e
test "$code" = 124
grep 'Listening Exam Generator: http://' healthy.log
test -f /test/data/jobs.db
echo 'PASS: first run and initialized server on the bare Ubuntu image (before installing Python).'
mkdir /tmp/portable-extracted
cd /tmp/portable-extracted
"$artifact" --appimage-extract > /tmp/package-inventory.txt
test -f squashfs-root/usr/share/licenses/libssl3.txt
test -f squashfs-root/usr/share/licenses/libgcc-s1.txt
test -f squashfs-root/usr/share/licenses/AppImage-runtime/AppImage-runtime.txt
if find squashfs-root -type f | grep -E '(^|/)(\.env|voices\.json|jobs\.db)|\.(wav|log|rs)$'; then
    echo 'Unexpected configuration, source or development data in AppImage' >&2
    exit 1
fi
LD_LIBRARY_PATH="$PWD/squashfs-root/usr/lib" ldd squashfs-root/usr/bin/server > /tmp/packaged-ldd.txt
if grep 'not found' /tmp/packaged-ldd.txt; then exit 1; fi
set +e
env -u APPIMAGE squashfs-root/AppRun --no-open --non-interactive > /tmp/extracted.log 2>&1
code=$?
set -e
test "$code" = 1
grep -q GEMINI_API_KEY /tmp/extracted.log
test -f .env && test -f voices.json
echo 'PASS: manual extraction, AppDir launch, dependency resolution, licenses and package inventory.'
if [ ! -e /dev/fuse ]; then
    echo 'NOT TESTED: native FUSE mounting; /dev/fuse is unavailable in this container.'
fi
