# Check a FRIDAY download

Each release on GitHub carries, next to the installers:

| File | What it is |
|---|---|
| `SHA256SUMS` | the SHA-256 of every file in the release |
| `SHA256SUMS.sig` | FRIDAY's signature over `SHA256SUMS` (ECDSA P-256, SHA-256) |
| `friday-X.Y.Z.spdx.json` | the software bill of materials: every crate and package FRIDAY is built from, as SPDX |

Check the signature first: it shows `SHA256SUMS` came from FRIDAY's release
workflow and wasn't changed. The sums then show your download is the file that
was released.

## 1. The checksums are FRIDAY's

The public key is in the repository at
[`installer/windows/friday-update-key.pub`](../installer/windows/friday-update-key.pub).
Take it from the release's own tag, then:

```sh
openssl dgst -sha256 -verify friday-update-key.pub -signature SHA256SUMS.sig SHA256SUMS
```

It prints `Verified OK`. Anything else: don't install the download.

## 2. Your download matches

Linux, in the folder with the download and `SHA256SUMS`:

```sh
sha256sum -c SHA256SUMS --ignore-missing
```

Each file you have says `OK`.

Windows, in PowerShell:

```powershell
(Get-FileHash .\FRIDAY-X.Y.Z-x64.msi -Algorithm SHA256).Hash
```

Compare it with that file's line in `SHA256SUMS` (case doesn't matter).

The Windows tray does both checks itself before it installs an update: it
downloads `SHA256SUMS` and `SHA256SUMS.sig`, checks the signature with the key
built into it, and opens the MSI only when its hash matches.

## Packages from FRIDAY's pacman repo

`friday-git` from the `[friday]` repo is checked by pacman: each package and
the repo database are signed with FRIDAY's release key, fingerprint

```
8CC5 1D30 6AAE 9874 EB05  6A81 6954 E96E 5CA1 63C2
```

(also in [`packaging/arch/friday-signing-key.fpr`](../packaging/arch/friday-signing-key.fpr),
and the key itself is installed at `/usr/share/friday/friday-signing-key.asc`).
With `SigLevel = Required DatabaseRequired`, pacman refuses an unsigned or
changed package.

## What FRIDAY is made of

`friday-X.Y.Z.spdx.json` is made by [syft](https://github.com/anchore/syft)
from the tagged source. It is in `SHA256SUMS`, so check it the same way. To
list what's in it, or scan it for known vulnerabilities:

```sh
jq -r '.packages[] | "\(.name) \(.versionInfo)"' friday-X.Y.Z.spdx.json
grype sbom:friday-X.Y.Z.spdx.json
```
