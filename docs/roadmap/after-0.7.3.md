# After v0.7.3: the brainstorm queue

On 2026-11-21 thirteen outside opinions (ChatGPT, Gemini and eleven
OpenCode free models) brainstormed what FRIDAY should improve and add. Their
ideas were merged, checked against the code, and the owner approved all
twenty below, in this order. One item at a time, each with its own branch,
tests and review; the real-hardware pass (`docs/HARDWARE-PASS-2026-11-22.md`)
comes first, and anything it finds jumps the queue. The vNext milestones
(`vnext.md`) stay the frame: these feed N2–N4 or sit beside them.

"Votes" is how many of the thirteen suggested it.

| # | Item | Votes | What done looks like |
|---|---|---|---|
| 1 | **Fast first answer** | 9 | A busy free provider no longer costs 15 s before the next is tried: the next starts when the first is slow to answer, the pill says which provider it's trying, and CI checks a keypress-to-first-words budget. |
| 2 | **The open audit items** | 7 | One project identity carried through timeline, thoughts and git facts; one resume grammar tested against the shell and the daemon; lessons scoped to their project (`vnext.md`, "Still open"). |
| 3 | **What FRIDAY used** | 5 | Every answer can show what went into it (selection, window, recall, page, lessons, project facts) and what left the computer, with "ask again without this". |
| 4 | **Screen asks send text first** | 3 | The window's accessibility text goes instead of a screenshot when there's enough of it; the screenshot only when there isn't. |
| 5 | **Onboarding that ends in a success** | 7 | `friday doctor` and the tour run a real ask end to end and finish with one task that works on the person's own window. |
| 6 | **Windows updates itself** | 2 | The tray checks GitHub releases, offers the new MSI and installs it. |
| 7 | **Clipboard history and snippets** | 4 | Searchable local clip history (secrets masked, private windows skipped), rewrite/paste-as on any clip, `;name` snippets. |
| 8 | **Undo for real actions** | 6 | Window moves, launches and file writes can be undone; an agent run ends with a diff and `friday agents revert`; answers get Run this / Undo. |
| 9 | **Terminal help beyond kitty** | 7 | "Explain the last error" and "tell me when it finishes" in foot, WezTerm and Windows Terminal; a confirmed "try a fix" loop. |
| 10 | **Browser tab context** | 4 | The focused tab's title, URL, selection and text come through CDP instead of a screenshot; "show me" works in Chromium. |
| 11 | **Thumbs-down becomes a test** | 2 | A bad answer saves a redacted repro (no content) to turn into a test. |
| 12 | **Windows parity** | 9 | Private windows, paste-as/rewrite, Type it, undo and N1 (workspaces, thoughts, timeline) on Windows. |
| 13 | **Longer recall and a timeline view** | 8 | Recall keeps 3 h / 24 h / 7 d (opt-in, local), a "what was I doing at 2:30" strip, cited search; OCR text for apps without accessibility text. |
| 14 | **Hands-free conversation** | 4 | "Friday, keep listening", with barge-in, from the existing wake word, transcription and voice. |
| 15 | **Privacy zones** | 2 | Per-app, per-site and per-folder rules: local only, never read, never indexed. |
| 16 | **Meetings become project memory** | 2 | A caption transcript becomes decisions, actions and questions in the project's thread, with timestamps. |
| 17 | **FRIDAY as an MCP server** | 3 | Other AI apps can use FRIDAY's context, projects and actions, each behind a permission. |
| 18 | **Search a folder you pick** | 3 | Local, cited search over the PDFs and notes of folders the person chooses. |
| 19 | **Teach a routine by showing it** | 5 | Record your own steps once (visible recording mark), edit the list, save as a routine. |
| 20 | **Encrypted sync** | 2 | Threads and workspaces sync between your own machines, end to end encrypted, opt-in per project. |

Left out on purpose: always-on capture, nagging, a second memory engine
(they break the guardrails in `vnext.md`); the daily brief and cited project
answers are already N3 and N2.
