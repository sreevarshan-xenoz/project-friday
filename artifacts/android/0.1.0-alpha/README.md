# Reproducible Android release — 0.1.0-alpha

Every Demo (001, 002, 010…) must be tied to an exact, checksummable binary so the
engineering record is reproducible. This folder holds one released artifact.

## Layout
```
0.1.0-alpha/
   app-debug.apk     # the exact binary shown in the demo (not in git: rebuild it and check sha256.txt)
   sha256.txt        # sha256 of app-debug.apk (verify before install)
   build-info.json   # provenance: git sha, timestamp, toolchain
   README.md         # this file
```

## Produce a new artifact
From `clients/android/`:

```powershell
$env:JAVA_HOME = "C:\Program Files\Eclipse Adoptium\jdk-17.0.17.10-hotspot"
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
.\gradlew assembleDebug
```

Then run the helper to stamp provenance next to the APK:

```powershell
pwsh artifacts/android/build-debug-artifact.ps1 -Version 0.1.0-alpha
```

It copies `app-debug.apk` here, writes `sha256.txt` and `build-info.json`
(git sha, UTC timestamp, Gradle + JDK versions), and prints the sha for paste
into the demo log.

> Release (`assembleRelease`) is **not** configured yet (no signing key). Debug
> artifacts are sufficient for Alpha Integration Week demos; cut a signed release
> before Beta.

## Verify before install
```powershell
certutil -hashfile app-debug.apk SHA256   # must match sha256.txt
```
