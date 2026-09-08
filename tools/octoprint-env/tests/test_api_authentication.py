"""The instance is left authenticated, and the key it provisioned is readable.

Disabling authentication is what this journey exists to refuse: the key is what
the printer plugin authenticates with, so a fixture whose API call succeeded
without one would prove a configuration nobody runs.
"""

from __future__ import annotations

from collections.abc import Callable

from environment import answer, api, key_from, script
from repo_checks.expect import contains, equal, passing, truth

# An authenticated endpoint: OctoPrint answers it to a caller carrying a key
# with the permission, and refuses it to a caller carrying none.
AUTHENTICATED = "/api/settings"
REFUSED = frozenset({401, 403})


def test_the_provisioned_key_is_accepted_and_the_same_call_without_it_is_refused(
    state_dir: Callable[[str], str],
) -> None:
    """The key is read from the path the script named on its own output."""
    result = script("up", "--state-dir", state_dir("authenticated"), "--no-print")

    passing(result, describing="starting an instance to authenticate against")
    started = answer(result)
    url = str(started["url"])
    named = str(started["api_key_file"])
    contains(result.stdout, named, describing="the script's own output, which names the key file")

    key = key_from(named)

    truth(key, describing=f"a key readable at {named}")
    carried, body = api(url, AUTHENTICATED, key)
    equal(carried, 200, describing=f"GET {AUTHENTICATED} carrying the provisioned key")
    truth(isinstance(body, dict), describing="a decoded settings answer")

    refused, _ = api(url, AUTHENTICATED, None)

    contains(REFUSED, refused, describing=f"the status of GET {AUTHENTICATED} carrying no key")
