"""Fixtures that hand a journey a real copy of the committed tree.

The hooks and fixtures are `journey_session`'s, which the tier's other project,
`repo-e2e-independent`, takes too; this file makes them this project's.
"""

from __future__ import annotations

from journey_session import (
    gate_copy as gate_copy,
)
from journey_session import (
    host_service_manager as host_service_manager,
)
from journey_session import (
    no_ambient_gate_base as no_ambient_gate_base,
)
from journey_session import (
    no_ambient_host_independence as no_ambient_host_independence,
)
from journey_session import (
    worker_count,
)
from journey_session import (
    pytest_terminal_summary as pytest_terminal_summary,
)
from journey_session import (
    shared_venv as shared_venv,
)

pytest_xdist_auto_num_workers = worker_count("repo-e2e")
