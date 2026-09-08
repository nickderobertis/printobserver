"""The Python client package is importable from a bootstrapped environment.

There is no client code yet, so the one journey this tier can drive is the one
a consumer takes first: import the package the distribution will ship.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.
import printobserver_sdk
from repo_checks.expect import equal


def test_the_package_imports_and_exports_nothing_yet() -> None:
    """A fresh environment can import the package the `sdks` node will fill in."""
    equal(printobserver_sdk.__all__, [])
