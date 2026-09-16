"""No copy of a platform fact is maintained independently of the one that declares it.

Three places state a platform fact again in a language that cannot import the
declaration it comes from: the JavaScript launcher's map, the install script's
own `uname` arms, and the toolchain file's list of Rust targets. Two of the
three were reconciled by nothing at all before this check, and what a user meets
when one falls behind is a global install that resolves to no program.

Each test below makes exactly one of the three disagree in a real copy of the
tree and reads what the committed check says about it.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_platforms import platform_facts
from repo_checks.expect import accepted, refused, refused_naming
from repo_checks.model import Repo
from treecopy import Tree

LAUNCHER = "npm/printobserver-cli/bin/printobserver.mjs"
SCRIPT = "scripts/install.sh"
TOOLCHAIN = "rust-toolchain.toml"

ARM64_ENTRY = '  "linux-arm64": "@printobserver/cli-linux-arm64",\n'
AARCH64_ARM = '    Linux/aarch64 | Linux/arm64) platform="linux-aarch64" ;;\n'

#: A second install script, as a fixture adds one: enough of a script for its
#: own arms to be read, fetched by a second command of route 3.
SECOND_SCRIPT = """#!/bin/sh
set -eu
case "$(uname -s)/$(uname -m)" in
    Linux/x86_64) platform="linux-x86_64" ;;
esac
echo "$platform" >&2
"""

SECOND_URL = (
    "https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-second.sh"
)
FETCH_SECOND = f"\n```console\ncurl -fsSL {SECOND_URL} | sh\n```\n"


def test_the_committed_tree_keeps_every_copy_in_step(committed: Repo) -> None:
    """The launcher, the install script and the toolchain all agree with the list."""
    accepted(platform_facts(committed))


def test_a_launcher_naming_a_platform_the_install_path_does_not_target_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A package the launcher resolves and nothing publishes installs to nothing."""
    broken = tree()
    broken.edit(
        LAUNCHER,
        ARM64_ENTRY,
        ARM64_ENTRY + '  "darwin-arm64": "@printobserver/cli-darwin-arm64",\n',
    )

    findings = platform_facts(broken.repo)

    refused_naming(findings, LAUNCHER, "resolves `darwin-arm64`")


def test_a_launcher_missing_a_platform_the_install_path_targets_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A platform the list reaches by route 2 and the launcher does not is an install that fails."""
    broken = tree()
    broken.edit(LAUNCHER, ARM64_ENTRY, "")

    findings = platform_facts(broken.repo)

    refused_naming(findings, LAUNCHER, "resolves no package for `linux-arm64`")


def test_a_launcher_resolving_another_package_name_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The package name is the descriptor's, and a second spelling of it is a second source."""
    broken = tree()
    broken.edit(LAUNCHER, ARM64_ENTRY, ARM64_ENTRY.replace("cli-linux-arm64", "cli-linux-aarch64"))

    findings = platform_facts(broken.repo)

    refused_naming(findings, "@printobserver/cli-linux-aarch64", "@printobserver/cli-linux-arm64")


def test_an_install_script_arm_naming_a_platform_the_list_does_not_carry_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A script that installs for a platform nothing publishes for downloads nothing."""
    broken = tree()
    broken.edit(SCRIPT, AARCH64_ARM, AARCH64_ARM.replace("linux-aarch64", "linux-riscv64"))

    findings = platform_facts(broken.repo)

    refused_naming(findings, SCRIPT, "linux-riscv64", "does not name")


def test_an_install_script_reaching_no_arm_for_a_targeted_platform_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A platform answered `install path: yes` and reached by no script cannot take route 3."""
    broken = tree()
    broken.edit(SCRIPT, AARCH64_ARM, "")

    findings = platform_facts(broken.repo)

    refused_naming(findings, "no install script", "`linux-aarch64`")


def test_two_install_scripts_reaching_one_platform_are_refused(
    tree: Callable[[], Tree],
) -> None:
    """Exactly one: two scripts installing one platform are two answers to one question."""
    broken = tree()
    broken.write("scripts/install-second.sh", SECOND_SCRIPT)
    broken.edit(
        "AGENTS.md",
        "\n### Then, check what you installed",
        FETCH_SECOND + "\n### Then, check what you installed",
    )

    findings = platform_facts(broken.repo)

    refused_naming(findings, "`linux-x86_64` is reached by 2", "exactly one must reach it")


def test_an_install_script_a_route_fetches_and_the_tree_does_not_carry_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A route fetching a script nobody committed is a route that downloads nothing."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "\n### Then, check what you installed",
        FETCH_SECOND + "\n### Then, check what you installed",
    )

    findings = platform_facts(broken.repo)

    refused(findings, "scripts/install-second.sh")


def test_a_toolchain_target_no_supported_platform_is_built_for_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The binding is one way, and this is the direction it binds."""
    broken = tree()
    broken.edit(
        TOOLCHAIN,
        'targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]',
        'targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", '
        '"x86_64-apple-darwin"]',
    )

    findings = platform_facts(broken.repo)

    refused_naming(findings, TOOLCHAIN, "x86_64-apple-darwin")


def test_a_toolchain_naming_fewer_targets_than_the_list_is_accepted(
    tree: Callable[[], Tree],
) -> None:
    """A build host installing one standard library is not a platform being dropped."""
    fewer = tree()
    fewer.edit(
        TOOLCHAIN,
        'targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]',
        'targets = ["x86_64-unknown-linux-gnu"]',
    )

    accepted(platform_facts(fewer.repo), describing="a toolchain installing one of the two")


def test_a_launcher_resolving_one_platform_twice_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The second entry replaces the first, and an install takes whichever came last."""
    broken = tree()
    broken.edit(
        LAUNCHER,
        ARM64_ENTRY,
        ARM64_ENTRY + '  "linux-arm64": "@printobserver/cli-linux-aarch64",\n',
    )

    findings = platform_facts(broken.repo)

    refused_naming(findings, LAUNCHER, "`linux-arm64` more than once")
