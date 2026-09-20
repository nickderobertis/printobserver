"""The platform-dispatch workflow runs one job on one platform by hand, and nothing else.

Every journey here drives the committed check over a real copy of the committed
tree with one defect in it: a trigger that fires on a change, a third input, an
input offering a platform the list does not name or missing one it does, a job
option no source matrixes, a workflow with a job that neither resolves nor runs
the dispatch, a running job that runs somewhere other than where the resolving
job answered, and a source job carrying a shape the committed script cannot run
by hand. And the script itself is driven, over a copy of the tree and with real
programs standing in for the ones its steps run, through every shape it reads
off a source job: a teardown that runs after a failure, a step waived, a step
conditioned on the runner's family or on every other family, a step run under
PowerShell, and an environment reading another step's outcome, or choosing
between several by family.
"""

from __future__ import annotations

import io
import os
import re
import stat
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks import dispatching
from repo_checks.checks_ci import JobKind, continuous_integration, platforms, status_contexts
from repo_checks.checks_dispatch import platform_dispatch
from repo_checks.checks_integration import integration_tier
from repo_checks.checks_platforms import unmatrixed_jobs
from repo_checks.dispatching import REFUSED, Declared, DispatchError, matrixed_jobs, resolve
from repo_checks.expect import absent, accepted, contains, equal, refused, refused_naming, truth
from repo_checks.model import Repo
from repo_checks.parsing import load_workflow
from repo_checks.platforms import supported
from repo_checks.shell import run as shell_run
from treecopy import Tree

DISPATCH = ".github/workflows/platform-dispatch.yml"
CI = ".github/workflows/ci.yml"
INSTALL = ".github/workflows/install-path.yml"
POLICY = "repo-policy.toml"

#: The job whose steps carry every shape the executor reads: a waived step, two
#: steps conditioned on the runner's family, and a report reading their
#: outcomes on `always()`.
ROUTE = "install-route-pypi"


def test_the_committed_tree_is_accepted(committed: Repo) -> None:
    """The workflow hands its inputs to the script, and every source job is runnable."""
    accepted(platform_dispatch(committed))


def test_each_job_and_platform_choice_resolves_to_that_jobs_source_on_that_runner(
    committed: Repo,
) -> None:
    """What a dispatch runs, resolved by the script for every pair the inputs offer.

    For every job option and every platform option the committed workflow
    offers, the script names that job of its source workflow and the runner the
    supported-platform list declares for the platform — and refuses a pair
    outside them.
    """
    declared = Declared.read(committed)
    if isinstance(declared, str):
        raise AssertionError(declared)
    workflow = load_workflow(committed.path(f".github/workflows/{declared.workflow}"))
    inputs = workflow["on"]["workflow_dispatch"]["inputs"]
    sources = matrixed_jobs(committed, declared.sources)
    runners = {platform.id: platform.runner for platform in supported(committed)}

    for chosen in inputs[declared.job_input]["options"]:
        for platform in inputs[declared.platform_input]["options"]:
            resolved = resolve(committed, chosen, platform)
            equal(resolved.source.name, chosen, describing=f"the job `{chosen}` resolves to")
            equal(
                resolved.source.workflow,
                sources[chosen].workflow,
                describing=f"the workflow `{chosen}` is read from",
            )
            equal(
                resolved.platform.runner,
                runners[platform],
                describing=f"where `{chosen}` runs for `{platform}`",
            )
    with pytest.raises(DispatchError, match="no platform-matrixed job"):
        resolve(committed, "no-such-job", "linux-x86_64")
    with pytest.raises(
        DispatchError, match=re.escape("no platform AGENTS.md's supported-platform list")
    ):
        resolve(committed, "gate", "linux-riscv64")


def test_the_step_classifying_checks_read_the_workflow_as_an_ordinary_one(
    committed: Repo,
) -> None:
    """Its two jobs run the script and nothing a classifier reads, so nothing passes it over.

    Each of the two is read as a job of its own — the kind a classifier answers
    for a job whose steps run none of the commands it reads — so the matrix
    rule, the gate rule, the unmatrixed-job record and the integration tier
    all accept the committed tree with this workflow read like any other.
    """
    accepted(platforms(committed), describing="the matrix rule")
    accepted(continuous_integration(committed), describing="the gate rule")
    accepted(unmatrixed_jobs(committed), describing="the unmatrixed-job record")
    accepted(integration_tier(committed), describing="the integration tier")
    equal(
        sorted(
            (context.job, context.kind)
            for context in status_contexts(committed)
            if context.file == DISPATCH.rpartition("/")[2]
        ),
        [("run", JobKind.OTHER), ("select", JobKind.OTHER)],
        describing="the two jobs read off the dispatch workflow, and what each is taken for",
    )


def test_a_trigger_that_fires_on_a_change_is_refused(tree: Callable[[], Tree]) -> None:
    """A workflow that ran the gate on a pull request too would be a second gate."""
    broken = tree()
    broken.edit(
        DISPATCH, "on:\n  workflow_dispatch:\n", "on:\n  pull_request:\n  workflow_dispatch:\n"
    )

    refused_naming(platform_dispatch(broken.repo), "fires on `pull_request`", "second gate")


def test_a_workflow_with_no_manual_dispatch_is_refused(tree: Callable[[], Tree]) -> None:
    """A dispatch workflow nothing can dispatch is nothing."""
    broken = tree()
    text = broken.read(DISPATCH)
    start, end = text.index("on:\n  workflow_dispatch:\n"), text.index("permissions:\n")
    broken.write(DISPATCH, f"{text[:start]}on:\n  pull_request:\n\n{text[end:]}")

    refused(platform_dispatch(broken.repo), "declares no `workflow_dispatch` trigger")


def test_a_third_input_is_refused(tree: Callable[[], Tree]) -> None:
    """A dispatch names a job and a platform, and nothing else."""
    broken = tree()
    broken.edit(
        DISPATCH,
        "      platform:\n",
        "      version:\n        description: A version.\n        required: false\n"
        "        type: string\n      platform:\n",
    )

    refused_naming(platform_dispatch(broken.repo), "takes a `version` input")


def test_a_platform_input_offering_a_platform_the_list_does_not_name_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An option no supported platform backs resolves to no runner."""
    broken = tree()
    broken.edit(
        DISPATCH,
        "          - macos-aarch64\n",
        "          - macos-aarch64\n          - linux-riscv64\n",
    )

    refused_naming(platform_dispatch(broken.repo), "`platform` input offers `linux-riscv64`")


def test_a_platform_input_missing_a_platform_the_list_names_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Every supported platform is one a dispatch can name."""
    broken = tree()
    broken.edit(DISPATCH, "          - macos-aarch64\n", "")

    refused_naming(
        platform_dispatch(broken.repo), "`platform` input does not offer `macos-aarch64`"
    )


def test_a_job_input_offering_a_job_no_source_matrixes_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A job the script cannot resolve is one a caller should not be offered."""
    broken = tree()
    broken.edit(DISPATCH, "          - gate\n", "          - gate\n          - llmlint\n")

    refused_naming(platform_dispatch(broken.repo), "`job` input offers `llmlint`")


def test_a_job_two_sources_both_matrix_is_refused_before_anything_runs(
    tree: Callable[[], Tree], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """One name in two sources is ambiguous, and order is no way to resolve it.

    A dispatch names a job by name alone, so a name both source workflows
    carry would run whichever the sources list first and say nothing about the
    other. The check refuses the tree, the resolver refuses the pair, and the
    script exits refused with nothing run — rather than any of them picking one.
    """
    broken = tree()
    broken.edit(INSTALL, "\n  install-route-script:\n", "\n  gate:\n")
    recording = Recording(tmp_path)
    recording.program(JUST)

    refused_naming(
        platform_dispatch(broken.repo), "`gate` is a platform-matrixed job of both ci.yml and"
    )
    with pytest.raises(DispatchError, match=re.escape("both ci.yml and install-path.yml")):
        resolve(broken.repo, "gate", "linux-x86_64")
    code = dispatching.main(
        ["resolve", "--job", "gate", "--platform", "linux-x86_64", "--root", str(broken.root)]
    )
    equal(code, REFUSED, describing="the exit of `resolve` over an ambiguous job")
    contains(capsys.readouterr().err, "refused: `gate` is a platform-matrixed job of both")
    equal(recording.recorded(), [], describing="what ran before the refusal")


def test_a_job_input_missing_a_matrixed_job_of_a_source_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Every platform-matrixed job of a source is one a dispatch can name."""
    broken = tree()
    broken.edit(DISPATCH, "          - integration\n", "")

    refused_naming(platform_dispatch(broken.repo), "`job` input does not offer `integration`")


def test_an_input_that_is_not_required_is_refused(tree: Callable[[], Tree]) -> None:
    """A dispatch naming no job would run on a default nobody chose."""
    broken = tree()
    broken.edit(
        DISPATCH,
        "      job:\n        description: The job to run, by its name in `ci.yml` or "
        "`install-path.yml`.\n        required: true\n",
        "      job:\n        description: The job to run, by its name in `ci.yml` or "
        "`install-path.yml`.\n        required: false\n",
    )

    refused_naming(platform_dispatch(broken.repo), "`job` input is not required")


def test_an_input_that_is_not_a_choice_is_refused(tree: Callable[[], Tree]) -> None:
    """A free-text platform is one the list may not name."""
    broken = tree()
    text = broken.read(DISPATCH)
    start = text.index("      platform:\n")
    end = text.index("permissions:\n")
    broken.write(
        DISPATCH,
        f"{text[:start]}      platform:\n        required: true\n        type: string\n\n"
        f"{text[end:]}",
    )

    refused_naming(platform_dispatch(broken.repo), "declares no `platform` input of type `choice`")


def test_a_job_that_neither_resolves_nor_runs_the_dispatch_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A third job is a job the dispatch did not name."""
    broken = tree()
    broken.append(
        DISPATCH,
        "\n  extra:\n    name: extra\n    runs-on: ubuntu-24.04\n    steps:\n"
        "      - run: echo extra\n",
    )

    refused_naming(platform_dispatch(broken.repo), "job `extra`", "runs one job and nothing else")


def test_a_running_job_that_runs_elsewhere_than_the_answer_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A `runs-on` of its own is a second place that decides where a dispatch runs."""
    broken = tree()
    broken.edit(
        DISPATCH, "    runs-on: ${{ needs.select.outputs.runner }}\n", "    runs-on: macos-15\n"
    )

    refused_naming(platform_dispatch(broken.repo), "job `run`", "does not run on")


def test_a_running_job_that_does_not_need_the_resolving_one_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Without `needs`, the answer it runs on is not there to read."""
    broken = tree()
    broken.edit(DISPATCH, "    needs: select\n", "")

    refused_naming(platform_dispatch(broken.repo), "does not need job `select`")


def test_a_resolving_job_publishing_no_runner_is_refused(tree: Callable[[], Tree]) -> None:
    """An answer nobody publishes is one the running job cannot run on."""
    broken = tree()
    broken.edit(DISPATCH, "    outputs:\n      runner: ${{ steps.resolve.outputs.runner }}\n", "")

    refused_naming(platform_dispatch(broken.repo), "job `select` publishes no `runner` output")


def test_a_script_not_handed_both_inputs_is_refused(tree: Callable[[], Tree]) -> None:
    """The script decides from the two inputs, so each reaches it as its own variable."""
    broken = tree()
    broken.edit(
        DISPATCH,
        '        run: just dispatch-run "$JOB" "$PLATFORM"\n',
        '        run: just dispatch-run "$JOB" macos-aarch64\n',
    )

    refused_naming(platform_dispatch(broken.repo), "job `run` runs", "handed the two inputs")


def test_a_script_handed_something_other_than_an_input_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A variable carrying a literal is a platform nobody dispatched."""
    broken = tree()
    text = broken.read(DISPATCH)
    broken.write(
        DISPATCH,
        text.replace(
            "          PLATFORM: ${{ inputs.platform }}\n", "          PLATFORM: macos-aarch64\n"
        ),
    )

    refused_naming(
        platform_dispatch(broken.repo), "does not hand it `inputs.platform` as `PLATFORM`"
    )


def test_a_conditioned_or_matrixed_dispatch_job_is_refused(tree: Callable[[], Tree]) -> None:
    """The one job runs unconditionally on its one runner."""
    broken = tree()
    broken.edit(
        DISPATCH, "    needs: select\n", "    needs: select\n    if: inputs.job != 'gate'\n"
    )

    refused_naming(platform_dispatch(broken.repo), "job `run` carries a condition or a matrix")


def test_a_source_job_the_script_cannot_run_is_refused_where_it_is_written(
    tree: Callable[[], Tree],
) -> None:
    """A step conditioned on something a dispatch by hand cannot read is refused at the source."""
    broken = tree()
    broken.edit(
        CI,
        "      - run: just test-integration\n",
        "      - if: github.event_name == 'push'\n        run: just test-integration\n",
    )

    refused_naming(
        platform_dispatch(broken.repo),
        "cannot run ci.yml's job `integration` by hand",
        "conditioned on `github.event_name == 'push'`",
    )


@pytest.mark.parametrize(
    ("old", "new", "why"),
    [
        (
            "      - run: just check\n",
            "      - run: just check\n        shell: cmd\n",
            "runs under `cmd`",
        ),
        (
            "      - run: just check\n",
            "      - run: just check\n        if: runner.os == 'Windows' || always()\n",
            "conditioned on `runner.os == 'Windows' || always()`",
        ),
        (
            "      - run: just check\n",
            "      - run: just check\n        env:\n"
            "          WHEN: ${{ runner.os == 'Linux' && steps.a.outcome || 'never' }}\n",
            "choice between outcomes",
        ),
        (
            "      - run: just check\n",
            "      - run: just check\n        env:\n          WHEN: ${{ github.sha }}\n",
            "reads a step's environment as a literal",
        ),
        (
            "      - run: just check\n",
            "      - run: just check\n        continue-on-error: ${{ true }}\n",
            "something other than a bool",
        ),
        (
            "      - run: just check\n",
            "      - name: nothing\n",
            "neither runs a command nor uses an action",
        ),
        (
            "      - run: just check\n",
            "      - run: just check\n        if: [always]\n",
            "an `if` that is not a string",
        ),
        (
            "      - run: just check\n",
            "      - run: just check\n        env: [A]\n",
            "an `env` that is not a mapping",
        ),
        (
            "      - run: just check\n",
            "      - run: just check\n        env:\n          WHEN: [now]\n",
            "sets `WHEN` to something other than a scalar",
        ),
        (
            "  gate:\n    env:\n      PRINTOBSERVER_PLATFORM: ${{ matrix.platform.id }}\n",
            "  gate:\n    env: [A]\n",
            "an `env` that is not a mapping",
        ),
        (
            "  gate:\n    env:\n",
            "  gate:\n    env:\n      WHEN: [now]\n",
            "sets `WHEN` to something other than a scalar",
        ),
    ],
    ids=[
        "shell",
        "compound-condition",
        "choice-not-of-outcomes",
        "expression-env",
        "expression-waiver",
        "no-command",
        "if-not-string",
        "env-not-mapping",
        "env-not-scalar",
        "job-env-not-mapping",
        "job-env-not-scalar",
    ],
)
def test_each_shape_the_script_cannot_run_is_refused(
    tree: Callable[[], Tree], old: str, new: str, why: str
) -> None:
    """Every refusal the executor makes names the shape, and the check surfaces it."""
    broken = tree()
    broken.edit(CI, old, new)

    refused_naming(platform_dispatch(broken.repo), "cannot run ci.yml's job `gate` by hand", why)


def test_a_declaration_that_is_absent_is_refused(tree: Callable[[], Tree]) -> None:
    """With no declaration, nothing says which workflow runs one cell by hand."""
    broken = tree()
    text = broken.read(POLICY)
    start, end = text.index("[dispatch]\n"), text.index("[agent]\n")
    broken.write(POLICY, text[:start] + text[end:])

    refused(platform_dispatch(broken.repo), "declares no `[dispatch]` section")


def test_a_declaration_missing_a_key_is_refused(tree: Callable[[], Tree]) -> None:
    """Each key names one thing the check reads, and none has a default."""
    broken = tree()
    broken.edit(POLICY, 'run_recipe = "dispatch-run"\n', "")

    refused_naming(platform_dispatch(broken.repo), "`[dispatch]` declares no run_recipe")


def test_a_declaration_naming_no_committed_workflow_is_refused(tree: Callable[[], Tree]) -> None:
    """A workflow that is not there dispatches nothing."""
    broken = tree()
    broken.edit(POLICY, 'workflow = "platform-dispatch.yml"', 'workflow = "no-such-dispatch.yml"')

    refused_naming(platform_dispatch(broken.repo), "no-such-dispatch.yml", "which is not there")


def test_a_source_that_is_not_there_is_refused(tree: Callable[[], Tree]) -> None:
    """A source nobody commits has no jobs to dispatch."""
    broken = tree()
    broken.edit(
        POLICY, 'sources = ["ci.yml", "install-path.yml"]', 'sources = ["ci.yml", "gone.yml"]'
    )

    refused_naming(platform_dispatch(broken.repo), "gone.yml", "commits no such workflow")


def test_a_declaration_naming_the_workflow_as_its_own_source_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow cannot dispatch one of its own jobs."""
    broken = tree()
    broken.edit(
        POLICY,
        'sources = ["ci.yml", "install-path.yml"]',
        'sources = ["ci.yml", "platform-dispatch.yml"]',
    )

    refused_naming(platform_dispatch(broken.repo), "as its own source")


def test_a_tree_with_no_platform_list_is_refused(tree: Callable[[], Tree]) -> None:
    """Without the list there is no runner to resolve a platform to."""
    broken = tree()
    broken.edit("AGENTS.md", "[//]: # (BEGIN supported-platforms)", "")

    refused_naming(platform_dispatch(broken.repo), "supported-platforms")


#: The one program every step of the gate and the integration tier runs.
JUST = "just"


class Recording:
    """Real programs standing in for the ones a source job's steps run.

    Each records the arguments it was given to a file, and exits as told: the
    steps then run for real under bash, through the executor, against
    programs that answer the way the real ones can.
    """

    def __init__(self, root: Path) -> None:
        """A directory of stand-ins, and the record they write."""
        self.programs = root / "programs"
        self.programs.mkdir()
        self.record = root / "record.txt"
        self.failing_on = ""

    def program(self, name: str, *, failing_on: str = "", exit_code: int = 7) -> None:
        """One stand-in that records `name` and its arguments, and fails on one argument.

        A shell script, which is what a step under `bash` runs on every host;
        on Windows a batch twin beside it too, because a step under `pwsh`
        there runs a program by a suffix `PATHEXT` names and a bare script by
        none.
        """
        script = self.programs / name
        script.write_text(
            "#!/bin/sh\n"
            f'echo "{name} $*" >> "$RECORD"\n'
            f'if [ -n "$FAIL_ON" ] && [ "$1" = "$FAIL_ON" ]; then exit {exit_code}; fi\n'
            "exit 0\n",
            encoding="utf-8",
        )
        script.chmod(script.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        if sys.platform == "win32":
            (self.programs / f"{name}.cmd").write_text(
                "@echo off\r\n"
                f'echo {name} %*>> "%RECORD%"\r\n'
                f'if not "%FAIL_ON%"=="" if "%~1"=="%FAIL_ON%" exit /b {exit_code}\r\n'
                "exit /b 0\r\n",
                encoding="utf-8",
            )
        self.failing_on = failing_on

    def pipe_reader(self, name: str, *, exit_code: int = 0) -> None:
        """One stand-in that reads its pipe out before it records, and exits as told.

        What `sh` and `sudo sh` do with the script `curl` hands them: read it
        before running it — so the record carries the order the commands
        finished in rather than the order a pipeline started them.
        """
        script = self.programs / name
        script.write_text(
            "#!/bin/bash\n"
            "if [ -p /dev/stdin ]; then cat > /dev/null; fi\n"
            f'echo "{name}${{*:+ $*}}" >> "$RECORD"\n'
            f"exit {exit_code}\n",
            encoding="utf-8",
        )
        script.chmod(script.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    #: What the stand-in profile defines: `irm`, `iex` and `Set-Service`, each
    #: recording what it was given. The fetch answers its own address as the
    #: script it fetched, the service installer and the start refuse as they do
    #: unattended, and anything else runs.
    POWERSHELL_PROFILE = (
        "Remove-Item Alias:irm -Force\n"
        "Remove-Item Alias:iex -Force\n"
        "function irm { param([string]$Uri)"
        ' Add-Content -LiteralPath $env:RECORD -Value "irm $Uri"; $Uri }\n'
        "function iex { param([Parameter(ValueFromPipeline)]$Command) process {"
        ' Add-Content -LiteralPath $env:RECORD -Value "iex $Command";'
        " if ($Command -like '*install-service*') { throw 'cannot run unattended' } } }\n"
        "function Set-Service {"
        ' Add-Content -LiteralPath $env:RECORD -Value "Set-Service $args";'
        " throw 'no service control manager here' }\n"
    )

    def powershell_commands(self) -> None:
        """Stand-ins for the commands the Windows pair runs, in PowerShell's own vocabulary.

        `irm` and `iex` are aliases and `Set-Service` a cmdlet, none of them a
        program on a path, so they are stood in for where PowerShell reads its
        commands from: a profile of this recording's own, which `pwsh -command`
        loads exactly as it does for a caller at a prompt. Written where each
        host's PowerShell reads one — under the configuration directory on
        Linux and macOS, under the profile's documents on Windows — with the
        environment `environment` hands every step naming this recording's
        root as both; and PowerShell is asked, under that environment, where
        it will read its profile from, so a host that reads it from somewhere
        this recording does not own is refused rather than run against the
        caller's own profile.

        Raises:
            AssertionError: If this host's PowerShell reads its profile from
                outside this recording's root.
        """
        root = self.programs.parent
        for directory in (root / "config" / "powershell", root / "Documents" / "PowerShell"):
            directory.mkdir(parents=True)
            (directory / "Microsoft.PowerShell_profile.ps1").write_text(
                self.POWERSHELL_PROFILE, encoding="utf-8"
            )
        asked = shell_run(
            ["pwsh", "-NoProfile", "-Command", "$PROFILE.CurrentUserCurrentHost"],
            cwd=root,
            env=self.environment(),
            timeout=60,
        )
        reads = Path(asked.stdout.strip())
        truth(
            asked.returncode == 0 and reads.is_file() and reads.is_relative_to(root),
            describing=f"pwsh to read its profile from under {root}, not from `{reads}`",
        )

    def environment(self, **extra: str) -> dict[str, str]:
        """An environment finding the stand-ins first, and telling them what to record."""
        environment = dict(os.environ)
        environment["PATH"] = os.pathsep.join([str(self.programs), environment.get("PATH", "")])
        environment["RECORD"] = str(self.record)
        environment["FAIL_ON"] = self.failing_on
        # A home of this recording's own, and where `pwsh` reads its profile
        # from relative to one on each host: the configuration directory on
        # Linux and macOS, the profile's documents on Windows.
        root = self.programs.parent
        environment["HOME"] = str(root)
        environment["USERPROFILE"] = str(root)
        environment["XDG_CONFIG_HOME"] = str(root / "config")
        environment.update(extra)
        return environment

    def recorded(self) -> list[str]:
        """Every invocation, in order."""
        if not self.record.is_file():
            return []
        return self.record.read_text(encoding="utf-8").splitlines()


def _run(
    tree: Tree, job: str, platform: str, recording: Recording, **extra: str
) -> tuple[int, str]:
    """Run one pair through the script's executor, as the workflow's `run` job does."""
    resolved = resolve(tree.repo, job, platform)
    said = io.StringIO()
    code = dispatching.execute(
        resolved,
        cwd=tree.root,
        environment=recording.environment(**extra),
        say=lambda line: print(line, file=said),
    )
    return code, said.getvalue()


def test_the_gate_runs_exactly_its_own_commands(tree: Callable[[], Tree], tmp_path: Path) -> None:
    """A dispatch of the gate runs the gate's two commands and nothing else."""
    copy = tree()
    recording = Recording(tmp_path)
    recording.program(JUST)

    code, said = _run(copy, "gate", "linux-aarch64", recording)

    equal(code, 0, describing=f"the exit of a gate whose commands all passed:\n{said}")
    equal(recording.recorded(), ["just bootstrap", "just check"], describing="what ran")


def test_a_teardown_runs_after_a_failure_and_the_exit_is_the_failures(
    tree: Callable[[], Tree], tmp_path: Path
) -> None:
    """The integration tier's `always()` bring-down runs after a failing tier, and the job fails."""
    copy = tree()
    recording = Recording(tmp_path)
    recording.program(JUST, failing_on="test-integration", exit_code=9)

    code, said = _run(copy, "integration", "macos-aarch64", recording)

    equal(code, 9, describing="the exit of a tier that failed")
    equal(
        recording.recorded(),
        ["just bootstrap", "just octoprint-up", "just test-integration", "just octoprint-down"],
        describing="what ran",
    )
    contains(said, "`just test-integration` failed with 9", describing=said)


def test_a_failure_stops_the_steps_after_it_that_are_not_always(
    tree: Callable[[], Tree], tmp_path: Path
) -> None:
    """A failed bring-up runs no tier, and still runs the bring-down."""
    copy = tree()
    recording = Recording(tmp_path)
    recording.program(JUST, failing_on="octoprint-up")

    code, _ = _run(copy, "integration", "linux-x86_64", recording)

    equal(code, 7, describing="the exit of a tier whose bring-up failed")
    equal(
        recording.recorded(),
        ["just bootstrap", "just octoprint-up", "just octoprint-down"],
        describing="what ran",
    )


#: What the shell managers' pair records on each family: the installer read
#: down its pipe, then that family's own start command.
SHELL_PAIR = {
    "systemctl": ["sudo sh", "sudo systemctl enable --now printobserver.service"],
    "launchctl": [
        "sudo sh",
        "sudo launchctl bootstrap system /Library/LaunchDaemons/"
        "io.github.nickderobertis.printobserver.plist",
    ],
}
SHELL_INSTALLER = (
    "curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
    "scripts/install-service.sh"
)
#: What the Windows pair records: the installer fetched and run, then the start.
WINDOWS_INSTALLER = (
    "https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
    "scripts/install-service.ps1"
)
WINDOWS_PAIR = [
    f"irm {WINDOWS_INSTALLER}",
    f"iex {WINDOWS_INSTALLER}",
    "Set-Service -Name printobserver -StartupType Automatic -Status Running",
]


@pytest.mark.parametrize(
    ("platform", "pair"),
    [
        ("linux-x86_64", [SHELL_INSTALLER, *SHELL_PAIR["systemctl"]]),
        ("macos-aarch64", [SHELL_INSTALLER, *SHELL_PAIR["launchctl"]]),
        ("windows-x86_64", WINDOWS_PAIR),
    ],
    ids=["systemd", "launchd", "windows-service"],
)
def test_an_install_route_runs_its_familys_steps_waiving_and_reporting_what_it_may(
    tree: Callable[[], Tree], tmp_path: Path, platform: str, pair: list[str]
) -> None:
    """The route job's waived steps fail without failing it, and the report reads their outcomes.

    Each family's pair runs on its own family alone — the shell installer on
    every family but Windows, each start command on its own, and the
    PowerShell pair under `pwsh` on Windows — the installer and the start are
    waived, and the summary the source writes on `always()` carries both
    outcomes, read through the `runner.os` choices between the pairs' steps as
    the source spells them.
    """
    copy = tree()
    recording = Recording(tmp_path)
    for name in ("pip", "printobserver", "curl"):
        recording.program(name)
    recording.powershell_commands()
    # `sudo` fails whatever it is asked: neither the installer nor the start
    # command can succeed unattended, which is why the source waives them.
    recording.pipe_reader("sudo", exit_code=1)
    summary = tmp_path / "summary.md"

    code, said = _run(copy, ROUTE, platform, recording, GITHUB_STEP_SUMMARY=str(summary))

    equal(code, 0, describing=f"the exit of a route whose only failures are waived:\n{said}")
    equal(
        recording.recorded(),
        ["pip install printobserver-cli", "printobserver --version", *pair],
        describing="what ran",
    )
    contains(said, "and its failure is waived", describing=said)
    equal(
        summary.read_text(encoding="utf-8"),
        f"{ROUTE}: service installation failure, service startup failure\n",
        describing="the summary the report step wrote",
    )


@pytest.mark.parametrize(
    ("platform", "taken"),
    [
        (
            "linux-aarch64",
            [
                "curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
                "scripts/install.sh",
                "sh",
                "printobserver --version",
            ],
        ),
        (
            "windows-aarch64",
            [
                "irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
                "scripts/install.ps1",
                "iex https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
                "scripts/install.ps1",
                "printobserver --version",
            ],
        ),
    ],
    ids=["shell-form", "powershell-form"],
)
def test_the_script_route_takes_the_form_of_the_script_its_family_runs(
    tree: Callable[[], Tree], tmp_path: Path, platform: str, taken: list[str]
) -> None:
    """A step on every family but one runs there and not on that one, and `pwsh` runs its own.

    The shell form's fetch and check are conditioned on `runner.os !=
    'Windows'`, so a Windows dispatch skips both and runs the PowerShell form's
    one step — two lines under `pwsh`, the fetch and the check in one session
    — and a Linux dispatch does the reverse. What each recorded is what its
    family's caller runs, and nothing of the other's.
    """
    copy = tree()
    recording = Recording(tmp_path)
    for name in ("printobserver", "curl"):
        recording.program(name)
    recording.pipe_reader("sh")
    recording.pipe_reader("sudo", exit_code=1)
    recording.powershell_commands()
    summary = tmp_path / "summary.md"

    code, said = _run(
        copy, "install-route-script", platform, recording, GITHUB_STEP_SUMMARY=str(summary)
    )

    equal(code, 0, describing=f"the exit of a route whose only failures are waived:\n{said}")
    equal(recording.recorded()[: len(taken)], taken, describing="how the route was taken")
    contains(
        summary.read_text(encoding="utf-8"),
        "install-route-script: service installation failure, service startup failure",
        describing="the summary the report step wrote",
    )


def test_a_powershell_step_fails_the_job_the_way_a_shell_one_does(
    tree: Callable[[], Tree], tmp_path: Path
) -> None:
    """An unwaived `pwsh` step that fails fails the job, and stops the steps after it.

    The check inside the PowerShell form's one step is a program on the path,
    run from `pwsh` as a caller who pasted the command and typed the check
    runs it; when it fails, the pair after it does not run and the report on
    `always()` says neither reached.
    """
    copy = tree()
    recording = Recording(tmp_path)
    recording.program("printobserver", failing_on="--version", exit_code=5)
    recording.pipe_reader("sudo", exit_code=1)
    recording.powershell_commands()
    summary = tmp_path / "summary.md"

    code, said = _run(
        copy, "install-route-script", "windows-x86_64", recording, GITHUB_STEP_SUMMARY=str(summary)
    )

    truth(code != 0, describing=f"the exit of a route whose check failed:\n{said}")
    equal(
        recording.recorded()[-1],
        "printobserver --version",
        describing="the last thing that ran before the failure stopped the job",
    )
    contains(said, "failed with", describing=said)
    contains(
        summary.read_text(encoding="utf-8"),
        "install-route-script: service installation skipped, service startup skipped",
        describing="the summary the report step wrote",
    )


def test_a_job_level_literal_environment_is_carried_and_an_expression_is_not(
    tree: Callable[[], Tree], tmp_path: Path
) -> None:
    """The registry proofs' version is an expression, so a dispatch proves the newest."""
    copy = tree()
    version = (
        "      PRINTOBSERVER_PROOF_VERSION: "
        "${{ inputs.version || needs.resolve.outputs.version }}\n"
    )
    copy.edit(INSTALL, version, f"{version}      PROOF_NOTE: literal\n")
    recording = Recording(tmp_path)
    (tmp_path / "programs" / JUST).write_text(
        '#!/bin/sh\necho "just $* version=${PRINTOBSERVER_PROOF_VERSION-unset} '
        'note=${PROOF_NOTE-unset}" >> "$RECORD"\n',
        encoding="utf-8",
    )
    (tmp_path / "programs" / JUST).chmod(0o755)

    code, _ = _run(copy, "prove-registry-pypi", "linux-x86_64", recording)

    equal(code, 0, describing="the exit of a proof that passed")
    equal(
        recording.recorded(),
        ["just prove-registry-pypi version=unset note=literal"],
        describing="what ran, and what it was handed",
    )


def test_the_script_refuses_an_unknown_pair_before_anything_runs(
    tree: Callable[[], Tree], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Both verbs refuse, exit 2, naming what can be named — and nothing has run."""
    copy = tree()
    recording = Recording(tmp_path)
    recording.program(JUST)

    for verb in ("resolve", "run"):
        code = dispatching.main(
            [verb, "--job", "gate", "--platform", "linux-riscv64", "--root", str(copy.root)]
        )
        equal(code, REFUSED, describing=f"the exit of `{verb}` over an unknown platform")
        contains(capsys.readouterr().err, "refused: `linux-riscv64` is no platform")
    equal(recording.recorded(), [], describing="what ran before the refusal")


def test_the_script_answers_the_runner_and_the_source(
    committed: Repo, capsys: pytest.CaptureFixture[str]
) -> None:
    """`resolve` prints what the workflow's `select` job appends to its outputs."""
    code = dispatching.main(
        [
            "resolve",
            "--job",
            "prove-registry-npm",
            "--platform",
            "macos-aarch64",
            "--root",
            str(committed.root),
        ]
    )

    equal(code, 0, describing="the exit of a pair the inputs offer")
    equal(
        capsys.readouterr().out,
        "runner=macos-15\nsource=install-path.yml\n",
        describing="what `resolve` printed",
    )


def test_a_source_the_script_cannot_run_is_refused_by_run_too(
    tree: Callable[[], Tree], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The executor refuses the same shape the check refuses, before running any step."""
    copy = tree()
    copy.edit(
        CI,
        "      - run: just check\n",
        "      - run: just check\n        working-directory: crates\n",
    )
    recording = Recording(tmp_path)
    recording.program(JUST)

    code = dispatching.main(
        ["run", "--job", "gate", "--platform", "linux-x86_64", "--root", str(copy.root)]
    )

    equal(code, REFUSED, describing="the exit of a run over a source it cannot run")
    contains(capsys.readouterr().err, "carries `working-directory`")
    truth(not recording.recorded(), describing="nothing to have run")
    absent(recording.recorded(), "just bootstrap", describing="what ran")
