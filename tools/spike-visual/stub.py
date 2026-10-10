"""SPIKE (spike-visual): a hard-coded stand-in supervisor for screenshots.

Answers the routes the screenshots ask for with fixed documents, so the
`printobserver` command renders them as it would a real supervisor's answer.
`STUB_MODE=before` answers today's accepted start; `after` the planned refusal.
"""

import json
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

MODE = os.environ.get("STUB_MODE", "after")
PRINT = "0199d4c2-7a10-7c3e-9f4b-2d1e6a8b0c11"
ACTION = "0199d4c9-1b22-7f05-8a61-5c3d9e7f2a40"
PLA = "0199d3f0-4c8a-7b21-a0d2-91e4b6c7d801"
PETG = "0199d3f1-02bd-7e6f-b3a4-6f0c2d8e9a12"
SILK = "0199d3f2-9e31-7a40-8c55-3b7d1f0e6c23"
FILE = "benchy_0.2mm_PLA.gcode"
T = "2026-10-10T{}Z"

MANIFEST = {"file_name": FILE, "material": "PLA", "nozzle_diameter_mm": 0.4,
            "slicer_profile": "draft", "allowed": {}, "metadata": {}}


def spool(sid, name, material, color, density, initial, remaining, tool, trust, tare=None):
    return {"id": sid, "name": name, "material": material, "color": color,
            "diameter_mm": 1.75, "density_g_cm3": density, "initial_g": initial,
            "remaining_g": remaining, "tare_g": tare, "loaded_on_tool": tool, "trust": trust,
            "registered_at": T.format("09:02:11"), "updated_at": T.format("15:01:30"),
            "retired_at": None}


SPOOLS = {"spools": [
    spool(PLA, "Prusament Galaxy Black", "PLA", "black", 1.24, 1000.0, 612.4, None,
          {"state": "verified"}, 254.0),
    spool(PETG, "Generic PETG clear", "PETG", "clear", 1.27, 1000.0, 860.0, None,
          {"state": "unverified", "marks": [{"reason": {"reason": "uncharged_print",
           "print_id": "0199c1aa-5d02-7b8e-9e10-4a2f7c6b3d90"}, "since": T.format("08:12:40")}]}),
    spool(SILK, "Silk gold (opened)", "PLA", "gold", 1.24, 250.0, 38.2, 0,
          {"state": "verified"}),
], "pending": []}


def event(eid, kind, at, payload, print_id=None, source="system"):
    e = {"id": eid, "kind": kind, "payload": payload, "received_at": T.format(at), "source": source}
    if print_id:
        e["print_id"] = print_id
    return e


TIMELINE = {"events": [
    event("0199d50b-3e70-7c12-8d4f-0a9b1c2d3e05", "spool_adjusted", "14:41:07", {
        "spool_id": PLA, "remaining_before_g": 618.9, "remaining_after_g": 612.4,
        "gross_g": 866.4, "actor": "operator",
        "reason": "weighed after the cancelled benchy"}, source="operator"),
    event("0199d4f8-77c1-7a3d-9b20-1e2f3a4b5c04", "filament_consumed", "13:58:30", {
        "charge_key": {"key": "print", "print_id": PRINT, "tool": 0, "segment": 0},
        "tool": 0, "spool_id": PLA, "method": "gcode_extrusion", "length_mm": 2856.1,
        "grams": 8.51, "diameter_mm": 1.75, "density_g_cm3": 1.24,
        "remaining_before_g": 627.41, "remaining_after_g": 618.9}, PRINT),
    event("0199d4f8-77c0-7d91-a4e3-2b3c4d5e6f03", "print_ended", "13:58:29", {
        "outcome": "cancelled", "state": "operational", "reason": "cancelled by the operator",
        "last_position": {"bytes": 412877, "size_bytes": 2310455,
                          "observed_at": T.format("13:58:28")}}, PRINT),
    event("0199d4c9-1b23-7b02-9c11-3c4d5e6f7a02", "print_started", "13:20:02", {
        "file_name": FILE, "started_by": ACTION, "loaded": [{"tool": 0, "spool_id": PLA}],
        "preflight": None}, PRINT),
    event("0199d3f4-0a11-7e20-8f31-4d5e6f7a8b01", "spool_loaded", "09:04:50", {
        "spool_id": PLA, "tool": 0, "replaced": None, "actor": "operator",
        "reason": "new spool on the MK4"}, source="operator"),
    event("0199d3f0-4c8b-7f62-b1c3-5e6f7a8b9c00", "spool_registered", "09:02:11", {
        "spool": dict(spool(PLA, "Prusament Galaxy Black", "PLA", "black", 1.24, 1000.0, 627.41,
                            None, {"state": "verified"}, 254.0), updated_at=T.format("09:02:11")),
        "actor": "operator", "reason": "part-used spool moved over from the old printer"},
        source="operator"),
]}

PREFLIGHT = {"preflight": {"file_name": FILE, "tools": [{
    "tool": 0, "spool_id": SILK, "required_g": 41.6, "buffer_g": 5.0, "remaining_g": 38.2,
    "verdict": "insufficient"}], "verdict": "insufficient", "checked_at": T.format("15:02:44")}}

ACCEPTED = {"record": {
    "decision": "accepted", "executed_at": T.format("13:20:02"), "id": ACTION,
    "outcome": "succeeded", "print_id": PRINT,
    "request": {"action": {"action": "start_print", "actor": "operator", "file_name": FILE,
                           "manifest": MANIFEST, "reason": "printing a benchy"},
                "actor": "operator", "requested_at": T.format("13:20:01")}}}

REJECTED = {"record": {
    "decision": {"rejected": {"insufficient_filament": {
        "tool": 0, "spool_id": SILK, "required_g": 41.6, "buffer_g": 5.0, "remaining_g": 38.2}}},
    "request": {"actor": "operator", "requested_at": T.format("15:02:44")}}}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def answer(self, status, doc):
        body = json.dumps(doc).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split("?")[0]
        routes = {"/v1/spools": SPOOLS, "/v1/history": TIMELINE, "/v1/preflight": PREFLIGHT}
        self.answer(200, routes[path]) if path in routes else self.answer(404, {"error": "not found"})

    def do_POST(self):
        self.rfile.read(int(self.headers.get("content-length", 0)))
        if self.path.endswith("/actions/start_print"):
            self.answer(*((200, ACCEPTED) if MODE == "before" else (409, REJECTED)))
        else:
            self.answer(404, {"error": "not found"})

    def log_message(self, *_):
        pass


HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
