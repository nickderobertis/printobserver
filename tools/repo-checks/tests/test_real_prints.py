"""The real-print cases under `tests/real-prints`, and the checks holding them.

Every test drives a check through `python -m repo_checks <check> --root`, which
is the entry point `just check-repo` runs, over the committed tree or over a
copy of it broken in exactly one way, and reads what it printed.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from typing import Any

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import equal, truth
from treecopy import REPO_ROOT, Tree

CHECKS = (
    "real-prints-schema",
    "real-prints-files",
    "real-prints-images",
    "real-prints-scenarios",
    "real-prints-operations",
    "real-prints-service-config",
)

ROOT = "tests/real-prints"
SLAB = f"{ROOT}/spaghetti-floating-slab"
NEST = f"{ROOT}/spaghetti-small-nest"
FAN = f"{ROOT}/fan-cut-bridge"
CONFIG = f"{ROOT}/service-config.toml"

# The scenarios each case declares, as the task that brought the cases here
# lists them. `bed-clear-check` has one per labelled frame.
SCENARIOS = {
    "spaghetti-floating-slab": ["paused-alert"],
    "spaghetti-small-nest": ["warning-then-detector-pause"],
    "spaghetti-debris-warning": ["static-debris-warning"],
    "filament-stall-air-print": ["stalled-while-reported-printing"],
    "fan-cut-bridge": ["bridge-sagging-uncooled"],
    "under-extrusion-lace": ["porous-walls"],
    "healthy-printing": ["toolhead-false-positive"],
    "bed-clear-check": [
        "clear-01-bed-lowered-1727",
        "clear-02-bed-raised-1921",
        "clear-03-bed-raised-2044",
        "clear-04-heating-before-first-layer-2044",
        "clear-05-head-parked-2219",
        "not-clear-01-one-stray-strand-2218",
        "not-clear-02-failed-print-left-on-bed-1546",
        "not-clear-03-parts-and-strands-0415",
    ],
}


Run = Callable[[str, Tree | None], tuple[int, str]]


@pytest.fixture
def check(capsys: pytest.CaptureFixture[str]) -> Run:
    """Run one check by name over a tree, answering its exit and what it printed."""

    def run(name: str, tree: Tree | None = None) -> tuple[int, str]:
        root = REPO_ROOT if tree is None else tree.root
        code = main([name, "--root", str(root)])
        return code, capsys.readouterr().err

    return run


def refused_with(result: tuple[int, str], *parts: str) -> None:
    """The check failed, with one finding naming every part."""
    code, err = result
    equal(code, 1, describing=f"the check's exit, having printed:\n{err}")
    lines = [line for line in err.splitlines() if all(part in line for part in parts)]
    truth(lines, describing=f"a finding naming {parts} in:\n{err}")


def case_json(tree: Tree, case: str) -> dict[str, Any]:
    """One case's parsed case.json in a copy of the tree."""
    return json.loads(tree.read(f"{ROOT}/{case}/case.json"))


def write_case(tree: Tree, case: str, data: dict[str, Any]) -> None:
    """Replace one case's case.json in a copy of the tree."""
    tree.write(f"{ROOT}/{case}/case.json", json.dumps(data, indent=2) + "\n")


@pytest.mark.parametrize("name", CHECKS)
def test_every_check_passes_on_the_committed_cases(check: Run, name: str) -> None:
    """Silent and zero over the cases this repository ships."""
    equal(check(name, None), (0, ""))


def test_every_case_declares_exactly_its_scenarios() -> None:
    """Each case carries the scenarios it was given, and nothing else."""
    for case, ids in SCENARIOS.items():
        data = json.loads((REPO_ROOT / ROOT / case / "case.json").read_text(encoding="utf-8"))
        equal([s["id"] for s in data["assertions"]["scenarios"]], ids, describing=case)
    bed = json.loads((REPO_ROOT / ROOT / "bed-clear-check/case.json").read_text(encoding="utf-8"))
    frames = [frame["file"] for frame in bed["frames"]]
    equal([s["event_image"] for s in bed["assertions"]["scenarios"]], frames)


def test_a_case_without_assertions_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """The schema requires the contract the skill tests read."""
    copy = tree()
    data = case_json(copy, "fan-cut-bridge")
    del data["assertions"]
    write_case(copy, "fan-cut-bridge", data)
    refused_with(check("real-prints-schema", copy), "fan-cut-bridge", "'assertions' is a required")
    # Every other check reading the cases names the one it could not read,
    # rather than passing it.
    for name in CHECKS[1:-1]:
        refused_with(check(name, copy), "fan-cut-bridge", "unreadable", name)


def test_a_recorded_alert_without_its_event_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """`event_id` is required exactly when the trigger is a recorded alert."""
    copy = tree()
    data = case_json(copy, "spaghetti-floating-slab")
    del data["assertions"]["scenarios"][0]["event_id"]
    write_case(copy, "spaghetti-floating-slab", data)
    refused_with(check("real-prints-schema", copy), "spaghetti-floating-slab", "'event_id'")


def test_a_case_that_does_not_parse_or_is_missing_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """A broken case.json and a directory with none are each named."""
    copy = tree()
    copy.write(f"{FAN}/case.json", "{ not json")
    copy.write(f"{ROOT}/stray-case/notes.txt", "a directory nobody described\n")
    result = check("real-prints-schema", copy)
    refused_with(result, "fan-cut-bridge", "does not parse")
    refused_with(result, "stray-case", "has no case.json")


def test_a_history_breaking_the_servers_contract_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """A history is held to the server's own `HistoryAnswer` before it is read."""
    copy = tree()
    history = f"{NEST}/printobserver-history.json"
    data = json.loads(copy.read(history))
    del data["events"][0]["kind"]
    copy.write(history, json.dumps(data))
    copy.write(f"{SLAB}/printobserver-history.json", "{ not json")
    result = check("real-prints-schema", copy)
    refused_with(result, "spaghetti-small-nest", "printobserver-history.json", "'kind'")
    refused_with(result, "spaghetti-floating-slab", "printobserver-history.json does not parse")
    refused_with(check("real-prints-images", copy), "spaghetti-small-nest", "unreadable")


def test_a_case_named_for_another_directory_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """A case's name is its directory's."""
    copy = tree()
    data = case_json(copy, "fan-cut-bridge")
    data["case"] = "fan-cut"
    write_case(copy, "fan-cut-bridge", data)
    refused_with(check("real-prints-schema", copy), "fan-cut-bridge", "'fan-cut'")


def test_a_flipped_image_byte_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """An agent image that is not the picture the turn was given."""
    copy = tree()
    image = copy.root / SLAB / "agent-alert-2-paused-1545.jpg"
    data = bytearray(image.read_bytes())
    data[len(data) // 2] ^= 0xFF
    image.write_bytes(bytes(data))
    refused_with(
        check("real-prints-images", copy),
        "spaghetti-floating-slab",
        "agent-alert-2-paused-1545.jpg",
        "01a0e4d4-f282-7303-a7b4-d147e98219ed",
    )


def test_a_wrong_history_sha256_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """A history recording a different picture for an event than the case carries."""
    copy = tree()
    history = f"{NEST}/printobserver-history.json"
    data = json.loads(copy.read(history))
    for event in data["events"]:
        if event["id"] == "01a0e5f7-d9cc-7193-a1ed-63752d271ce2":
            event["image"]["sha256"] = "0" * 64
    copy.write(history, json.dumps(data))
    refused_with(
        check("real-prints-images", copy),
        "spaghetti-small-nest",
        "agent-look-1-2103.jpg",
        "0" * 64,
    )


def test_an_agent_image_with_no_event_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """An agent image unlisted, or listed against an event that recorded no image."""
    copy = tree()
    data = case_json(copy, "spaghetti-small-nest")
    data["agent_images"] = [
        entry for entry in data["agent_images"] if entry["file"] != "agent-look-1-2103.jpg"
    ]
    data["agent_images"][0]["event_id"] = "01a0e5f9-4366-7cf1-9c21-74c75dd44fdd"
    write_case(copy, "spaghetti-small-nest", data)
    result = check("real-prints-images", copy)
    refused_with(result, "spaghetti-small-nest", "agent-look-1-2103.jpg", "not listed")
    refused_with(
        result, "spaghetti-small-nest", "agent-alert-1-warning-2103.jpg", "records no image"
    )


def test_a_missing_referenced_file_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """A frame the case names but does not carry."""
    copy = tree()
    (copy.root / SLAB / "05-paused-by-obico-1546.jpg").unlink()
    refused_with(
        check("real-prints-files", copy),
        "spaghetti-floating-slab",
        "05-paused-by-obico-1546.jpg",
        "not a file",
    )
    refused_with(
        check("real-prints-scenarios", copy),
        "spaghetti-floating-slab",
        "paused-alert",
        "05-paused-by-obico-1546.jpg",
    )


def test_a_file_named_anywhere_in_a_source_must_exist(check: Run, tree: Callable[[], Tree]) -> None:
    """Every file a `sources` sentence names is checked, not only the one it leads with."""
    copy = tree()
    (copy.root / ROOT / "under-extrusion-lace/make_box.py").unlink()
    refused_with(
        check("real-prints-files", copy), "under-extrusion-lace", "make_box.py", "not a file"
    )


def test_an_unreferenced_extra_file_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """A file in a case that its case.json says nothing about."""
    copy = tree()
    (copy.root / FAN / "06-unlabelled.jpg").write_bytes(b"\xff\xd8\xff")
    refused_with(
        check("real-prints-files", copy),
        "fan-cut-bridge",
        "06-unlabelled.jpg",
        "names it nowhere",
    )


def test_a_dropped_file_must_name_its_branch(check: Run, tree: Callable[[], Tree]) -> None:
    """A G-code named bare, or a kept file pointed at the branch, is refused."""
    copy = tree()
    data = case_json(copy, "fan-cut-bridge")
    data["sources"]["gcode"] = "fan-cut-test.gcode is the file OctoPrint printed"
    data["frames"][0]["file"] = (
        "fix/windows-supervision-turns@45e7fed:tests/real-prints/fan-cut-bridge/05-finished.jpg"
    )
    write_case(copy, "fan-cut-bridge", data)
    result = check("real-prints-files", copy)
    refused_with(result, "fan-cut-bridge", "fan-cut-test.gcode", "without the branch")
    refused_with(result, "fan-cut-bridge", "05-finished.jpg", "this tree should carry")


def test_an_unknown_event_id_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """A scenario's event, and an event a look delivers, must be in the history."""
    copy = tree()
    data = case_json(copy, "spaghetti-small-nest")
    scenario = data["assertions"]["scenarios"][0]
    scenario["event_id"] = "01a0e5f7-0000-0000-0000-000000000000"
    scenario["looks"][1]["arrived_event_ids"] = ["01a0e5f8-0000-0000-0000-000000000000"]
    write_case(copy, "spaghetti-small-nest", data)
    result = check("real-prints-scenarios", copy)
    refused_with(result, "warning-then-detector-pause", "01a0e5f7-0000-0000-0000-000000000000")
    refused_with(result, "warning-then-detector-pause", "01a0e5f8-0000-0000-0000-000000000000")


def test_a_recorded_alert_must_be_an_alert_with_its_own_image(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """The event is an Obico alert, and is handed with the picture it recorded."""
    copy = tree()
    data = case_json(copy, "spaghetti-small-nest")
    scenario = data["assertions"]["scenarios"][0]
    scenario["event_id"] = "01a0e5f7-d9cc-7193-a1ed-63752d271ce2"
    scenario["printer_state"] = "spinning"
    write_case(copy, "spaghetti-small-nest", data)
    result = check("real-prints-scenarios", copy)
    refused_with(result, "warning-then-detector-pause", "no obico_failure_alert")
    refused_with(result, "warning-then-detector-pause", "agent-look-1-2103.jpg")
    refused_with(result, "warning-then-detector-pause", "'spinning'")


def test_a_recorded_alert_whose_picture_the_case_does_not_keep_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """A recorded alert's image is the agent image kept against that event."""
    copy = tree()
    data = case_json(copy, "spaghetti-floating-slab")
    data["agent_images"] = [
        entry
        for entry in data["agent_images"]
        if entry["event_id"] != "01a0e4d4-f282-7303-a7b4-d147e98219ed"
    ]
    write_case(copy, "spaghetti-floating-slab", data)
    refused_with(
        check("real-prints-scenarios", copy),
        "paused-alert",
        "01a0e4d4-f282-7303-a7b4-d147e98219ed",
        "no agent_images entry",
    )


def test_a_recorded_alert_in_a_case_with_no_history_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """A case with no history has no recorded alert to name."""
    copy = tree()
    data = case_json(copy, "fan-cut-bridge")
    scenario = data["assertions"]["scenarios"][0]
    scenario["trigger"] = "recorded_alert"
    scenario["event_id"] = "01a0e5f7-7e0d-7bd0-b2a9-6cfdfbdf9ce0"
    data["assertions"]["scenarios"].append(dict(scenario))
    write_case(copy, "fan-cut-bridge", data)
    result = check("real-prints-scenarios", copy)
    refused_with(result, "fan-cut-bridge", "has no printobserver-history.json")
    refused_with(result, "fan-cut-bridge", "declared twice")


def test_a_scenario_image_from_another_case_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """Every image a scenario names is a file of its own case."""
    copy = tree()
    data = case_json(copy, "fan-cut-bridge")
    data["assertions"]["scenarios"][0]["looks"][0]["image"] = (
        "../under-extrusion-lace/05-finished.jpg"
    )
    write_case(copy, "fan-cut-bridge", data)
    refused_with(
        check("real-prints-scenarios", copy),
        "bridge-sagging-uncooled",
        "../under-extrusion-lace/05-finished.jpg",
    )


def test_an_unknown_operation_or_argument_is_refused(check: Run, tree: Callable[[], Tree]) -> None:
    """A step names a command printobserver has, with that command's parameters."""
    copy = tree()
    data = case_json(copy, "fan-cut-bridge")
    scenario = data["assertions"]["scenarios"][0]
    scenario["accept_any_of"] = [[{"operation": "set-fan-percent", "args": {"pct": 100}}]]
    scenario["never"] = [
        {"operation": "set-fan-speed"},
        {"operation": "set-fan-percent", "args": {"percent": "full"}},
        {"operation": "set-fan-percent", "args": {"duration_s": 1.5}},
        {"operation": "acknowledge-failure", "args": {"disposition": "halt"}},
    ]
    write_case(copy, "fan-cut-bridge", data)
    result = check("real-prints-operations", copy)
    refused_with(result, "fan-cut-bridge", "'pct'", "not one of its parameters")
    refused_with(result, "fan-cut-bridge", "'set-fan-speed'", "does not have")
    # Each value is held to its parameter's own shape, its referenced types included.
    refused_with(result, "fan-cut-bridge", "'full'", "shape refuses")
    refused_with(result, "fan-cut-bridge", "1.5", "shape refuses")
    refused_with(result, "fan-cut-bridge", "'halt'", "shape refuses")


@pytest.mark.parametrize(
    ("path", "old", "new", "name", "naming"),
    [
        (
            "schemas/printobserver-printer-api/PrinterState.json",
            '"oneOf": [',
            '"anyOf": [',
            "real-prints-scenarios",
            "names no state",
        ),
        (
            "schemas/printobserver-printer-api/PrinterState.json",
            '"oneOf": [',
            '"type": 5, "oneOf": [',
            "real-prints-scenarios",
            "not a valid JSON Schema",
        ),
        (
            "schemas/printobserver-server/operations.json",
            '"responses": [',
            '"responses": [], "was": [',
            "real-prints-operations",
            "cannot be read as the server's description",
        ),
        (
            "schemas/printobserver-core/AcknowledgementDisposition.json",
            '"oneOf": [',
            '"type": 5, "oneOf": [',
            "real-prints-operations",
            "AcknowledgementDisposition is not a valid JSON Schema",
        ),
        (
            "tests/real-prints/case.schema.json",
            '"type": "object",',
            '"type": 5,',
            "real-prints-schema",
            "case.schema.json is not a valid JSON Schema",
        ),
        (
            "schemas/printobserver-server/HistoryAnswer.json",
            '"type": "object"',
            '"type": 5',
            "real-prints-files",
            "HistoryAnswer.json is not a valid JSON Schema",
        ),
    ],
)
def test_a_contract_the_checks_read_must_itself_be_readable(
    check: Run, tree: Callable[[], Tree], path: str, old: str, new: str, name: str, naming: str
) -> None:
    """A committed contract that is not a valid schema is a finding, not a crash."""
    copy = tree()
    copy.edit(path, old, new)
    refused_with(check(name, copy), naming)


def test_a_contract_that_is_no_schema_object_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """A schema file holding a bare JSON value is named, not indexed into."""
    copy = tree()
    copy.write("schemas/printobserver-printer-api/PrinterState.json", "true\n")
    refused_with(
        check("real-prints-scenarios", copy), "PrinterState.json is not a JSON Schema object"
    )


def test_an_operation_renamed_in_the_server_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """The operations are read from the server's description, not restated."""
    copy = tree()
    copy.edit(
        "schemas/printobserver-server/operations.json",
        '"name": "set_flowrate_factor"',
        '"name": "set_flow_factor"',
    )
    refused_with(
        check("real-prints-operations", copy), "under-extrusion-lace", "set-flowrate-factor"
    )


@pytest.mark.parametrize(
    ("old", "new", "field"),
    [
        ('api_key = "<redacted>"', 'api_key = "4f1c2d0e9b"', "octoprint.api_key"),
        ('shared_secret = "<redacted>"', 'shared_secret = ""', "ingress.shared_secret"),
        ('access_token = "<redacted>"', 'access_token = "tok"', "obico.access_token"),
        ("[camera]", '[api]\ncredential = "long-random"\n\n[camera]', "api.credential"),
        # A secret field no configuration has yet is held by how it is named.
        ("[camera]", '[camera]\nstream_token = "abc"', "camera.stream_token"),
    ],
)
def test_an_unredacted_credential_is_refused(
    check: Run, tree: Callable[[], Tree], old: str, new: str, field: str
) -> None:
    """Every credential-bearing field holds the placeholder."""
    copy = tree()
    copy.edit(CONFIG, old, new)
    refused_with(check("real-prints-service-config", copy), "service-config.toml", field)


def test_a_service_config_that_does_not_parse_is_refused(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """The configuration is TOML a supervisor could read."""
    copy = tree()
    copy.append(CONFIG, "[safety\n")
    refused_with(check("real-prints-service-config", copy), "service-config.toml", "does not parse")
