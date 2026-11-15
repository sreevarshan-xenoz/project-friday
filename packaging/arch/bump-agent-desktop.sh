#!/usr/bin/env bash
# Check a new agent-desktop release and, if it passes, move the pin in the
# PKGBUILD to it. Safe to run anywhere with curl, jq, npm, tar and sha256sum.
#
#   bash packaging/arch/bump-agent-desktop.sh                 # the latest release
#   bash packaging/arch/bump-agent-desktop.sh --version 1.4.0
#   bash packaging/arch/bump-agent-desktop.sh --pkgbuild FILE # edit another copy
#
# What it checks before it changes anything:
#   1. the version is newer than the pin and has the same major version;
#   2. the release tarball matches the release's own checksums.txt;
#   3. the Linux binary and its helper are byte-identical to the ones in the
#      npm package, and npm verifies that package's registry signature and
#      its build provenance (attestation): two channels, one of them signed;
#   4. the binary runs, speaks the output format FRIDAY reads (schema 2.x),
#      and still has every subcommand and flag FRIDAY's engine passes it.
# Exit status: 0 nothing to do, 10 the PKGBUILD was changed, 3 a new major
# version (needs a person), 1 a check failed. A markdown summary goes to
# $SUMMARY_FILE when it is set.
set -euo pipefail

repo="vercel-labs/agent-desktop"
here=$(cd "$(dirname "$0")" && pwd)
pkgbuild="$here/PKGBUILD"
want=""
while [ $# -gt 0 ]; do
  case "$1" in
    --version) want="${2:?--version needs a value}"; shift 2 ;;
    --pkgbuild) pkgbuild="${2:?--pkgbuild needs a file}"; shift 2 ;;
    *) echo "usage: $0 [--version X.Y.Z] [--pkgbuild FILE]" >&2; exit 2 ;;
  esac
done
summary() { [ -z "${SUMMARY_FILE:-}" ] || printf '%s\n' "$*" >>"$SUMMARY_FILE"; }
say() { printf '%s\n' "$*"; summary "$*"; }
fail() { say "FAILED: $*"; exit 1; }
semver() { [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; }
newer() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n1)" = "$1" ] && [ "$1" != "$2" ]; }

current=$(sed -n 's/^_ad_ver=\(.*\)$/\1/p' "$pkgbuild" | head -n1)
semver "$current" || fail "can't read the pinned version from $pkgbuild"

if [ -z "$want" ]; then
  tag=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" | jq -r '.tag_name // empty') \
    || fail "couldn't ask GitHub for the latest release"
  want="${tag#v}"
fi
semver "$want" || fail "\"$want\" is not a release version (X.Y.Z)"

summary "### agent-desktop $current -> $want"
if ! newer "$want" "$current"; then say "Pinned $current is current (latest is $want). Nothing to do."; exit 0; fi
if [ "${want%%.*}" != "${current%%.*}" ]; then
  say "$want is a new major version (pinned $current). FRIDAY's engine reads schema 2.x of 1.x releases: a person must review it."
  exit 3
fi

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
base="https://github.com/$repo/releases/download/v$want"
tar_name="agent-desktop-v$want-x86_64-unknown-linux-gnu.tar.gz"
curl -fsSL "$base/$tar_name" -o "$work/$tar_name" || fail "couldn't download $tar_name"
curl -fsSL "$base/checksums.txt" -o "$work/checksums.txt" || fail "couldn't download checksums.txt"
curl -fsSL "https://raw.githubusercontent.com/$repo/v$want/LICENSE" -o "$work/LICENSE" || fail "couldn't download LICENSE"

# 2. the release's own checksum
listed=$(awk -v f="$tar_name" '$2 == f { print $1 }' "$work/checksums.txt")
actual=$(sha256sum "$work/$tar_name" | awk '{print $1}')
[ -n "$listed" ] || fail "checksums.txt doesn't list $tar_name"
[ "$listed" = "$actual" ] || fail "the tarball doesn't match checksums.txt"
say "- release tarball matches checksums.txt ($actual)"

# 3. npm: same bytes, signed registry package, verified provenance
mkdir "$work/rel" "$work/npm"
tar -xzf "$work/$tar_name" -C "$work/rel"
(cd "$work/npm" && npm pack "agent-desktop@$want" >/dev/null 2>&1) || fail "couldn't fetch agent-desktop@$want from npm"
tar -xzf "$work"/npm/agent-desktop-"$want".tgz -C "$work/npm"
cmp -s "$work/rel/agent-desktop" "$work/npm/package/bin/agent-desktop-linux-x64" \
  || fail "the GitHub release binary is NOT the npm package's binary"
cmp -s "$work/rel/agent-desktop-linux-clipboard-owner" "$work/npm/package/bin/agent-desktop-linux-clipboard-owner-linux-x64" \
  || fail "the GitHub release helper is NOT the npm package's helper"
say "- the binary and helper are byte-identical to the npm package's"
mkdir "$work/inst"
audit=$(cd "$work/inst" && npm init -y >/dev/null 2>&1 && npm install --ignore-scripts --no-audit --no-fund "agent-desktop@$want" >/dev/null 2>&1 && npm audit signatures 2>&1) \
  || fail "npm could not verify the package's signatures: $audit"
grep -q "verified registry signature" <<<"$audit" || fail "npm did not verify the registry signature"
grep -q "verified attestation" <<<"$audit" || fail "npm found no verified build provenance (attestation)"
say "- npm: verified registry signature and verified build provenance"

# 4. it runs, and it still has what FRIDAY's engine uses
bin="$work/rel/agent-desktop"
ver=$("$bin" version 2>/dev/null) || fail "the binary did not run"
[ "$(jq -r '.ok' <<<"$ver")" = "true" ] || fail "'version' did not succeed"
case "$(jq -r '.version' <<<"$ver")" in 2.*) ;; *) fail "output format $(jq -r '.version' <<<"$ver") is not the 2.x FRIDAY reads" ;; esac
[ "$(jq -r '.data.version' <<<"$ver")" = "$want" ] || fail "the binary says it is $(jq -r '.data.version' <<<"$ver"), not $want"
need() { # need <subcommand> <flag>...
  local sub=$1; shift; local help; help=$("$bin" "$sub" --help 2>&1) || fail "'$sub' is gone"
  local f; for f in "$@"; do grep -q -- "$f" <<<"$help" || fail "'$sub' no longer has $f"; done
}
need list-apps --app
need list-windows
need snapshot --app --window-id --interactive-only --compact
need find --app --name --role --exact --limit --window-id
need get --property
need click
need type --stdin
say "- it runs, speaks output format 2.x, and has every subcommand and flag FRIDAY's engine uses"

new_license=$(sha256sum "$work/LICENSE" | awk '{print $1}')
old_license=$(sed -n 's/^_ad_sha256_license=\(.*\)$/\1/p' "$pkgbuild" | head -n1)
if [ "$new_license" != "$old_license" ]; then say "- NOTE: the licence text changed; read it before merging"; fi

sed -i "s/^_ad_ver=.*/_ad_ver=$want/; s/^_ad_sha256_tar=.*/_ad_sha256_tar=$actual/; s/^_ad_sha256_license=.*/_ad_sha256_license=$new_license/" "$pkgbuild"
say "Pin moved to $want in $(basename "$pkgbuild"). CI's package build and smoke test are the last gate."
exit 10
