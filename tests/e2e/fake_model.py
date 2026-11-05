#!/usr/bin/env python3
"""A fake OpenAI-compatible model server for FRIDAY's end-to-end ask test.

The daemon is pointed at it as the llama.cpp provider (LLAMACPP_ENABLED=1,
LLAMACPP_BASE_URL=http://127.0.0.1:<port>/v1). It answers deterministically:
  - the Investigation Engine's prompt gets {"requirements":[]} (no evidence),
  - any other prompt (the answer step) gets "FAKE-MODEL-ANSWER: " plus the
    user's request,
  - a request with an image (a screen ask) gets the question and the image's
    type and size,
streams the reply (server-sent events, word by word) when asked to, and
appends every request to the log file, so a test can prove which model
calls a goal made and that its answer came from here.

Usage: fake_model.py <port-file> <log-file>   (writes the chosen port, then serves)
"""

import base64
import json
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

MARKER = "FAKE-MODEL-ANSWER"
# Seconds between streamed pieces (FRIDAY publishes progress every 0.15 s).
STREAM_PAUSE = 0.25


def reply_for(prompt: str, images: list[str] | None = None) -> str:
    """The model's reply: no evidence for the investigation; for the answer
    step, an echo of the user's request (the line after "Request:"), so a test
    can tell which question was answered. A request carrying an image (a
    screen ask) gets the question asked about it and the image's type and
    size, so a test can tell the screenshot itself reached the model."""
    if "Investigation Engine" in prompt:
        return '{"requirements":[]}'
    lines = prompt.splitlines()
    if images:
        # FRIDAY's screen instruction puts the question after "asks:".
        question = ""
        for i, line in enumerate(lines[:-2]):
            if line.rstrip().endswith("asks:"):
                question = lines[i + 2] if not lines[i + 1].strip() else lines[i + 1]
                break
        return f"{MARKER}: {question[:200]} [saw a {images[0]}]"
    request = ""
    for i, line in enumerate(lines[:-1]):
        if line.strip() == "Request:":
            request = lines[i + 1]
            break
    if not request:
        non_empty = [line for line in lines if line.strip()]
        request = non_empty[-1] if non_empty else ""
    return f"{MARKER}: {request[:200]}"


def describe_image(url: str) -> str:
    """ "<bytes>-byte <W>x<H> <mime>" for a data: URI (the size from the PNG
    header), else "remote image"."""
    match = re.match(r"data:([^;,]+);base64,(.*)$", url, re.S)
    if not match:
        return "remote image"
    try:
        data = base64.b64decode(match.group(2), validate=True)
    except ValueError:
        return f"undecodable {match.group(1)}"
    dims = ""
    if data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) >= 24:
        dims = f"{int.from_bytes(data[16:20], 'big')}x{int.from_bytes(data[20:24], 'big')} "
    return f"{len(data)}-byte {dims}{match.group(1)}"


def prompt_of(request: dict[str, Any]) -> tuple[str, list[str]]:
    """The text of a chat-completions request (one line per text part) and a
    description of each image part in it."""
    parts: list[str] = []
    images: list[str] = []
    for message in request.get("messages") or []:
        content = message.get("content")
        if isinstance(content, str):
            parts.append(content)
            continue
        for part in content or []:
            if part.get("type") == "text":
                parts.append(str(part.get("text", "")))
            elif part.get("type") == "image_url":
                images.append(describe_image(str((part.get("image_url") or {}).get("url", ""))))
    return "\n".join(parts), images


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

    def _stream(self, reply: str) -> None:
        """The reply as server-sent events, a word or so at a time with pauses
        longer than FRIDAY's progress throttle, so a test can see it grow."""
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        words = reply.split(" ")
        pieces = [w + " " for w in words[:-1]] + [words[-1]]
        for piece in pieces:
            event = {"choices": [{"index": 0, "delta": {"content": piece}}]}
            self.wfile.write(f"data: {json.dumps(event)}\n\n".encode())
            self.wfile.flush()
            time.sleep(STREAM_PAUSE)
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

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
        prompt, images = prompt_of(request)
        with open(self.log_path, "a", encoding="utf-8") as log:
            entry = {
                "path": self.path,
                "model": request.get("model"),
                "prompt": prompt,
                "images": images,
                "stream": bool(request.get("stream")),
            }
            log.write(json.dumps(entry) + "\n")
        if request.get("stream"):
            self._stream(reply_for(prompt, images))
            return
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
                        "message": {"role": "assistant", "content": reply_for(prompt, images)},
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
