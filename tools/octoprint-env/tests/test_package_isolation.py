"""An edit to one environment's installed package reaches no other environment.

`uv` installs from a cache every environment on the host shares, by hard link
unless told otherwise. A hard-linked file edited in place — a diagnosis trying a
fix in an installed module — is the cached file, so the next environment
installed on that host receives the edit as though it were the published
package. This journey provisions two instances on one cache of their own and
edits the first's installed OctoPrint in between.
"""

from __future__ import annotations

import os
from collections.abc import Callable
from pathlib import Path

from environment import script
from repo_checks.expect import equal, passing

#: The installed module this journey edits: one the managed repair never touches.
EDITED = ("octoprint", "__init__.py")

#: What the edit appends, which no published release carries.
EDIT = b"\n# edited in place by a diagnosis\n"


def _installed(state: Path) -> Path:
    """The edited module's path inside an instance's environment."""
    found = [
        path
        for path in (state / "venv").glob("**/site-packages/" + "/".join(EDITED))
        if path.is_file()
    ]
    equal(len(found), 1, describing=f"installed {'/'.join(EDITED)} under {state}")
    return found[0]


def test_an_in_place_edit_to_one_environment_leaves_the_next_install_unchanged(
    state_dir: Callable[[str], str], tmp_path: Path
) -> None:  # llmlint: ignore[test_tiers_split_by_project_not_by_marker] See suppressions.toml.
    """A second clean environment on the same cache receives the published file."""
    # A cache of this journey's own: an edit reaching it would otherwise reach
    # every environment the host installs afterwards.
    env = {**os.environ, "UV_CACHE_DIR": str(tmp_path / "uv-cache")}
    first = Path(state_dir("first"))
    passing(script("install", "--state-dir", str(first), env=env), describing="the first install")
    module = _installed(first)
    published = module.read_bytes()
    with module.open("r+b") as edited:
        edited.seek(0, os.SEEK_END)
        edited.write(EDIT)

    second = Path(state_dir("second"))
    passing(script("install", "--state-dir", str(second), env=env), describing="the second install")
    equal(
        _installed(second).read_bytes(),
        published,
        describing="the second environment's module after the first's was edited in place",
    )
