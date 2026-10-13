"""Drive the hosted-runner temp recipe and read what real Python children use."""

from __future__ import annotations

import json
import os
import shutil
import sys
from pathlib import Path

import pytest
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.shell import run

ROOT = Path(__file__).resolve().parents[3]
PROBE = """
import json, os, tempfile
with tempfile.NamedTemporaryFile() as opened:
    print(json.dumps({"temp": tempfile.gettempdir(), "file": opened.name,
                      "TEMP": os.environ["TEMP"], "TMP": os.environ["TMP"]}))
"""


@pytest.mark.parametrize("action", ["setup", "cleanup"])
def test_exact_powershell_permissions_match_the_real_recipe(action: str) -> None:
    """Reconcile the scoped permission with just's actual flags and drive default."""
    settings = json.loads((ROOT / ".claude/settings.json").read_text(encoding="utf-8"))
    result = run(["just", "--dry-run", "runner-temp", action], cwd=ROOT)
    passing(result)
    _, separator, arguments = result.stderr.partition("; pwsh ")
    truth(separator, describing="the actual PowerShell invocation after the platform guard")
    command = "pwsh " + arguments.strip()
    truth(
        f"Bash({command})" in settings["permissions"]["allow"],
        describing="an exact permission reconciled against just's rendered invocation",
    )


@pytest.fixture(name="tmp_path")
def physical_tmp_path(tmp_path: Path) -> Path:
    """Supply a real physical directory even on hosts whose system temp has aliases."""
    return tmp_path.resolve()


def test_missing_powershell_names_how_to_restore_the_recipe(tmp_path: Path) -> None:
    """The real recipe refuses a PATH with no PowerShell before changing temp."""
    just = shutil.which("just")
    bash = shutil.which("bash")
    if just is None or bash is None:
        raise AssertionError("Install just and bash to drive the real recipe")
    empty_path = tmp_path / "no programs"
    empty_path.mkdir()
    environment = dict(os.environ, PATH=str(empty_path), PRINTOBSERVER_PLATFORM="windows-x86_64")
    environment_file = tmp_path / "job-env"
    environment["GITHUB_ENV"] = str(environment_file)
    result = run([just, "--shell", bash, "runner-temp"], cwd=ROOT, env=environment, timeout=30)
    failing(result, naming="install PowerShell Core or restore pwsh to PATH")
    truth(
        not environment_file.exists(), describing="no temp exports before the missing tool refusal"
    )


def _setup(tmp_path: Path, *, drive_present: bool) -> dict[str, str]:
    """Run the recipe before importing tempfile in a fresh child."""
    drive = tmp_path / "scratch drive"
    fallback = tmp_path / "existing temp"
    fallback.mkdir()
    (fallback / "unrelated").write_text("keep", encoding="utf-8")
    if drive_present:
        drive.mkdir()
        (drive / "unrelated").write_text("keep", encoding="utf-8")
    environment_file = tmp_path / "job-env"
    environment = dict(os.environ, PRINTOBSERVER_PLATFORM="windows-x86_64")
    environment["GITHUB_ENV"] = str(environment_file)
    result = run(
        ["just", "runner-temp", "setup", str(drive), str(fallback)],
        cwd=ROOT,
        env=environment,
        timeout=30,
    )
    passing(result)
    for line in environment_file.read_text(encoding="utf-8-sig").splitlines():
        name, value = line.split("=", 1)
        environment[name] = value
    return environment


@pytest.mark.parametrize("drive_present", [True, False])
def test_children_use_scratch_or_usable_fallback_and_cleanup_keeps_unrelated_files(
    tmp_path: Path, drive_present: bool
) -> None:
    """The real recipe exports the same temp to children and Python's file creation."""
    environment = _setup(tmp_path, drive_present=drive_present)
    chosen = Path(environment["TEMP"])
    expected_parent = tmp_path / ("scratch drive" if drive_present else "existing temp")
    equal(chosen.parent if drive_present else chosen, expected_parent)
    result = run(["just", "--command", sys.executable, "-c", PROBE], cwd=ROOT, env=environment)
    passing(result)
    answer = json.loads(result.stdout)
    equal(Path(answer["temp"]), chosen)
    equal(Path(answer["file"]).parent, chosen)
    equal(answer["TEMP"], answer["TMP"])
    passing(run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30))
    for line in Path(environment["GITHUB_ENV"]).read_text(encoding="utf-8-sig").splitlines():
        name, value = line.split("=", 1)
        environment[name] = value
    post_job = run(["just", "--command", sys.executable, "-c", PROBE], cwd=ROOT, env=environment)
    passing(post_job)
    equal(Path(json.loads(post_job.stdout)["temp"]), tmp_path / "existing temp")
    truth((expected_parent / "unrelated").is_file(), describing="unrelated files to survive")
    truth(
        (tmp_path / "existing temp" / "unrelated").is_file(),
        describing="files in the existing temp directory to survive",
    )
    if drive_present:
        truth(not chosen.exists(), describing="only this job's scratch to be removed")
    else:
        truth(chosen.exists(), describing="the shared fallback to remain")


@pytest.mark.parametrize("has_another_jobs_marker", [False, True])
def test_cleanup_refuses_a_foreign_jobs_directory(
    tmp_path: Path, has_another_jobs_marker: bool
) -> None:
    """A foreign path in the job environment is refused without removing it."""
    environment = _setup(tmp_path, drive_present=False)
    foreign = tmp_path / ("printobserver-" + "a" * 32)
    foreign.mkdir()
    if has_another_jobs_marker:
        (foreign / ".printobserver-owned").write_text(foreign.name, encoding="utf-8")
    retained = foreign / "unrelated"
    retained.write_text("keep", encoding="utf-8")
    environment["PRINTOBSERVER_JOB_SCRATCH"] = str(foreign)
    result = run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30)
    failing(result, naming="Refusing cleanup of unowned scratch")
    contains(result.stderr, "Refusing cleanup of unowned scratch")
    truth(retained.is_file(), describing="the foreign directory contents to survive")


@pytest.mark.parametrize("platform", ["windows-aarch64", ""])
def test_windows_arm_and_manual_dispatch_keep_existing_temp(tmp_path: Path, platform: str) -> None:
    """Only x86 opts in, even if another host has a writable scratch drive."""
    environment_file = tmp_path / "job-env"
    environment = dict(
        os.environ,
        PRINTOBSERVER_PLATFORM=platform,
        GITHUB_ENV=str(environment_file),
    )
    passing(run(["just", "runner-temp", "setup", str(tmp_path)], cwd=ROOT, env=environment))
    truth(
        not environment_file.exists(),
        describing="the existing environment to remain usable unchanged",
    )
    equal(list(tmp_path.iterdir()), [])


@pytest.mark.parametrize("defect", ["invalid-leaf", "missing-marker", "marker-content"])
def test_cleanup_checks_the_recorded_owners_directory_marker(tmp_path: Path, defect: str) -> None:
    """Matching the job token alone never authorizes a directory with invalid identity."""
    environment = _setup(tmp_path, drive_present=False)
    leaf = "wrong-leaf" if defect == "invalid-leaf" else "printobserver-" + "b" * 32
    scratch = tmp_path / leaf
    scratch.mkdir()
    retained = scratch / "unrelated"
    retained.write_text("keep", encoding="utf-8")
    if defect != "missing-marker":
        content = "another-owner" if defect == "marker-content" else leaf
        (scratch / ".printobserver-owned").write_text(content, encoding="utf-8")
    environment["PRINTOBSERVER_JOB_SCRATCH"] = str(scratch)
    environment["PRINTOBSERVER_JOB_SCRATCH_OWNER"] = leaf
    result = run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30)
    failing(result, naming="Refusing cleanup of unowned scratch")
    contains(result.stderr, "Clear the foreign PRINTOBSERVER_JOB_SCRATCH")
    truth(retained.is_file(), describing="the invalid scratch contents to survive")


def test_setup_requires_the_environment_file_for_later_children(tmp_path: Path) -> None:
    """Refuse a setup that could not export its selection to subsequent steps."""
    environment = dict(os.environ, PRINTOBSERVER_PLATFORM="windows-x86_64")
    environment.pop("GITHUB_ENV", None)
    result = run(["just", "runner-temp", "setup", str(tmp_path)], cwd=ROOT, env=environment)
    failing(result, naming="Set GITHUB_ENV to a writable environment file")
    equal(list(tmp_path.iterdir()), [])


@pytest.mark.parametrize("field", ["drive", "fallback", "environment"])
@pytest.mark.parametrize("line_break", ["\r", "\n"])
def test_environment_paths_cannot_inject_a_second_export(
    tmp_path: Path, field: str, line_break: str
) -> None:
    """Reject line delimiters before a path reaches the runner's environment file."""
    paths = {
        "drive": str(tmp_path / "drive"),
        "fallback": str(tmp_path),
        "environment": str(tmp_path / "job-env"),
    }
    paths[field] += line_break + "INJECTED=unsafe"
    environment = dict(
        os.environ,
        PRINTOBSERVER_PLATFORM="windows-x86_64",
        GITHUB_ENV=paths["environment"],
    )
    result = run(
        ["just", "runner-temp", "setup", paths["drive"], paths["fallback"]],
        cwd=ROOT,
        env=environment,
        timeout=30,
    )
    failing(result, naming="Paths must contain no line breaks")
    equal(list(tmp_path.iterdir()), [])


def test_an_unusable_fallback_is_refused_before_exports(tmp_path: Path) -> None:
    """No usable drive or existing fallback means no children inherit a broken temp."""
    environment_file = tmp_path / "job-env"
    environment = dict(
        os.environ,
        PRINTOBSERVER_PLATFORM="windows-x86_64",
        GITHUB_ENV=str(environment_file),
    )
    result = run(
        ["just", "runner-temp", "setup", str(tmp_path / "absent-drive"), str(tmp_path / "absent")],
        cwd=ROOT,
        env=environment,
        timeout=30,
    )
    failing(result, naming="Supply an existing writable fallback directory")
    truth(not environment_file.exists(), describing="no unusable temp to be exported")
    equal(list(tmp_path.iterdir()), [])


def test_an_unwritable_environment_file_cleans_only_its_new_scratch(tmp_path: Path) -> None:
    """A failed export does not leave the successfully created scratch behind."""
    environment = dict(
        os.environ,
        PRINTOBSERVER_PLATFORM="windows-x86_64",
        GITHUB_ENV=str(tmp_path),
    )
    retained = tmp_path / "unrelated"
    retained.write_text("keep", encoding="utf-8")
    result = run(["just", "runner-temp", "setup", str(tmp_path)], cwd=ROOT, env=environment)
    failing(result, naming="Set GITHUB_ENV to a writable environment file")
    equal(list(tmp_path.iterdir()), [retained])


ACL = """
param([string]$Action, [string]$Directory, [string]$Backup)
$ErrorActionPreference = 'Stop'
$acl = Get-Acl -LiteralPath $Directory
if ($Action -eq 'deny') {
    [IO.File]::WriteAllText($Backup, $acl.Sddl)
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    $rule = [Security.AccessControl.FileSystemAccessRule]::new(
        $identity, 'Write', 'ContainerInherit, ObjectInherit', 'None', 'Deny'
    )
    $acl.AddAccessRule($rule)
} else {
    $acl.SetSecurityDescriptorSddlForm([IO.File]::ReadAllText($Backup))
}
Set-Acl -LiteralPath $Directory -AclObject $acl
"""


def test_an_existing_unwritable_drive_recovers_to_the_usable_fallback(tmp_path: Path) -> None:
    """Real filesystem permissions refuse scratch creation; the job still has temp."""
    if sys.platform != "win32" and os.geteuid() == 0:
        pytest.skip("root bypasses the filesystem permission refusal this journey induces")
    drive = tmp_path / "drive"
    drive.mkdir()
    fallback = tmp_path / "fallback"
    fallback.mkdir()
    acl_script = tmp_path / "permissions.ps1"
    backup = tmp_path / "permissions.sddl"
    original_mode = drive.stat().st_mode & 0o777
    if sys.platform == "win32":
        acl_script.write_text(ACL, encoding="utf-8")
        passing(
            run(["pwsh", "-NoProfile", "-File", str(acl_script), "deny", str(drive), str(backup)])
        )
    else:
        drive.chmod(0o500)
    try:
        environment_file = tmp_path / "job-env"
        environment = dict(
            os.environ,
            PRINTOBSERVER_PLATFORM="windows-x86_64",
            GITHUB_ENV=str(environment_file),
        )
        result = run(
            ["just", "runner-temp", "setup", str(drive), str(fallback)],
            cwd=ROOT,
            env=environment,
            timeout=30,
        )
        passing(result)
        contains(result.stdout, "scratch creation failed:")
        contains(result.stdout, str(fallback))
        contains(
            environment_file.read_text(encoding="utf-8-sig"), f"TEMP={fallback}\nTMP={fallback}"
        )
        equal(list(drive.iterdir()), [])
    finally:
        if sys.platform == "win32":
            passing(
                run(
                    [
                        "pwsh",
                        "-NoProfile",
                        "-File",
                        str(acl_script),
                        "restore",
                        str(drive),
                        str(backup),
                    ]
                )
            )
        else:
            drive.chmod(original_mode)


@pytest.mark.parametrize("stage", ["marker", "probe"])
def test_file_creation_failure_removes_the_partially_created_scratch(
    tmp_path: Path, stage: str
) -> None:
    """A real native path limit permits the directory but refuses a longer file."""
    if sys.platform == "win32":
        pytest.skip("this journey induces the Unix PATH_MAX boundary, not a Windows path limit")
    limit = os.pathconf(tmp_path, "PC_PATH_MAX")
    drive = tmp_path / "drive"
    drive.mkdir()
    parent_length = limit - (60 if stage == "marker" else 80)
    while parent_length - len(str(drive)) >= 2:
        remaining = parent_length - len(str(drive)) - 1
        drive /= "d" * min(180, remaining)
        drive.mkdir()
    fallback = tmp_path / "fallback"
    fallback.mkdir()
    environment_file = tmp_path / "job-env"
    environment = dict(
        os.environ,
        PRINTOBSERVER_PLATFORM="windows-x86_64",
        GITHUB_ENV=str(environment_file),
    )
    result = run(
        ["just", "runner-temp", "setup", str(drive), str(fallback)],
        cwd=ROOT,
        env=environment,
        timeout=30,
    )
    if stage == "marker":
        passing(result)
        contains(result.stdout, "scratch creation failed:")
        contains(result.stdout, ".printobserver-owned")
        contains(
            environment_file.read_text(encoding="utf-8-sig"), f"TEMP={fallback}\nTMP={fallback}"
        )
    else:
        failing(result, naming="Supply an existing writable fallback directory")
        truth(not environment_file.exists(), describing="no unusable scratch to be exported")
    equal(list(drive.iterdir()), [], describing="the partially created scratch to be removed")


def test_the_host_selects_a_usable_fallback_when_the_recipe_omits_it(tmp_path: Path) -> None:
    """The production default comes from this real child's host temp environment."""
    fallback = tmp_path / "host temp"
    fallback.mkdir()
    environment_file = tmp_path / "job-env"
    environment = dict(
        os.environ,
        PRINTOBSERVER_PLATFORM="windows-x86_64",
        GITHUB_ENV=str(environment_file),
        TEMP=str(fallback),
        TMP=str(fallback),
        TMPDIR=str(fallback),
    )
    result = run(
        ["just", "runner-temp", "setup", str(tmp_path / "absent-drive")],
        cwd=ROOT,
        env=environment,
        timeout=30,
    )
    passing(result)
    equal(
        len((result.stdout + result.stderr).splitlines()), 1, describing="one useful success line"
    )
    for line in environment_file.read_text(encoding="utf-8-sig").splitlines():
        name, value = line.split("=", 1)
        environment[name] = value
    observed = run(["just", "--command", sys.executable, "-c", PROBE], cwd=ROOT, env=environment)
    passing(observed)
    answer = json.loads(observed.stdout)
    equal(Path(answer["temp"]), fallback)
    equal(Path(answer["file"]).parent, fallback)
    equal(answer["TEMP"], answer["TMP"])


@pytest.mark.parametrize("failure", ["unusable-fallback", "environment-file"])
def test_cleanup_keeps_scratch_when_post_job_temp_cannot_be_restored(
    tmp_path: Path, failure: str
) -> None:
    """Never remove the current temp before exporting usable temp for post-job children."""
    environment = _setup(tmp_path, drive_present=True)
    scratch = Path(environment["PRINTOBSERVER_JOB_SCRATCH"])
    if failure == "unusable-fallback":
        environment["PRINTOBSERVER_JOB_TEMP_FALLBACK"] = str(tmp_path / "absent")
        expected = "Supply an existing writable fallback directory"
    else:
        environment["GITHUB_ENV"] = str(tmp_path)
        expected = "Set GITHUB_ENV to a writable environment file"
    result = run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30)
    failing(result, naming=expected)
    truth(scratch.is_dir(), describing="the current job temp to remain usable after failed restore")
    environment["PRINTOBSERVER_JOB_TEMP_FALLBACK"] = str(tmp_path / "existing temp")
    environment["GITHUB_ENV"] = str(tmp_path / "job-env")
    passing(run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30))


@pytest.mark.parametrize("failure", ["marker-read", "scratch-delete"])
def test_cleanup_permission_failures_name_the_recovery(tmp_path: Path, failure: str) -> None:
    """Real Unix refusal over a long path retains the contiguous recovery message."""
    if sys.platform == "win32":
        pytest.skip("Unix read/delete permission induction; Windows uses different ACL semantics")
    if os.geteuid() == 0:
        pytest.skip("root bypasses the permission refusal this journey induces")
    case_root = tmp_path / "permission failure in a long scratch directory"
    case_root.mkdir()
    environment = _setup(case_root, drive_present=True)
    scratch = Path(environment["PRINTOBSERVER_JOB_SCRATCH"])
    marker = scratch / ".printobserver-owned"
    restricted = marker if failure == "marker-read" else scratch.parent
    original_mode = restricted.stat().st_mode & 0o777
    restricted.chmod(0 if failure == "marker-read" else 0o500)
    try:
        result = run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30)
        failing(
            result,
            naming="Restore access to the named job files and retry just runner-temp cleanup",
        )
        truth(scratch.exists(), describing="the directory the OS refused to clean to remain")
    finally:
        restricted.chmod(original_mode)
        # A failed recursive delete may have removed the marker before rmdir failed.
        marker.write_text(scratch.name, encoding="utf-8")
        passing(run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment, timeout=30))


def test_relative_inputs_export_absolute_temp_for_children_in_another_directory(
    tmp_path: Path,
) -> None:
    """The setup cwd cannot become an implicit part of a later child's temp identity."""
    drive = tmp_path / "drive"
    drive.mkdir()
    fallback = tmp_path / "fallback"
    fallback.mkdir()
    environment_file = tmp_path / "job-env"
    environment = dict(
        os.environ,
        PRINTOBSERVER_PLATFORM="windows-x86_64",
        GITHUB_ENV=str(environment_file),
    )
    script = str(ROOT / "scripts" / "runner-temp.ps1")
    passing(
        run(
            [
                "pwsh",
                "-NoProfile",
                "-File",
                script,
                "-Action",
                "setup",
                "-DriveRoot",
                "drive",
                "-Fallback",
                "fallback",
            ],
            cwd=tmp_path,
            env=environment,
        )
    )
    for line in environment_file.read_text(encoding="utf-8-sig").splitlines():
        name, value = line.split("=", 1)
        environment[name] = value
    try:
        chosen = Path(environment["TEMP"])
        truth(chosen.is_absolute(), describing="an absolute temp path independent of child cwd")
        observed = run([sys.executable, "-c", PROBE], cwd=fallback, env=environment)
        passing(observed)
        equal(Path(json.loads(observed.stdout)["file"]).parent, chosen)
    finally:
        passing(
            run(
                [
                    "pwsh",
                    "-NoProfile",
                    "-File",
                    script,
                    "-Action",
                    "cleanup",
                    "-DriveRoot",
                    "drive",
                ],
                cwd=tmp_path,
                env=environment,
            )
        )


@pytest.mark.parametrize("relative", [".", "child"])
def test_cleanup_refuses_to_restore_temp_inside_the_scratch_it_will_delete(
    tmp_path: Path, relative: str
) -> None:
    """A writable fallback must also survive the cleanup that follows its export."""
    environment = _setup(tmp_path, drive_present=True)
    scratch = Path(environment["PRINTOBSERVER_JOB_SCRATCH"])
    restored = scratch / relative
    restored.mkdir(exist_ok=True)
    environment["PRINTOBSERVER_JOB_TEMP_FALLBACK"] = str(restored)
    try:
        result = run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment)
        failing(result, naming="Choose a fallback outside this job's scratch")
        truth(scratch.is_dir(), describing="the currently usable scratch not to be removed")
    finally:
        if scratch.exists():
            environment["PRINTOBSERVER_JOB_TEMP_FALLBACK"] = str(tmp_path / "existing temp")
            passing(run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment))


def _directory_link(tmp_path: Path, alias: Path, target: Path) -> None:
    """Create a real host directory link, using a junction on Windows."""
    if sys.platform == "win32":
        link_script = tmp_path / "link.ps1"
        link_script.write_text(
            "param($Link, $Target)\n"
            "$ErrorActionPreference = 'Stop'\n"
            "New-Item -ItemType Junction -Path $Link -Target $Target | Out-Null\n",
            encoding="utf-8",
        )
        passing(run(["pwsh", "-NoProfile", "-File", str(link_script), str(alias), str(target)]))
    else:
        alias.symlink_to(target, target_is_directory=True)


@pytest.mark.parametrize("through_parent", [False, True])
def test_cleanup_refuses_an_outside_link_that_resolves_into_scratch(
    tmp_path: Path, through_parent: bool
) -> None:
    """A real symlink/junction's spelling cannot hide its deleted fallback target."""
    environment = _setup(tmp_path, drive_present=True)
    scratch = Path(environment["PRINTOBSERVER_JOB_SCRATCH"])
    target = scratch / "child"
    target.mkdir()
    alias = tmp_path / "outside alias"
    _directory_link(tmp_path, alias, scratch)
    environment["PRINTOBSERVER_JOB_TEMP_FALLBACK"] = str(
        alias / "child" if through_parent else alias
    )
    try:
        result = run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment)
        failing(result, naming="Choose a physical temp directory without symlinks or junctions")
        truth(scratch.is_dir(), describing="the live scratch to remain usable after refusal")
    finally:
        environment["PRINTOBSERVER_JOB_TEMP_FALLBACK"] = str(tmp_path / "existing temp")
        if scratch.exists():
            passing(run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment))
        if sys.platform == "win32":
            alias.rmdir()
        else:
            alias.unlink()


@pytest.mark.parametrize("selected", ["drive", "fallback"])
@pytest.mark.parametrize("through_parent", [False, True])
def test_setup_refuses_linked_temp_paths(
    tmp_path: Path, selected: str, through_parent: bool
) -> None:
    """A linked drive or fallback cannot receive a new job directory or export."""
    target = tmp_path / "physical"
    target.mkdir()
    (target / "child").mkdir()
    alias = tmp_path / "alias"
    _directory_link(tmp_path, alias, target)
    linked = alias / "child" if through_parent else alias
    environment = dict(os.environ, PRINTOBSERVER_PLATFORM="windows-x86_64")
    export = tmp_path / "exports"
    environment["GITHUB_ENV"] = str(export)
    try:
        result = run(
            [
                "just",
                "runner-temp",
                "setup",
                str(linked if selected == "drive" else target),
                str(linked if selected == "fallback" else target),
            ],
            cwd=ROOT,
            env=environment,
        )
        failing(result, naming="Choose a physical temp directory without symlinks or junctions")
        truth(not export.exists(), describing="no unusable temp exported")
        equal(sorted(p.name for p in target.iterdir()), ["child"], describing="no scratch created")
    finally:
        alias.rmdir() if sys.platform == "win32" else alias.unlink()


@pytest.mark.parametrize("through_parent", [False, True])
def test_cleanup_refuses_linked_scratch(tmp_path: Path, through_parent: bool) -> None:
    """A matching ownership marker cannot authorize deleting through a link."""
    environment = _setup(tmp_path, drive_present=True)
    scratch = Path(environment["PRINTOBSERVER_JOB_SCRATCH"])
    alias = tmp_path / ("alias" if through_parent else scratch.name)
    _directory_link(tmp_path, alias, scratch.parent if through_parent else scratch)
    environment["PRINTOBSERVER_JOB_SCRATCH"] = str(
        alias / scratch.name if through_parent else alias
    )
    try:
        result = run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment)
        failing(result, naming="Choose a physical temp directory without symlinks or junctions")
        truth(scratch.is_dir(), describing="the physical job directory to survive refusal")
    finally:
        environment["PRINTOBSERVER_JOB_SCRATCH"] = str(scratch)
        if scratch.exists():
            passing(run(["just", "runner-temp", "cleanup"], cwd=ROOT, env=environment))
        alias.rmdir() if sys.platform == "win32" else alias.unlink()
