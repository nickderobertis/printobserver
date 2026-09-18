"""The platform-dispatch workflow runs one job on one platform by hand, and nothing else.

Every journey here drives the committed check over a real copy of the committed
tree with one defect in it: a trigger that fires on a change, an input offering
a platform the list does not name or missing one it does, a job option nothing
selects or one selected twice, a runner the list does not declare, a copy whose
steps drifted from its source, and a copy carrying what a single cell of its
source does not. And the checks that classify a job by its steps are shown to
pass the copies over, because read as jobs of their own they would be a second
gate and a second integration tier with no matrix.
"""

from __future__ import annotations

import json
from collections.abc import Callable

from repo_checks.checks_ci import continuous_integration, platforms, status_contexts
from repo_checks.checks_dispatch import Declared, dispatch_workflow, platform_dispatch
from repo_checks.checks_integration import integration_tier
from repo_checks.checks_platforms import unmatrixed_jobs
from repo_checks.expect import accepted, equal, refused, refused_naming, truth
from repo_checks.model import Repo
from repo_checks.parsing import jobs_of, load_workflow
from treecopy import Tree

DISPATCH = ".github/workflows/platform-dispatch.yml"
CI = ".github/workflows/ci.yml"
INSTALL = ".github/workflows/install-path.yml"
POLICY = "repo-policy.toml"

#: The one map every job resolves the platform named to a runner through.
RUNNERS = (
    '{"linux-x86_64":"ubuntu-24.04","linux-aarch64":"ubuntu-24.04-arm",'
    '"macos-aarch64":"macos-15","windows-x86_64":"windows-2025",'
    '"windows-aarch64":"windows-11-arm"}'
)


def test_the_committed_tree_is_accepted(committed: Repo) -> None:
    """Every copy agrees with its source, and both inputs offer exactly what they should."""
    accepted(platform_dispatch(committed))


def test_each_job_and_platform_choice_resolves_to_that_jobs_steps_on_that_runner(
    committed: Repo,
) -> None:
    """What a dispatch runs, read off the committed workflow the way GitHub reads it.

    For every job option and every platform option: exactly one job's condition
    is met, its runner is the one the supported-platform list declares for the
    platform, and its steps are the source job's own. The resolution is read
    here rather than trusted to the check, so the two agree by construction.
    """
    from repo_checks.checks_dispatch import RUNNER_MAP, SELECTION, matrixed_jobs
    from repo_checks.platforms import supported

    declared = Declared.read(committed)
    if isinstance(declared, str):
        raise AssertionError(declared)
    workflow = load_workflow(committed.path(f".github/workflows/{declared.workflow}"))
    inputs = workflow["on"]["workflow_dispatch"]["inputs"]
    jobs = jobs_of(workflow)
    sources = matrixed_jobs(committed, declared.sources)
    runners = {platform.id: platform.runner for platform in supported(committed)}

    for chosen in inputs[declared.job_input]["options"]:
        for platform in inputs[declared.platform_input]["options"]:
            selected = [
                name
                for name, job in jobs.items()
                if (found := SELECTION.match(str(job.get("if", ""))))
                and found["job"] == chosen
                and found["input"] == declared.job_input
            ]
            equal(selected, [chosen], describing=f"the jobs a dispatch of `{chosen}` selects")
            job = jobs[chosen]
            mapped = RUNNER_MAP.match(str(job["runs-on"]))
            if mapped is None:
                raise AssertionError(f"`{chosen}` resolves no runner: {job['runs-on']}")
            equal(
                json.loads(mapped["map"])[platform],
                runners[platform],
                describing=f"where `{chosen}` runs for `{platform}`",
            )
            equal(
                job["steps"],
                sources[chosen][1]["steps"],
                describing=f"what `{chosen}` runs, against {sources[chosen][0]}",
            )


def test_a_trigger_that_fires_on_a_change_is_refused(tree: Callable[[], Tree]) -> None:
    """A copy of the gate that also fires on a pull request is a second gate."""
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


def test_a_platform_input_offering_a_platform_the_list_does_not_name_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An option no supported platform backs runs on a runner the list never declared."""
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
    """An option naming a job that runs once per change, or no job at all, selects nothing."""
    broken = tree()
    broken.edit(DISPATCH, "          - gate\n", "          - gate\n          - llmlint\n")

    refused_naming(platform_dispatch(broken.repo), "`job` input offers `llmlint`")


def test_a_job_input_missing_a_matrixed_job_of_a_source_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Every platform-matrixed job of the sources is one a dispatch can name."""
    broken = tree()
    broken.edit(DISPATCH, "          - integration\n", "")

    refused_naming(platform_dispatch(broken.repo), "`job` input does not offer `integration`")


def test_an_input_that_is_not_required_is_refused(tree: Callable[[], Tree]) -> None:
    """A dispatch naming no platform would run on a default nobody chose."""
    broken = tree()
    broken.edit(
        DISPATCH,
        "      platform:\n        description: The platform to run it on, as `AGENTS.md`'s "
        "supported-platform list names it.\n        required: true\n",
        "      platform:\n        description: The platform to run it on, as `AGENTS.md`'s "
        "supported-platform list names it.\n        required: false\n",
    )

    refused_naming(platform_dispatch(broken.repo), "`platform` input is not required")


def test_a_job_selected_by_nothing_is_refused(tree: Callable[[], Tree]) -> None:
    """A job with no selection runs on every dispatch, whatever job was named."""
    broken = tree()
    broken.edit(DISPATCH, "    if: inputs.job == 'gate'\n", "")

    findings = platform_dispatch(broken.repo)

    refused_naming(findings, "job `gate`", "is not selected by `inputs.job == '<job>'` alone")
    refused_naming(findings, "declares no job selected by `inputs.job == 'gate'`")


def test_a_job_selected_by_more_than_the_job_input_is_refused(tree: Callable[[], Tree]) -> None:
    """A condition reading anything else could run the job for a dispatch that named another."""
    broken = tree()
    broken.edit(
        DISPATCH,
        "    if: inputs.job == 'gate'\n",
        "    if: inputs.job == 'gate' || inputs.platform == 'macos-aarch64'\n",
    )

    refused_naming(platform_dispatch(broken.repo), "job `gate`", "is not selected by")


def test_two_jobs_selected_by_one_option_are_refused(tree: Callable[[], Tree]) -> None:
    """A dispatch naming that option would run both."""
    broken = tree()
    broken.edit(DISPATCH, "    if: inputs.job == 'integration'\n", "    if: inputs.job == 'gate'\n")

    findings = platform_dispatch(broken.repo)

    refused_naming(findings, "job `integration`", "already selects job `gate`")
    refused_naming(findings, "declares no job selected by `inputs.job == 'integration'`")


def test_a_runner_the_list_does_not_declare_is_refused(tree: Callable[[], Tree]) -> None:
    """The platform named runs on the runner the list declares for it and no other."""
    broken = tree()
    broken.edit(DISPATCH, '"macos-aarch64":"macos-15"', '"macos-aarch64":"macos-14"')

    refused_naming(
        platform_dispatch(broken.repo), "maps `macos-aarch64` to `macos-14`", "`macos-15`"
    )


def test_a_runner_map_missing_a_platform_is_refused(tree: Callable[[], Tree]) -> None:
    """A platform the map omits is one a dispatch naming it runs nowhere."""
    broken = tree()
    broken.edit(DISPATCH, ',"macos-aarch64":"macos-15"', "")

    refused_naming(platform_dispatch(broken.repo), "maps no runner for `macos-aarch64`")


def test_a_runner_resolved_some_other_way_is_refused(tree: Callable[[], Tree]) -> None:
    """A runner written as a literal runs every platform on one machine."""
    broken = tree()
    broken.edit(
        DISPATCH,
        f"    runs-on: &runner ${{{{ fromJSON('{RUNNERS}')[inputs.platform] }}}}\n",
        "    runs-on: &runner ubuntu-24.04\n",
    )

    refused_naming(
        platform_dispatch(broken.repo), "job `gate`", "does not resolve `inputs.platform`"
    )


def test_a_copy_whose_steps_drifted_from_its_source_is_refused(tree: Callable[[], Tree]) -> None:
    """A copy runs what its source runs, or a dispatch proves something the source never did."""
    broken = tree()
    text = broken.read(DISPATCH)
    at = text.index("      - run: just check\n")
    broken.write(
        DISPATCH,
        f"{text[:at]}      - run: just test\n{text[at + len('      - run: just check\n') :]}",
    )

    refused_naming(
        platform_dispatch(broken.repo), "job `gate`", "runs steps other than ci.yml's job `gate`"
    )


def test_a_source_job_that_changed_is_a_copy_that_drifted(tree: Callable[[], Tree]) -> None:
    """The other direction: a source gaining a step leaves the copy behind."""
    broken = tree()
    broken.edit(
        CI, "      - run: just check\n", "      - run: just check\n      - run: just build\n"
    )

    refused_naming(platform_dispatch(broken.repo), "job `gate`", "runs steps other than")


def test_a_copy_carrying_a_matrix_of_its_own_is_refused(tree: Callable[[], Tree]) -> None:
    """A matrix on a copy is every cell again, which is what the dispatch exists to avoid."""
    broken = tree()
    broken.edit(
        DISPATCH,
        "    if: inputs.job == 'gate'\n",
        "    if: inputs.job == 'gate'\n    strategy:\n      matrix:\n        platform:\n"
        "          - id: linux-x86_64\n            runner: ubuntu-24.04\n",
    )

    refused_naming(platform_dispatch(broken.repo), "job `gate`", "carries `strategy`")


def test_a_declaration_that_is_absent_is_refused(tree: Callable[[], Tree]) -> None:
    """A tree declaring no dispatch workflow has nothing to hold one to."""
    broken = tree()
    text = broken.read(POLICY)
    start, end = text.index("[dispatch]\n"), text.index("[agent]\n")
    broken.write(POLICY, text[:start] + text[end:])

    refused(platform_dispatch(broken.repo), "declares no `[dispatch]` section")
    truth(dispatch_workflow(broken.repo) is None, describing="the workflow to pass over")


def test_a_declaration_naming_no_committed_workflow_is_refused(tree: Callable[[], Tree]) -> None:
    """The workflow the declaration names is one the tree commits."""
    broken = tree()
    broken.remove(DISPATCH)

    refused(platform_dispatch(broken.repo), "which is not there")


def test_a_source_that_is_not_there_is_refused(tree: Callable[[], Tree]) -> None:
    """A source workflow the tree does not commit is one nothing can be a copy of."""
    broken = tree()
    broken.edit(
        POLICY, 'sources = ["ci.yml", "install-path.yml"]', 'sources = ["ci.yml", "gone.yml"]'
    )

    refused_naming(platform_dispatch(broken.repo), "gone.yml", "commits no such workflow")


def test_the_step_classifying_checks_pass_the_copies_over(committed: Repo) -> None:
    """Read as jobs of their own, the copies would be a second gate with no matrix.

    So the checks that classify a job by its steps — the matrix rule, the gate
    and install-job rules, the status contexts, the unmatrixed-job record and
    the integration tier — read every workflow but this one, and the committed
    tree passes all of them with the copies in place.
    """
    truth(dispatch_workflow(committed) is not None, describing="the workflow to pass over")
    accepted(platforms(committed), describing="the matrix rule")
    accepted(continuous_integration(committed), describing="the gate rule")
    accepted(unmatrixed_jobs(committed), describing="the unmatrixed-job record")
    accepted(integration_tier(committed), describing="the integration tier")
    equal(
        [
            context.file
            for context in status_contexts(committed)
            if context.file == DISPATCH.rpartition("/")[2]
        ],
        [],
        describing="the status contexts read off the dispatch workflow",
    )


def test_without_the_pass_over_the_copies_would_be_refused(tree: Callable[[], Tree]) -> None:
    """The pass-over is load-bearing: name the dispatch workflow elsewhere and the copies show.

    Pointing the declaration at another file makes the real dispatch workflow
    an ordinary one to every other check, and its copy of the gate is then a
    gate job with no matrix.
    """
    broken = tree()
    broken.edit(POLICY, 'workflow = "platform-dispatch.yml"', 'workflow = "no-such-dispatch.yml"')

    refused_naming(
        platforms(broken.repo), "platform-dispatch.yml", "job `gate`", "declares no platform matrix"
    )


def test_a_declaration_missing_a_key_is_refused(tree: Callable[[], Tree]) -> None:
    """A declaration naming no sources, or no input, says what it is missing."""
    broken = tree()
    broken.edit(POLICY, 'sources = ["ci.yml", "install-path.yml"]\n', "")

    refused_naming(platform_dispatch(broken.repo), "`[dispatch]` declares no sources")


def test_a_declaration_naming_the_workflow_as_its_own_source_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow's jobs cannot be copies of themselves."""
    broken = tree()
    broken.edit(
        POLICY,
        'sources = ["ci.yml", "install-path.yml"]',
        'sources = ["ci.yml", "install-path.yml", "platform-dispatch.yml"]',
    )

    refused_naming(platform_dispatch(broken.repo), "as its own source")


def test_a_tree_with_no_platform_list_is_refused(tree: Callable[[], Tree]) -> None:
    """With no list there is nothing to hold the platform input to."""
    broken = tree()
    broken.edit(
        "AGENTS.md", "[//]: # (BEGIN supported-platforms)", "[//]: # (BEGIN platforms-once)"
    )

    refused(platform_dispatch(broken.repo), "supported-platforms")


def test_an_input_that_is_not_a_choice_is_refused(tree: Callable[[], Tree]) -> None:
    """A free-text input can name a job or a platform nothing here declares."""
    broken = tree()
    broken.edit(
        DISPATCH,
        "        required: true\n        type: choice\n        options:\n          - gate\n",
        "        required: true\n        type: string\n",
    )

    refused_naming(platform_dispatch(broken.repo), "no `job` input of type `choice`")


def test_a_runner_map_that_is_not_json_is_refused(tree: Callable[[], Tree]) -> None:
    """A map nothing can read maps nothing."""
    broken = tree()
    broken.edit(DISPATCH, f"fromJSON('{RUNNERS}')", "fromJSON('not json')")

    refused_naming(platform_dispatch(broken.repo), "job `gate`", "not JSON")


def test_a_runner_map_that_is_not_a_map_is_refused(tree: Callable[[], Tree]) -> None:
    """A list of runners says nothing about which platform runs where."""
    broken = tree()
    broken.edit(DISPATCH, f"fromJSON('{RUNNERS}')", "fromJSON('[\"ubuntu-24.04\"]')")

    refused_naming(platform_dispatch(broken.repo), "job `gate`", "something other than a map")
