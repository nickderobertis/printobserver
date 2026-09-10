"""Every artifact this repository publishes, built from the committed tree.

Six of them: the three clients a dependent takes as a dependency, and the three
alternative routes an end user gets the `printobserver` program by. They are
built here rather than by six build backends, for one reason that reaches all
six — **no manifest in this tree carries a version**. Release automation owns
the number, writes it into the workspace, and every artifact takes it from
there, so there is nothing for a person to keep in step and nothing to fall out
of step.

`release-targets.toml` is what says which artifacts there are. Nothing here
restates a name, a registry or a route.
"""

from __future__ import annotations

from release_artifacts.build import BuildError, Built, build, build_all, staged_release
from release_artifacts.targets import Target, TargetError, declared

__all__ = [
    "BuildError",
    "Built",
    "Target",
    "TargetError",
    "build",
    "build_all",
    "declared",
    "staged_release",
]
