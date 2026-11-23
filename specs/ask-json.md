# `friday ask --json`: asking FRIDAY from a script

The stable way for a script to ask FRIDAY a question and read the answer.
The output and exit codes below are a contract: they change only with a
new major version.

```sh
friday ask --json "What does the Rust error E0308 mean?"    # Linux
friday-ctl goal --json "What does the Rust error E0308 mean?" # either OS
```

`friday ask --json` takes `--web` (search the web, cite sources) and `--new`
(start a new conversation), before the question. It never opens a window,
shows a notification, or reads the screen or the selection. Those need a
person, so `--screen`, `--selection`, `--region` and `--follow-up` with `--json` exit 2.
On Windows, call `friday-ctl.exe` (in FRIDAY's install folder) with the same
arguments; `--new` is `--new-conversation` there.

## Output

One JSON object on one line, on stdout, when the ask ends (`--stream` is
ignored with `--json`). Nothing else goes to stdout. Messages for people go
to stderr. Stdout stays empty only on a usage error (exit 2).

| Field | Always | Meaning |
|---|---|---|
| `outcome` | yes | `completed`, `failed`, `rejected`, `timed_out` or `unreachable` |
| `result` | yes | the answer (a string) when completed, else `null` |
| `error` | yes | why it didn't complete, in plain words, else `null` |
| `needs_confirmation` | no | the action FRIDAY wants a yes for (`tool`, `action`) |
| `served_by` | no | which model answered (`provider`, `model`, `ms`) |
| `used` | no | what FRIDAY drew on beyond the question (labels) |
| `masked` | no | how many key-like secrets were masked before sending |

New optional fields may be added. Scripts should ignore fields they don't know.

## Exit codes

| Code | `outcome` | Meaning |
|---|---|---|
| 0 | `completed` | answered; the answer is `result` |
| 1 | `failed` or `rejected` | FRIDAY tried and couldn't (no model reachable, the action failed), or refused the request |
| 2 | (none) | usage: no question, a flag `--json` doesn't take, or `--json` on anything but `goal` |
| 3 | `failed` | it needs a confirmation first (`needs_confirmation`); FRIDAY did nothing |
| 4 | `timed_out` | no answer within `FRIDAY_CTL_WAIT_SECS` seconds (default 180): the ask may still finish, or FRIDAY stopped while answering |
| 5 | `unreachable` | FRIDAY isn't running, friday-ctl couldn't reach it (or it didn't say it took the ask within 10 s), or friday-ctl isn't installed |

## Example

```sh
if out=$(friday ask --json "Summarise: $(cat notes.txt)"); then
  jq -r .result <<<"$out"
else
  echo "FRIDAY: $(jq -r .error <<<"$out")" >&2
fi
```

Covered by `tests/e2e/ask.sh` ("for scripts (--json)") and
`iso/friday/test/checks/cli.sh` ("friday ask --json").
