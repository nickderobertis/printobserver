"""SPIKE MOCK (throwaway): a stand-in supervisor answering canned documents.

The real `printobserver` command is pointed at it, so every screenshot is the
real command's own rendering of an answer. `before` answers are what the
unchanged server answers for the synthetic print at its 13:12 check; `after`
answers add the fields the plan adds. Nothing here is the server's logic.

usage: stub_server.py <port> <before|after> <state-dir>
"""

import copy
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

PORT, MODE, STATE = int(sys.argv[1]), sys.argv[2], sys.argv[3]

PRINT = "0193688a-5c00-7a1e-8b0c-5e1f0a2b3c4d"
FRAME_1212 = "5e1f" + "0" * 56 + "1212"
FRAME_1214 = "5e1f" + "0" * 56 + "1214"
FRAME_FRESH = "5e1f" + "0" * 56 + "1931"
WHOLE_1212 = f"{STATE}/check-images/5e-{FRAME_1212}-1280x720.jpg"
FRESH_PATH = f"{STATE}/images/5e/{FRAME_FRESH}"


def event(n, at, kind, payload, image=None):
    return {
        "id": f"0193688a-{6000 + n:04x}-7000-8000-00000000{n:04x}",
        "source": "supervisor",
        "received_at": at,
        "kind": kind,
        "print_id": PRINT,
        "payload": payload,
        "image": {"id": f"0193688a-{7000 + n:04x}-7000-8000-00000000{n:04x}", "sha256": image}
        if image
        else None,
    }


PRINTER = {
    "connection": "printing",
    "observed_at": "2026-11-26T19:12:07Z",
    "tools": [{"actual_c": 215.1, "target_c": 215.0}],
    "bed": {"actual_c": 60.0, "target_c": 60.0},
    "fan_percent": 100.0,
    "feedrate_factor": 1.0,
    "flowrate_factor": 1.0,
}
JOB = {"state": "printing", "file_name": "two-colour-box.gcode", "completion": 4.1,
       "file_position_bytes": 4224}

WHOLE_NOTE = (
    "the whole camera frame, scaled to fit 1280x720 and uncropped, because the print's "
    "first-layer alignment failed: the printed first layer lies 59.7 px from where the "
    "calibration projects it"
)
INCONCLUSIVE = (
    "the print's first-layer alignment is inconclusive: the toolhead covers where the "
    "first layer was drawn, so it is judged again on the next check's frame"
)

VIEW = {
    "model": "translation",
    "matrix": [[1.0, 0.0, 75.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    "landmarks": [
        {"name": "rail-bracket-top-right", "offset_px": [74.2, 0.8], "score": 0.91},
        {"name": "rail-end-right", "offset_px": [75.6, -0.4], "score": 0.88},
        {"name": "frame-screws-right-front", "offset_px": [74.9, 1.1], "score": 0.84},
        {"name": "frame-label-right", "offset_px": [75.3, -1.5], "score": 0.79},
    ],
    "residual_px": 1.2,
}

# --- look -----------------------------------------------------------------
look = {
    "event": event(1, "2026-11-26T19:12:09Z", "camera_look",
                   {"waited_s": 0, "frame_captured_at": "2026-11-26T19:12:07Z", "delivered": []},
                   FRAME_1212),
    "detector_paused": False,
    "frame": {"id": "0193688a-7001-7000-8000-000000000001", "sha256": FRAME_1212},
    "image_path": WHOLE_1212,
    "image_note": WHOLE_NOTE,
    "printer": PRINTER,
    "job": JOB,
}

# --- status ---------------------------------------------------------------
status = {
    "print": {"id": PRINT, "state": "printing", "file_name": "two-colour-box.gcode",
              "opened_at": "2026-11-26T19:05:41Z", "job_started_at": "2026-11-26T19:05:44Z",
              "narrowings": []},
    "printer": PRINTER,
    "job": JOB,
    "interventions": [],
    "detector": {"status": "scoring_nothing_flagged", "since": "2026-11-26T19:06:02Z"},
    "session": None,
}

# --- history --------------------------------------------------------------
observation_1212 = event(10, "2026-11-26T19:12:07Z", "periodic_observation",
                         {"interval_s": 120, "frame_captured_at": "2026-11-26T19:12:07Z"},
                         FRAME_1212)
observation_1214 = event(20, "2026-11-26T19:14:08Z", "periodic_observation",
                         {"interval_s": 120, "frame_captured_at": "2026-11-26T19:14:08Z"},
                         FRAME_1214)
new_events = [
    event(11, "2026-11-26T19:12:07Z", "camera_view_judged",
          {"frame_sha256": FRAME_1212, "outcome": "refit", "view_correction": VIEW, "reason": None}),
    event(12, "2026-11-26T19:12:07Z", "camera_view_refit",
          {"frame_sha256": FRAME_1212, "view_correction": VIEW}),
    event(13, "2026-11-26T19:12:07Z", "first_layer_alignment_judged",
          {"verdict": "inconclusive", "offset_px": None, "correction_px": None,
           "match_strength": 0.22, "reason": INCONCLUSIVE, "frame_sha256": FRAME_1212,
           "view_correction": VIEW}),
    event(14, "2026-11-26T19:12:07Z", "check_image_changed",
          {"kind": "whole_frame",
           "fallback": {"code": "alignment_inconclusive", "message": INCONCLUSIVE},
           "frame_sha256": FRAME_1212}),
    event(21, "2026-11-26T19:14:08Z", "camera_view_judged",
          {"frame_sha256": FRAME_1214, "outcome": "refit", "view_correction": VIEW, "reason": None}),
    event(22, "2026-11-26T19:14:08Z", "first_layer_alignment_judged",
          {"verdict": "passed", "offset_px": 18.4, "correction_px": [-15.0, 30.0],
           "match_strength": 0.81, "reason": "the first layer was found clearly, 18.4 px from "
           "where the re-fit view projects it, and that offset is corrected from here on",
           "frame_sha256": FRAME_1214, "view_correction": VIEW}),
    event(23, "2026-11-26T19:14:08Z", "check_image_changed",
          {"kind": "three_panel", "fallback": None, "frame_sha256": FRAME_1214}),
]

# --- recheck-camera (after only) -----------------------------------------
recheck = {
    "frame_path": FRESH_PATH,
    "alignment": {"verdict": "passed", "offset_px": 18.4, "correction_px": [-15.0, 30.0]},
    "view_correction": VIEW,
    "image_kind": "three_panel",
    "image_fallback": None,
}

if MODE == "after":
    look["image_kind"] = "whole_frame"
    look["image_fallback"] = {"code": "alignment_inconclusive", "message": INCONCLUSIVE}
    look["image_note"] = (
        "the whole camera frame, scaled to fit 1280x720 and uncropped, because " + INCONCLUSIVE
    )
    status["last_check_image"] = {
        "at": "2026-11-26T19:12:07Z",
        "kind": "whole_frame",
        "fallback": {"code": "alignment_inconclusive", "message": INCONCLUSIVE},
    }
    history = [observation_1214, *reversed(new_events[4:]), observation_1212,
               *reversed(new_events[:4])]
else:
    history = [observation_1214, observation_1212]

# Newest first, as the server answers.
history = sorted(history, key=lambda each: (each["received_at"], each["id"]), reverse=True)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _answer(self, document, code=200):
        body = json.dumps(document).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path.endswith("/look"):
            return self._answer(copy.deepcopy(look))
        if path.endswith("/status"):
            return self._answer(status)
        if path.endswith("/history"):
            return self._answer({"events": history})
        return self._answer({"error": "not mocked", "path": path}, 404)

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        if self.path.endswith("/camera/recheck") and MODE == "after":
            return self._answer(recheck)
        return self._answer({"error": "not mocked", "path": self.path}, 404)

    def log_message(self, *args):
        pass


HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
