#!/usr/bin/env python3
"""A fake OpenAI-compatible model server for FRIDAY's end-to-end ask test.

The daemon is pointed at it as the llama.cpp provider (LLAMACPP_ENABLED=1,
LLAMACPP_BASE_URL=http://127.0.0.1:<port>/v1). It answers deterministically:
  - the Investigation Engine's prompt gets {"requirements":[]} (no evidence),
  - any other prompt (the answer step) gets "FAKE-MODEL-ANSWER: " plus the
    user's request,
and appends every request to the log file, so a test can prove which model
calls a goal made and that its answer came from here.

Usage: fake_model.py <port-file> <log-file>   (writes the chosen port, then serves)
"""

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

MARKER = "FAKE-MODEL-ANSWER"


def reply_for(prompt: str) -> str:
    """The model's reply: no evidence for the investigation; for the answer
    step, an echo of the user's request (the line after "Request:"), so a test
    can tell which question was answered."""
    if "Investigation Engine" in prompt:
        return '{"requirements":[]}'
    lines = prompt.splitlines()
    request = ""
    for i, line in enumerate(lines[:-1]):
        if line.strip() == "Request:":
            request = lines[i + 1]
            break
    if not request:
        non_empty = [line for line in lines if line.strip()]
        request = non_empty[-1] if non_empty else ""
    return f"{MARKER}: {request[:200]}"


def prompt_of(request: dict[str, Any]) -> str:
    """All message contents of a chat-completions request, one per line."""
    parts = []
    for message in request.get("messages") or []:
        content = message.get("content")
        parts.append(content if isinstance(content, str) else json.dumps(content))
    return "\n".join(parts)


class Handler(BaseHTTPRequestHandler):
    log_path = ""

    def log_message(self, format: str, *args: Any) -> None:
        pass

    def _send(self, code: int, body: dict[str, Any]) -> None:
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self) -> None:
        if self.path.rstrip("/").endswith("/models"):
            self._send(200, {"object": "list", "data": [{"id": "fake", "object": "model"}]})
        else:
            self._send(404, {"error": "not found"})

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length") or 0)
        try:
            request = json.loads(self.rfile.read(length) or b"{}")
        except json.JSONDecodeError:
            request = {}
        prompt = prompt_of(request)
        with open(self.log_path, "a", encoding="utf-8") as log:
            entry = {"path": self.path, "model": request.get("model"), "prompt": prompt}
            log.write(json.dumps(entry) + "\n")
        self._send(
            200,
            {
                "id": "fake-1",
                "object": "chat.completion",
                "model": request.get("model") or "fake",
                "choices": [
                    {
                        "index": 0,
                        "finish_reason": "stop",
                        "message": {"role": "assistant", "content": reply_for(prompt)},
                    }
                ],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            },
        )


def main() -> None:
    port_file, log_file = sys.argv[1], sys.argv[2]
    Handler.log_path = log_file
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    with open(port_file, "w", encoding="utf-8") as f:
        f.write(str(server.server_address[1]))
    server.serve_forever()


if __name__ == "__main__":
    main()
