"""Assembling a wheel, including one carrying a program built for a platform.

A wheel is a zip with a manifest, and this writes one rather than driving a
build backend. Two reasons, and both are about the artifact rather than about
convenience: the command-line distribution's wheel has to carry a **platform
tag stating the C library its program was built against**, which no pure-Python
backend produces; and the same code writing both distributions means the
client's wheel and the program's wheel cannot come to be assembled two
different ways.
"""

from __future__ import annotations

import base64
import csv
import hashlib
import io
import zipfile
from dataclasses import dataclass, field
from pathlib import Path

#: The wheel format this writes, as `WHEEL` states it.
WHEEL_VERSION = "1.0"

#: The tag a wheel carrying no compiled code takes.
PURE_TAG = "py3-none-any"

#: The interpreter and ABI a wheel of this repository takes. The program a
#: platform wheel carries is not a Python extension: it runs under no
#: interpreter, so the wheel is tagged for none in particular.
INTERPRETER = "py3-none"


@dataclass(frozen=True, slots=True)
class Distribution:
    """What one wheel says about itself."""

    #: The distribution name, as the registry serves it.
    name: str
    #: The version, which release automation derives from the commit subjects.
    version: str
    summary: str
    #: The Python versions it declares support for.
    requires_python: str
    #: The license it is published under.
    license: str
    #: Where its sources are.
    homepage: str

    @property
    def normalized(self) -> str:
        """The name as a wheel's own file names spell it."""
        return self.name.replace("-", "_").replace(".", "_")

    @property
    def dist_info(self) -> str:
        """The metadata directory inside the wheel."""
        return f"{self.normalized}-{self.version}.dist-info"

    @property
    def data(self) -> str:
        """The directory an installer copies into the environment."""
        return f"{self.normalized}-{self.version}.data"

    def metadata(self) -> str:
        """The `METADATA` a registry and an installed package are read from."""
        return "\n".join(
            [
                "Metadata-Version: 2.1",
                f"Name: {self.name}",
                f"Version: {self.version}",
                f"Summary: {self.summary}",
                f"Home-page: {self.homepage}",
                f"License: {self.license}",
                f"Requires-Python: {self.requires_python}",
                "",
                self.summary,
                "",
            ]
        )


@dataclass(slots=True)
class Wheel:
    """One wheel being assembled."""

    distribution: Distribution
    #: The compatibility tag it carries, `py3-none-any` for a pure one.
    tag: str
    #: What goes in it, by the path it takes inside the wheel.
    contents: dict[str, bytes] = field(default_factory=dict)
    #: Which of those an installer marks executable, as scripts.
    scripts: set[str] = field(default_factory=set)

    @property
    def file_name(self) -> str:
        """What the wheel is called, which is how an installer reads its tag."""
        return f"{self.distribution.normalized}-{self.distribution.version}-{self.tag}.whl"

    def add(self, inside: str, content: bytes) -> None:
        """Put one file in the wheel."""
        self.contents[inside] = content

    def add_tree(self, root: Path, under: str) -> None:
        """Put a whole directory of Python sources in the wheel."""
        for path in sorted(root.rglob("*")):
            if not path.is_file() or "__pycache__" in path.parts:
                continue
            self.add(f"{under}/{path.relative_to(root).as_posix()}", path.read_bytes())

    def add_script(self, name: str, program: Path) -> None:
        """Put a program in the wheel, for an installer to put on the path."""
        inside = f"{self.distribution.data}/scripts/{name}"
        self.add(inside, program.read_bytes())
        self.scripts.add(inside)

    def write(self, into: Path) -> Path:
        """Write the wheel, and answer where it was written."""
        into.mkdir(parents=True, exist_ok=True)
        target = into / self.file_name
        entries = dict(self.contents)
        entries[f"{self.distribution.dist_info}/METADATA"] = self.distribution.metadata().encode()
        entries[f"{self.distribution.dist_info}/WHEEL"] = "\n".join(
            [
                f"Wheel-Version: {WHEEL_VERSION}",
                "Generator: printobserver-release-artifacts",
                f"Root-Is-Purelib: {'true' if self.tag == PURE_TAG else 'false'}",
                f"Tag: {self.tag}",
                "",
            ]
        ).encode()

        record = io.StringIO()
        writer = csv.writer(record, lineterminator="\n")
        with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
            for inside, content in sorted(entries.items()):
                info = zipfile.ZipInfo(inside, date_time=(1980, 1, 1, 0, 0, 0))
                # Executable where it is a script an installer puts on the
                # path, and read-only otherwise. A wheel whose program is not
                # executable installs a file nobody can run.
                info.external_attr = (0o755 if inside in self.scripts else 0o644) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, content)
                writer.writerow([inside, _digest(content), len(content)])
            writer.writerow([f"{self.distribution.dist_info}/RECORD", "", ""])
            archive.writestr(
                zipfile.ZipInfo(
                    f"{self.distribution.dist_info}/RECORD", date_time=(1980, 1, 1, 0, 0, 0)
                ),
                record.getvalue(),
            )
        return target


def _digest(content: bytes) -> str:
    """One file's digest, as a wheel's `RECORD` states it."""
    raw = hashlib.sha256(content).digest()
    return "sha256=" + base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii")
