"""What makes the all-operation walk's own equality worth having.

`test_live.py` asserts that what each method answered carries, field for field,
what the **real** supervisor sent. An assertion nothing can fail is not one, so
two clients that a walk over emitted values would never catch are driven through
that same comparison here and are asserted to be refused.

Both are about the one thing this system will not do: neither puts image bytes
in an answer legitimately — they put them where a client with a defect would, at
a field of a generated response type that declares nothing of the kind, which is
exactly the shape no probe guessing at encodings would find. One carries a
base64 of the image in an existing string field; the other carries the bytes
themselves, as a `bytes` value rather than as any rendering of them into text.

Each variant is the published client with one thing done to what it answers,
which is what a defect is. The comparison is the committed one — `live.matches`,
the function the generated walk calls — so what is proven is that walk's own
assertion rather than a copy of it.
"""

from __future__ import annotations

import base64
import copy
import json
from collections.abc import Iterator
from pathlib import Path
from typing import cast

import pytest
from live import Proxy, matches, same
from printobserver_sdk import Client
from printobserver_sdk.contract import ImageAnswer
from repo_checks.expect import equal, truth
from world import Standing, Supervisor


@pytest.fixture(scope="module")
def world(tmp_path_factory: pytest.TempPathFactory) -> Iterator[Supervisor]:
    """A real supervisor over the scripted `OctoPrint`, held up for this module."""
    with Standing(tmp_path_factory.mktemp("falsifying")) as supervisor:
        yield supervisor


def _bytes_of(answered: ImageAnswer) -> bytes:
    """The stored image's own bytes, opened at the path the supervisor answered."""
    path = answered.get("path")
    truth(isinstance(path, str), describing="the supervisor to answer a path on its host")
    return Path(str(path)).read_bytes()


def base64_of_the_image_into_a_string_field(answered: ImageAnswer) -> ImageAnswer:
    """A client that writes a base64 of the image into an existing string field.

    The field is the record's own digest, which is a string the contracts
    declare and a caller reads — so nothing about the answer's *shape* has
    changed, and only what it carries has.
    """
    variant = copy.deepcopy(answered)
    variant["record"]["sha256"] = base64.b64encode(_bytes_of(answered)).decode("ascii")
    return variant


def the_images_bytes_in_place_of_a_field(answered: ImageAnswer) -> dict[str, object]:
    """A client that returns the image's own bytes in place of a field's value.

    The bytes themselves, and no rendering of them into text: what this puts
    where the content type belongs is exactly what a client whose own answer
    carried that field as bytes would hand its caller. Decoding them into a
    string first — lossily or otherwise — would be a *string* substitution
    wearing an image's name, and would leave the one shape this is about
    untested.

    The published types are left alone. A defect is a client that answers
    something other than what the contract says, so the variant is built as the
    document a client answers with rather than by giving a generated type a
    field it does not have.
    """
    variant: dict[str, object] = json.loads(json.dumps(answered))
    record = cast("dict[str, object]", variant["record"])
    record["content_type"] = _bytes_of(answered)
    return variant


def test_the_equality_the_walk_asserts_refuses_a_client_that_carries_the_image(
    world: Supervisor,
) -> None:
    """The published client passes it; both variants are refused."""
    with Proxy(world.server) as proxy:
        client = Client(proxy.url, "operator")

        answered = client.image(world.image_id)
        sent = proxy.last().answer

        # The image is the one this world opened, so what the variants carry is
        # this print's own image rather than a file that happened to be there.
        equal(answered["record"]["print_id"], world.print_id)
        equal(answered["record"]["event_id"], world.event_id)

        # The published client passes it, which is what makes the two refusals
        # below about the variants rather than about the comparison.
        same("image", answered, sent)

        with_base64 = base64_of_the_image_into_a_string_field(answered)
        truth(
            with_base64["record"]["sha256"] != answered["record"]["sha256"],
            describing="the first variant to have changed something",
        )
        truth(
            not matches(with_base64, sent),
            describing="a client writing a base64 of the image into a string field to be refused",
        )

        # Not text, and not a decoding of the bytes into text: the image this
        # world stores opens `ff d8`, which no string of any encoding this
        # system speaks could carry, and what the variant puts in the field is
        # the byte sequence itself.
        bytes_answered = _bytes_of(answered)
        with pytest.raises(UnicodeDecodeError):
            bytes_answered.decode("utf-8")
        with_bytes = the_images_bytes_in_place_of_a_field(answered)
        substituted = cast("dict[str, object]", with_bytes["record"])["content_type"]
        equal(
            substituted,
            bytes_answered,
            describing="what the second variant put where the content type belongs",
        )
        truth(
            not matches(with_bytes, sent),
            describing="a client returning the image's bytes in place of a field to be refused",
        )

        equal(proxy.calls(), 1, describing="the calls that went through the proxy")
