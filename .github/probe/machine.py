import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _a(self, s, b):
        self.send_response(s)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        if self.path.startswith("/api/printer"):
            b = {"state": {"text": "Printing", "flags": {"operational": True, "printing": True}},
                 "temperature": {"tool0": {"actual": 210.0, "target": 210.0, "offset": 0.0},
                                 "bed": {"actual": 60.0, "target": 60.0, "offset": 0.0}}}
        elif self.path.startswith("/api/job"):
            b = {"state": "Printing", "job": {"file": {"name": "benchy.gcode", "origin": "local", "size": 4096},
                 "estimatedPrintTime": 3600.0}, "progress": {"completion": 12.5, "printTime": 450, "printTimeLeft": 3150}}
        else:
            return self._a(204, b"")
        self._a(200, json.dumps(b).encode())

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length") or 0))
        self._a(204, b"")

    def log_message(self, *a):
        pass


ThreadingHTTPServer(("127.0.0.1", 5000), H).serve_forever()
