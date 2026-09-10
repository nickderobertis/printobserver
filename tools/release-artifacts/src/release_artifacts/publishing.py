"""Putting each built artifact where its own consumers install it from.

Every publish here authenticates with an **API token carried in a repository
secret**. Not a keyless trusted publisher: this repository's own manifest,
`gh-secrets.json`, is the authoritative list of what it holds, and a publish
that authenticated by something outside it would be a credential nobody
declared. The names below are the environment those tokens arrive in, and
`just check-repo` refuses a workflow that names a secret that manifest does not
declare.
"""

from __future__ import annotations

from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts.build import ASSEMBLED_HERE, CHECKSUMS
from release_artifacts.targets import Target, declared

#: The environment each registry's own credential arrives in, which is also the
#: repository secret it is carried by. One place spells them; `gh-secrets.json`
#: is where a check confirms the repository holds them.
CREDENTIALS = {
    "pypi": "PYPI_TOKEN",
    "npm": "NPM_TOKEN",
    "release": "RELEASE_PLZ_TOKEN",
}

#: How long any one publish is given.
PUBLISH_TIMEOUT_SECONDS = 900


class PublishError(RuntimeError):
    """An artifact could not be published."""


def _credential(environment: dict[str, str], registry: str) -> str:
    """The token one registry is published under.

    Raises:
        PublishError: If the environment carries none, which is a publish that
            would otherwise fail after a merge rather than before one.
    """
    name = CREDENTIALS[registry]
    token = environment.get(name, "").strip()
    if not token:
        msg = (
            f"publishing to {registry} needs {name}, and the environment carries none. "
            f"It is a repository secret `gh-secrets.json` declares."
        )
        raise PublishError(msg)
    return token


def _ran(argv: list[str], *, cwd: Path, env: dict[str, str], describing: str) -> str:
    """Run one publish, or stop saying what it said.

    Raises:
        PublishError: If it failed.
    """
    result = run(argv, cwd=cwd, env=env, timeout=PUBLISH_TIMEOUT_SECONDS)
    if result.returncode != 0:
        msg = f"{describing} failed ({result.returncode}):\n{result.stdout}{result.stderr}"
        raise PublishError(msg)
    return result.stdout + result.stderr


def _built(dist: Path, suffixes: tuple[str, ...]) -> list[Path]:
    """Every built artifact of a kind, in a stable order."""
    return sorted(path for path in dist.iterdir() if path.name.endswith(suffixes))


def publish(repo: Repo, dist: Path, environment: dict[str, str]) -> list[str]:
    """Publish every artifact this tool assembles, and say what went where.

    Raises:
        PublishError: If a registry refused one, or a credential is absent.
    """
    said: list[str] = []
    registries = {
        target.registry
        for target in declared(repo.root)
        if target.built_by == ASSEMBLED_HERE and target.registry != "crate"
    }
    if "pypi" in registries:
        token = _credential(environment, "pypi")
        for wheel in _built(dist, (".whl",)):
            _ran(
                ["uv", "publish", "--token", token, str(wheel)],
                cwd=repo.root,
                env=environment,
                describing=f"publishing {wheel.name}",
            )
            said.append(f"pypi\t{wheel.name}")
    if "npm" in registries:
        token = _credential(environment, "npm")
        npmrc = dist / ".npmrc"
        npmrc.write_text(f"//registry.npmjs.org/:_authToken={token}\n", encoding="utf-8")
        for tarball in _built(dist, (".tgz",)):
            _ran(
                [
                    "npm",
                    "publish",
                    "--access",
                    "public",
                    f"--userconfig={npmrc}",
                    str(tarball),
                ],
                cwd=repo.root,
                env=environment,
                describing=f"publishing {tarball.name}",
            )
            said.append(f"npm\t{tarball.name}")
        npmrc.unlink()
    if "release" in registries:
        said.extend(_publish_release(repo, dist, environment))
    return said


def _publish_release(repo: Repo, dist: Path, environment: dict[str, str]) -> list[str]:
    """Put the per-platform artifacts the install script downloads on the release."""
    token = _credential(environment, "release")
    version = _version(repo)
    assets = [path for path in _built(dist, (".tar.gz",)) if path.name.startswith("printobserver-")]
    digests = dist / CHECKSUMS
    if digests.is_file():
        assets.append(digests)
    _ran(
        ["gh", "release", "upload", f"v{version}", *[str(path) for path in assets], "--clobber"],
        cwd=repo.root,
        env={**environment, "GH_TOKEN": token},
        describing=f"uploading the release artifacts of v{version}",
    )
    return [f"release\t{path.name}" for path in assets]


def _version(repo: Repo) -> str:
    """The version release automation wrote into the workspace."""
    from release_artifacts.targets import workspace

    return workspace(repo.root)["version"]


def registries_of(repo: Repo) -> list[Target]:
    """Every target this tool publishes, in the order the declaration names them."""
    return [target for target in declared(repo.root) if target.built_by == ASSEMBLED_HERE]
