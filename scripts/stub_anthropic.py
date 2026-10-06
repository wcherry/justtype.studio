#!/usr/bin/env python3
"""A stand-in for the Anthropic Messages API, for the smoke walkthrough.

Answers POST /v1/messages with a short summary built from the request, and
writes every request it receives (headers and body) to a log so the smoke can
check what the server sent: the model, the fallback header, the manuscript in
its tags. Spends nothing and needs no key.

    stub_anthropic.py PORT LOGFILE
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

PORT, LOG = int(sys.argv[1]), sys.argv[2]


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        with open(LOG, "a") as log:
            log.write(json.dumps({"path": self.path, "headers": dict(self.headers), "body": body}) + "\n")
        text = body["messages"][0]["content"]
        words = len(text.split())
        reply = {
            "id": "msg_stub",
            "type": "message",
            "role": "assistant",
            "model": body["model"],
            "stop_reason": "end_turn",
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": f"A stub summary of {words} words of manuscript."},
            ],
            "usage": {"input_tokens": words, "output_tokens": 9},
        }
        out = json.dumps(reply).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, *args):
        pass


HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
