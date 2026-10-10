#!/usr/bin/env python3
"""A stand-in for the Anthropic Messages API, for the smoke walkthrough.

Answers POST /v1/messages with a short summary built from the request — or,
for the writing assistant, a proofread, rewrite, continuation or answer — and
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
        # Writing the daily prompt sends no writer's text, so it is logged on
        # its own, out of the way of the checks that nothing else was sent.
        log_to = LOG + ".prompts" if "daily writing prompt" in body["system"] else LOG
        with open(log_to, "a") as log:
            log.write(json.dumps({"path": self.path, "headers": dict(self.headers), "body": body}) + "\n")
        text = body["messages"][0]["content"]
        words = len(text.split())
        answer = f"A stub summary of {words} words of manuscript."
        # The daily prompt: a new one each time, numbered by how many came before.
        if "daily writing prompt" in body["system"]:
            answer = f"A stub prompt: a lighthouse keeper finds {text.count('<prompt>') + 1} letters on the stairs."
        # The writing assistant: an answer shaped like the one it asked for.
        if "writing assistant" in body["system"]:
            passage = text.split("<passage>\n", 1)[1].split("\n</passage>", 1)[0] if "<passage>" in text else ""
            if "Proofread" in body["system"]:
                answer = passage.replace("teh", "the").replace("recieve", "receive")
            elif "Rewrite the passage" in body["system"]:
                answer = f"A stub rewrite of {len(passage.split())} words."
            elif "Continue the scene" in body["system"]:
                answer = "the stub went on.\n\nA second stub paragraph."
            else:
                answer = "A stub answer to the question."
        reply = {
            "id": "msg_stub",
            "type": "message",
            "role": "assistant",
            "model": body["model"],
            "stop_reason": "end_turn",
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": answer},
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
