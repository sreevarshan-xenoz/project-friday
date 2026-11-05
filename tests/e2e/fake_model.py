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

MARKER = "FAKE-MODEL-ANSWER"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _send(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path.rstrip("/").endswith("/models"):
            self._send(200, {"object": "list", "data": [{"id": "fake", "object": "model"}]})
        else:
            self._send(404, {"error": "not found"})

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        try:
            req = json.loads(self.rfile.read(length) or b"{}")
        except json.JSONDecodeError:
            req = {}
        messages = req.get("messages") or []
        text = "\n".join(
            m.get("content") if isinstance(m.get("content"), str) else json.dumps(m.get("content"))
            for m in messages
        )
        with open(LOG, "a", encoding="utf-8") as log:
            log.write(json.dumps({"path": self.path, "model": req.get("model"), "prompt": text}) + "\n")
        if "Investigation Engine" in text:
            content = '{"requirements":[]}'
        else:
            # Echo the user's request (the line after "Request:" in the answer
            # step's prompt), so a test can tell which question was answered.
            lines = text.splitlines()
            request = next((lines[i + 1] for i, l in enumerate(lines[:-1]) if l.strip() == "Request:"), "")
            if not request:
                request = ([l for l in lines if l.strip()][-1:] or [""])[0]
            content = f"{MARKER}: {request[:200]}"
        self._send(200, {
            "id": "fake-1",
            "object": "chat.completion",
            "model": req.get("model") or "fake",
            "choices": [{"index": 0, "finish_reason": "stop",
                         "message": {"role": "assistant", "content": content}}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        })


if __name__ == "__main__":
    PORT_FILE, LOG = sys.argv[1], sys.argv[2]
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    with open(PORT_FILE, "w", encoding="utf-8") as f:
        f.write(str(server.server_address[1]))
    server.serve_forever()
