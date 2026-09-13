"""The crate set, the module comments, and the edge table the dependency rule is."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import tomllib
from collections.abc import Callable

from repo_checks.checks_repo import workspace
from repo_checks.expect import accepted, contains, equal, refused
from repo_checks.model import Repo
from treecopy import Tree

# The crates this repository is required to hold, named here rather than read
# from the tree, so that a tree that lost one is refused by the list rather than
# by agreeing with itself.
REQUIRED_CRATES = (
    "printobserver-types",
    "printobserver-printer-api",
    "printobserver-vision-api",
    "printobserver-supervisor-api",
    "printobserver-octoprint",
    "printobserver-obico",
    "printobserver-oneharness",
    "printobserver-store-sqlite",
    "printobserver-core",
    "printobserver-server",
    "printobserver-sdk",
    "printobserver",
)

DEPENDENCY = '{name} = {{ path = "../{name}" }}\n'
DEPENDENCIES = "[dependencies]\n"
POLICY = "repo-policy.toml"

# The four adapters and the store: the crates the composition root alone may
# name, which the edge table says by admitting them in one row.
IMPLEMENTATIONS = (
    "printobserver-octoprint",
    "printobserver-obico",
    "printobserver-oneharness",
    "printobserver-store-sqlite",
)


def with_only_dependencies(manifest: str, *names: str) -> str:
    """`manifest` declaring exactly the path dependencies named, and no other.

    The edge under test is written by replacing the crate's dependency table
    rather than by appending a second one, so a fixture says the same thing
    whatever the crate it is built from already declares. Appending grew a
    manifest with two `[dependencies]` headers, or two entries for one crate,
    the day core gained a dependency of its own — and a fixture that no longer
    parses proves nothing about the dependency rule.
    """
    entries = "".join(DEPENDENCY.format(name=name) for name in names)
    lines = manifest.splitlines(keepends=True)
    if DEPENDENCIES not in lines:
        return manifest + "\n" + DEPENDENCIES + entries
    start = lines.index(DEPENDENCIES) + 1
    end = next(
        (index for index in range(start, len(lines)) if lines[index].startswith("[")),
        len(lines),
    )
    return "".join(lines[:start]) + entries + "".join(lines[end:])


def test_the_workspace_holds_exactly_the_required_crates(committed: Repo) -> None:
    """Every crate the design names, and no crate it does not."""
    equal(sorted(committed.crate_names), sorted(REQUIRED_CRATES))


def test_the_committed_workspace_is_accepted(committed: Repo) -> None:
    """The workspace this repository ships satisfies its own dependency rule."""
    accepted(workspace(committed))


def test_every_crate_states_what_it_owns_and_what_it_may_depend_on(
    committed: Repo,
) -> None:
    """The layering is readable at the top of every crate, not only in a check."""
    for directory in committed.crate_dirs:
        source = directory / "src" / "lib.rs"
        if not source.is_file():
            source = directory / "src" / "main.rs"
        comment = source.read_text(encoding="utf-8")
        contains(comment, "Owns:")
        contains(comment, "May depend on:")


def test_a_missing_crate_is_refused(tree: Callable[[], Tree]) -> None:
    """A workspace that lost a crate the design names."""
    broken = tree()
    broken.remove("crates/printobserver-obico")

    findings = workspace(broken.repo)

    refused(findings, "printobserver-obico")


def test_an_undeclared_crate_is_refused(tree: Callable[[], Tree]) -> None:
    """A crate nobody declared is a layer nobody agreed to."""
    broken = tree()
    broken.write(
        "crates/printobserver-surprise/Cargo.toml",
        '[package]\nname = "printobserver-surprise"\nversion.workspace = true\n',
    )
    broken.write("crates/printobserver-surprise/src/lib.rs", "//! Owns: nothing.\n")

    findings = workspace(broken.repo)

    refused(findings, "printobserver-surprise")


def test_a_crate_whose_comment_omits_what_it_owns_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A module comment that does not say what the crate owns."""
    broken = tree()
    broken.write("crates/printobserver-core/src/lib.rs", "//! May depend on: nothing.\n")

    findings = workspace(broken.repo)

    refused(findings, "does not say what it owns")


def test_a_crate_whose_comment_omits_its_dependencies_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A module comment that does not say what the crate may depend on."""
    broken = tree()
    broken.write("crates/printobserver-core/src/lib.rs", "//! Owns: the supervision logic.\n")

    findings = workspace(broken.repo)

    refused(findings, "does not say what it may depend on")


def test_core_depending_on_an_implementation_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The edge the whole design exists to forbid, refused by the table's row for core."""
    broken = tree()
    broken.write(
        "crates/printobserver-core/Cargo.toml",
        with_only_dependencies(
            broken.read("crates/printobserver-core/Cargo.toml"), "printobserver-octoprint"
        ),
    )

    findings = workspace(broken.repo)

    refused(findings, "printobserver-core` depends on `printobserver-octoprint")
    refused(findings, "which the dependency table does not admit")


def test_an_edge_the_table_does_not_admit_is_refused_in_any_dependency_table(
    tree: Callable[[], Tree],
) -> None:
    """A test-only edge is an edge: the rule reads every dependency table.

    Driven over a port naming an adapter under `dev-dependencies`, because that
    is the table a reader forgets, and a port that could reach an
    implementation through its tests would stop being a port.
    """
    broken = tree()
    manifest = broken.read("crates/printobserver-vision-api/Cargo.toml")
    broken.write(
        "crates/printobserver-vision-api/Cargo.toml",
        manifest.replace(
            "[dev-dependencies]\n",
            "[dev-dependencies]\n" + DEPENDENCY.format(name="printobserver-obico"),
        ),
    )
    contains(broken.read("crates/printobserver-vision-api/Cargo.toml"), "printobserver-obico")

    findings = workspace(broken.repo)

    refused(findings, "`printobserver-vision-api` depends on `printobserver-obico`")


def test_an_implementation_depending_on_another_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An edge between two adapters is a hidden coupling between two vendors."""
    broken = tree()
    broken.write(
        "crates/printobserver-obico/Cargo.toml",
        with_only_dependencies(
            broken.read("crates/printobserver-obico/Cargo.toml"), "printobserver-octoprint"
        ),
    )

    findings = workspace(broken.repo)

    refused(findings, "`printobserver-obico` depends on `printobserver-octoprint`")


def test_a_client_depending_on_an_implementation_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The generated client's row admits nothing, so it may name nothing."""
    broken = tree()
    broken.write(
        "crates/printobserver-sdk/Cargo.toml",
        with_only_dependencies(
            broken.read("crates/printobserver-sdk/Cargo.toml"), "printobserver-octoprint"
        ),
    )

    findings = workspace(broken.repo)

    refused(findings, "`printobserver-sdk` depends on `printobserver-octoprint`")
    refused(findings, "names no crate at all")


def test_a_row_naming_a_crate_the_workspace_lacks_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A row for a crate nobody has is a rule about nothing."""
    broken = tree()
    broken.write(
        POLICY,
        broken.read(POLICY).replace(
            '"printobserver-sdk" = []\n',
            '"printobserver-sdk" = []\n"printobserver-store-api" = ["printobserver-types"]\n',
        ),
    )

    findings = workspace(broken.repo)

    refused(findings, "declares crate `printobserver-store-api`, which the workspace does not hold")


def test_a_row_admitting_a_crate_the_workspace_lacks_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An admitted edge to a crate nobody has is an edge nothing can check."""
    broken = tree()
    broken.write(
        POLICY,
        broken.read(POLICY).replace(
            '"printobserver-sdk" = []\n',
            '"printobserver-sdk" = ["printobserver-store-api"]\n',
        ),
    )

    findings = workspace(broken.repo)

    refused(
        findings,
        "row for `printobserver-sdk` admits `printobserver-store-api`, which the workspace "
        "does not hold",
    )


def test_a_workspace_crate_the_table_omits_is_refused(tree: Callable[[], Tree]) -> None:
    """A crate with no row has no rule, and a rule with a gap is not the rule."""
    broken = tree()
    broken.write(POLICY, broken.read(POLICY).replace('"printobserver-sdk" = []\n', ""))

    findings = workspace(broken.repo)

    refused(findings, "holds crate `printobserver-sdk`, which `repo-policy.toml` does not declare")


def test_a_policy_with_no_table_is_refused(tree: Callable[[], Tree]) -> None:
    """A rule stated nowhere refuses nothing, so its absence is the finding."""
    broken = tree()
    broken.write(
        POLICY, broken.read(POLICY).replace("[crates.may_depend_on]", "[crates.was_a_table]")
    )

    findings = workspace(broken.repo)

    refused(findings, "declares no `crates.may_depend_on` table")


def test_the_table_equals_the_graph_the_design_states(committed: Repo) -> None:
    """The four properties the graph is drawn for, read off the committed rows.

    The type crate depends on nothing; each port on the type crate alone; the
    supervision domain on the type crate and the three ports and on no adapter
    or store; and every adapter and the store are admitted by the server's row
    and by no other. The command reaches them through the server.
    """
    table = committed.policy["crates"]["may_depend_on"]
    equal(table["printobserver-types"], [])
    for port in (
        "printobserver-printer-api",
        "printobserver-vision-api",
        "printobserver-supervisor-api",
    ):
        equal(table[port], ["printobserver-types"])
    equal(
        sorted(table["printobserver-core"]),
        sorted(
            [
                "printobserver-types",
                "printobserver-printer-api",
                "printobserver-vision-api",
                "printobserver-supervisor-api",
            ]
        ),
    )
    for implementation in IMPLEMENTATIONS:
        admitting = sorted(name for name, admitted in table.items() if implementation in admitted)
        equal(admitting, ["printobserver-server"], describing=implementation)


def test_the_server_is_the_only_crate_that_names_an_implementation(
    committed: Repo,
) -> None:
    """The one crate the table lets name an implementation is the one that does.

    `printobserver` reaches the implementations through `printobserver-server`
    rather than naming them itself, so the tree this repository ships has
    exactly one crate that names one — asserted over the manifests here, where
    a reader can see which crate that is, beside the table that admits it.
    """
    implementations = set(IMPLEMENTATIONS)
    naming = sorted(
        name
        for name in committed.crate_names
        if implementations
        & set(
            tomllib.loads(
                (committed.path(f"crates/{name}") / "Cargo.toml").read_text(encoding="utf-8")
            )
            .get("dependencies", {})
            .keys()
        )
    )

    equal(naming, ["printobserver-server"])


# What the command-line program must depend on, so a fixture that is about one
# forbidden edge carries everything else the rule requires and fails on that
# edge alone.
CLI = "crates/printobserver/Cargo.toml"
CLI_REQUIRES = ("printobserver-server", "printobserver-sdk")


def test_the_command_line_program_names_the_server_and_the_client(
    committed: Repo,
) -> None:
    """The tree this repository ships has the two edges the rule requires."""
    manifest = tomllib.loads(committed.path(CLI).read_text(encoding="utf-8"))
    for required in CLI_REQUIRES:
        contains(str(sorted(manifest["dependencies"])), required)
    accepted(workspace(committed))


def test_the_command_line_program_naming_a_vendor_adapter_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The edge that would let a client reach a printer without the supervisor.

    Driven over each forbidden adapter in turn rather than one of them: the
    command-line program is a composition root, so the rule that stops every
    other crate naming an implementation permits it one — and what makes these
    two different is that it is a *client* of a running server for everything
    but starting one.
    """
    for edge in ("printobserver-octoprint", "printobserver-obico"):
        broken = tree()
        broken.write(
            CLI,
            with_only_dependencies(broken.read(CLI), *CLI_REQUIRES, edge),
        )

        findings = workspace(broken.repo)

        refused(findings, f"`printobserver` depends on `{edge}`")


def test_the_command_line_program_missing_the_client_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A command line that is not built on the client of the surface it calls."""
    broken = tree()
    broken.write(
        CLI,
        with_only_dependencies(broken.read(CLI), "printobserver-server"),
    )

    findings = workspace(broken.repo)

    refused(findings, "printobserver-sdk")


def test_core_may_depend_on_a_port(tree: Callable[[], Tree]) -> None:
    """The rule forbids the edges it names and permits the ones it allows."""
    allowed = tree()
    allowed.write(
        "crates/printobserver-core/Cargo.toml",
        with_only_dependencies(
            allowed.read("crates/printobserver-core/Cargo.toml"), "printobserver-printer-api"
        ),
    )

    accepted(workspace(allowed.repo))
