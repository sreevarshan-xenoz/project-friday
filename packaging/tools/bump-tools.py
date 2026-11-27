#!/usr/bin/env python3
"""Weekly check for the external tools FRIDAY pins (packaging/tools/tools.json).

For each tool: find its newest release on GitHub, and if it is newer than
the pin and in the same series (same major version; for 0.x, the same minor),
download every pinned asset, record its sha256, and smoke-test the Linux
build (its --version names the new version, and its smoke command prints
what it should). Only then is the pin moved in tools.json.

Exit 0: nothing new. 10: pins moved (open a pull request). 3: a new series
needs a person. 1: a check failed (treat as a possible supply-chain problem).
A summary goes to $SUMMARY_FILE as Markdown.
"""

import hashlib
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from typing import Any

ROOT = os.path.dirname(os.path.abspath(__file__))
MANIFEST = os.environ.get("TOOLS_JSON", os.path.join(ROOT, "tools.json"))
summary: list[str] = []


def get(url: str, accept: str = "application/vnd.github+json") -> bytes:
    req = urllib.request.Request(url, headers={"Accept": accept, "User-Agent": "friday-tools-bump"})
    tok = os.environ.get("GH_TOKEN")
    if tok and url.startswith("https://api.github.com/"):
        req.add_header("Authorization", f"Bearer {tok}")
    with urllib.request.urlopen(req, timeout=120) as r:
        data: bytes = r.read()
        return data


def ver(s: str) -> tuple[int, ...]:
    return tuple(int(x) for x in s.lstrip("v").split("-")[0].split("."))


def same_series(old: str, new: str) -> bool:
    o, n = ver(old), ver(new)
    return o[:2] == n[:2] if o[0] == 0 else o[0] == n[0]


def smoke(tool: dict[str, Any], name: str, path: str, version: str) -> None:
    with tempfile.TemporaryDirectory() as d:
        with tarfile.open(path) as t:
            t.extractall(d, filter="data")
        exe = os.path.join(d, name)
        if not os.path.exists(exe):  # the program in the archive's one folder
            for sub in os.listdir(d):
                if os.path.isfile(os.path.join(d, sub, name)):
                    exe = os.path.join(d, sub, name)
        # The new build runs with nothing secret in reach (CI also runs this
        # in a job with a read-only token and no saved credentials).
        env = {
            k: v
            for k, v in os.environ.items()
            if "TOKEN" not in k and not k.startswith(("GITHUB_", "ACTIONS_"))
        }
        out = subprocess.run(
            [exe, "--version"], capture_output=True, text=True, timeout=60, env=env
        ).stdout
        if version not in out:
            raise RuntimeError(f"--version says {out.strip()!r}, not {version}")
        s = tool.get("smoke")
        if s:
            r = subprocess.run(
                [exe] + s["args"], capture_output=True, text=True, timeout=120, env=env
            )
            if s["expect"] not in r.stdout:
                raise RuntimeError(f"smoke test: {s['expect']!r} not in its output")


def main() -> int:
    tools = json.load(open(MANIFEST))
    moved, rc = [], 0
    for name, tool in tools.items():
        if os.environ.get("LATEST_TAGS"):  # tests: {"owner/repo": "vX.Y.Z"}
            new = json.load(open(os.environ["LATEST_TAGS"]))[tool["repo"]].lstrip("v")
        else:
            rel = json.loads(get(f"https://api.github.com/repos/{tool['repo']}/releases/latest"))
            new = rel["tag_name"].lstrip("v")
        if ver(new) <= ver(tool["version"]):
            summary.append(f"- {name}: {tool['version']} is the newest.")
            continue
        if not same_series(tool["version"], new):
            summary.append(
                f"- {name}: {new} is a new series (pinned {tool['version']}); a person decides."
            )
            rc = max(rc, 3)
            continue
        try:
            with tempfile.TemporaryDirectory() as d:
                sums = {}
                for plat, a in tool["assets"].items():
                    asset = a["name"].replace("{version}", new)
                    url = f"https://github.com/{tool['repo']}/releases/download/v{new}/{asset}"
                    data = get(url, "application/octet-stream")
                    sums[plat] = hashlib.sha256(data).hexdigest()
                    with open(os.path.join(d, asset), "wb") as f:
                        f.write(data)
                linux = tool["assets"].get("linux-x86_64")
                if linux:
                    smoke(tool, name, os.path.join(d, linux["name"].replace("{version}", new)), new)
            for plat, h in sums.items():
                tool["assets"][plat]["sha256"] = h
            summary.append(
                f"- {name}: {tool['version']} → {new}: assets downloaded, "
                "checksums recorded, Linux build smoke-tested."
            )
            tool["version"] = new
            moved.append(name)
        except Exception as e:  # noqa: BLE001
            summary.append(f"- {name}: {new} failed its check: {e}")
            return finish(1)
    if moved:
        with open(MANIFEST, "w") as f:
            f.write(json.dumps(tools, indent=2) + "\n")
        return finish(10 if rc == 0 else rc)
    return finish(rc)


def finish(rc: int) -> int:
    text = "\n".join(summary)
    print(text)
    if os.environ.get("SUMMARY_FILE"):
        with open(os.environ["SUMMARY_FILE"], "w") as f:
            f.write(text + "\n")
    return rc


if __name__ == "__main__":
    sys.exit(main())
