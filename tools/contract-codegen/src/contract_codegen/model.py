"""The shapes a generated client is written from.

One model, three emitters. Everything a language-specific emitter needs to know
about a contract type is here, so that a shape the contracts grow is read once
rather than three times — and so that the three clients cannot come to disagree
about what a shape is by disagreeing about how to read it.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True, slots=True)
class Ref:
    """One of the contracts' own named types."""

    name: str


@dataclass(frozen=True, slots=True)
class Scalar:
    """A value with no name of its own: a string, a number, a flag."""

    #: `string`, `number`, `integer` or `boolean`.
    kind: str


@dataclass(frozen=True, slots=True)
class ListOf:
    """A sequence of one shape."""

    item: TypeExpr


@dataclass(frozen=True, slots=True)
class MapOf:
    """An object keyed by strings, whose values are all one shape."""

    value: TypeExpr


@dataclass(frozen=True, slots=True)
class Nullable:
    """A shape that may also be absent or null."""

    inner: TypeExpr


TypeExpr = Ref | Scalar | ListOf | MapOf | Nullable


@dataclass(frozen=True, slots=True)
class Field:
    """One property of an object shape."""

    name: str
    type: TypeExpr
    required: bool
    doc: str


@dataclass(frozen=True, slots=True)
class Variant:
    """One arm of a union, in one of the three shapes an arm takes.

    An arm carrying nothing travels as its own tag. An arm carrying one shape
    that has a name of its own travels as that shape under the tag, and is
    `payload`. An arm carrying an object nobody named travels as that object's
    own properties under the tag, and is `fields`.
    """

    tag: str
    doc: str
    fields: tuple[Field, ...] = ()
    payload: TypeExpr | None = None

    @property
    def unit(self) -> bool:
        """Whether this arm carries nothing but its own tag."""
        return not self.fields and self.payload is None


@dataclass(frozen=True, slots=True)
class Alias:
    """A named type that is another shape under a name of its own."""

    name: str
    target: TypeExpr
    doc: str


@dataclass(frozen=True, slots=True)
class Enumeration:
    """A closed set of strings."""

    name: str
    doc: str
    #: Each admitted string, with the sentence the contracts give for it.
    values: tuple[tuple[str, str], ...] = ()


@dataclass(frozen=True, slots=True)
class Struct:
    """An object with named properties.

    An object may also carry a closed set of arms beside its own properties:
    the event record is one object with a `kind` naming which payload it
    carries, and every arm shares the properties every event has. `variants` is
    that set and `tag_field` the property its tag travels in, both empty for an
    object that is only its properties.
    """

    name: str
    doc: str
    fields: tuple[Field, ...] = ()
    variants: tuple[Variant, ...] = ()
    tag_field: str = ""

    @property
    def tagged(self) -> bool:
        """Whether this object carries a closed set of arms beside its fields."""
        return bool(self.tag_field)


@dataclass(frozen=True, slots=True)
class Union:
    """A closed set of arms, distinguished by a tag.

    `tag_field` names the property the tag travels in for an *internally*
    tagged union — the action vocabulary is the one of those. Where it is
    empty the union is externally tagged: an arm with a payload travels as a
    one-property object whose property is the arm's own name, and an arm
    without one travels as that name.
    """

    name: str
    doc: str
    variants: tuple[Variant, ...] = ()
    tag_field: str = ""

    @property
    def internally_tagged(self) -> bool:
        """Whether the tag travels as a property beside the payload."""
        return bool(self.tag_field)


Declaration = Alias | Enumeration | Struct | Union


@dataclass(frozen=True, slots=True)
class Parameter:
    """One value a request to an operation carries."""

    name: str
    type: TypeExpr
    required: bool
    #: `path`, `query` or `body`.
    located: str

    @property
    def optional(self) -> bool:
        """Whether a caller may leave this value out.

        Two ways say the same thing: a value a request need not carry, and one
        declared as its own shape or null. A generated method takes either as
        one optional argument rather than as two different ones.
        """
        return not self.required or isinstance(self.type, Nullable)

    @property
    def carried(self) -> TypeExpr:
        """The shape this value has when it is there at all."""
        return self.type.inner if isinstance(self.type, Nullable) else self.type


@dataclass(frozen=True, slots=True)
class Operation:
    """One operation the server serves, as a client calls it."""

    name: str
    method: str
    path: str
    #: `read`, `write`, or the action of the vocabulary it asks for.
    effect: str
    #: The shape a successful answer carries.
    answer: str
    #: The shape the policy's own rejection carries, for an operation that can
    #: be rejected.
    rejection: str
    parameters: tuple[Parameter, ...]
    image_path_field: str

    @property
    def mutating(self) -> bool:
        """Whether this operation changes something, so it carries a reason."""
        return self.effect != "read"

    @property
    def rejectable(self) -> bool:
        """Whether the policy can refuse this operation and answer the refusal."""
        return bool(self.rejection)

    def supplied(self) -> tuple[Parameter, ...]:
        """Every value a caller of a generated method supplies, in method order.

        `actor` is not one of them: who a client is acting as is what the
        client was constructed with, so a method that took it again would let
        one call act as somebody the client is not. Required values come
        before optional ones, so that a language whose optional arguments must
        come last and one whose need not agree on the order.
        """
        taken = [parameter for parameter in self.parameters if parameter.name != ACTOR_PARAMETER]
        ordered = [parameter for parameter in taken if parameter.located == "path"]
        rest = [parameter for parameter in taken if parameter.located != "path"]
        ordered.extend(parameter for parameter in rest if not parameter.optional)
        ordered.extend(parameter for parameter in rest if parameter.optional)
        return tuple(ordered)


#: The value every mutating request carries saying who is asking, which a
#: client supplies from what it was constructed with rather than per call.
ACTOR_PARAMETER = "actor"

#: The value every mutating request carries saying why, which a client refuses
#: to send when it is empty — before the request is made at all.
REASON_PARAMETER = "reason"


@dataclass(frozen=True, slots=True)
class Contract:
    """Everything the three clients are generated from."""

    #: The version of the contract the clients are generated against, which is
    #: the version the type crate declares in the tree they were generated from.
    version: str
    version_prefix: str
    media_type: str
    declarations: tuple[Declaration, ...] = ()
    operations: tuple[Operation, ...] = ()
    #: Every declaration by name, for an emitter following a reference.
    by_name: dict[str, Declaration] = field(default_factory=dict)
