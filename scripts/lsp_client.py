"""Minimal JSON-RPC client for driving `qz lsp` over stdio in check scripts."""

import json
import subprocess


class Client:
    def __init__(self, qz):
        self.process = subprocess.Popen([str(qz), "lsp"], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.next_id = 0

    def send(self, message):
        body = json.dumps({"jsonrpc": "2.0", **message}).encode()
        self.process.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        self.process.stdin.flush()

    def receive(self):
        length = None
        while True:
            line = self.process.stdout.readline()
            assert line, "server closed stdout"
            if line in (b"\r\n", b"\n"):
                break
            name, _, value = line.decode().partition(":")
            if name.lower() == "content-length":
                length = int(value)
        return json.loads(self.process.stdout.read(length))

    def request(self, method, params):
        self.next_id += 1
        request_id = self.next_id
        message = {"id": request_id, "method": method}
        if params is not None:
            message["params"] = params
        self.send(message)
        while True:
            message = self.receive()
            if message.get("id") == request_id and "method" not in message:
                assert "error" not in message, message
                return message.get("result")

    def notify(self, method, params):
        message = {"method": method}
        if params is not None:
            message["params"] = params
        self.send(message)

    def close(self):
        self.request("shutdown", None)
        self.notify("exit", None)
        self.process.stdin.close()
        self.process.wait(timeout=30)

    def wait_for(self, method):
        """Read messages until a notification `method` arrives; returns its params."""
        while True:
            message = self.receive()
            if message.get("method") == method:
                return message.get("params")
