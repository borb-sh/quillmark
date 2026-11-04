"""Type stubs for the compiled `quillmark._quillmark` extension.

Signatures only, hand-kept in step with `src/types.rs` / `src/enums.rs`, which
carry the prose: pyo3 docstrings are runtime-only, so without this file the whole
surface resolves to `Any`.

A `Card` crosses as a plain dict and a payload value as whatever JSON maps to, so
both are `Any`-shaped: their schemas are the quill's, not Python's.
"""

from collections.abc import Sequence
from datetime import date
from pathlib import Path
from typing import Any, Literal, TypedDict, final, overload

__all__ = [
    "Artifact",
    "Diagnostic",
    "Document",
    "Engine",
    "Location",
    "OutputFormat",
    "Quill",
    "QuillmarkError",
    "Reader",
    "RenderResult",
    "Severity",
    "SyncState",
    "Writer",
    "diff",
]

@final
class QuillmarkError(Exception):
    diagnostics: list[Diagnostic]

@final
class Severity:
    ERROR: Severity
    WARNING: Severity

    @property
    def name(self) -> str: ...
    @staticmethod
    def all() -> list[Severity]: ...
    def __repr__(self) -> str: ...

@final
class OutputFormat:
    PDF: OutputFormat
    SVG: OutputFormat
    PNG: OutputFormat

    @property
    def name(self) -> str: ...
    @staticmethod
    def all() -> list[OutputFormat]: ...
    def __repr__(self) -> str: ...

@final
class Location:
    @property
    def file(self) -> str: ...
    @property
    def line(self) -> int: ...
    @property
    def column(self) -> int: ...

@final
class Diagnostic:
    @property
    def severity(self) -> Severity: ...
    @property
    def message(self) -> str: ...
    @property
    def code(self) -> str | None: ...
    @property
    def location(self) -> Location | None: ...
    @property
    def hint(self) -> str | None: ...
    @property
    def path(self) -> str | None: ...
    @property
    def args(self) -> dict[str, Any]: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

@final
class Artifact:
    @property
    def bytes(self) -> bytes: ...
    @property
    def format(self) -> OutputFormat: ...
    @property
    def mime_type(self) -> str: ...
    def save(self, path: str) -> None: ...

@final
class RenderResult:
    @property
    def artifacts(self) -> list[Artifact]: ...
    @property
    def warnings(self) -> list[Diagnostic]: ...
    @property
    def format(self) -> OutputFormat: ...
    @property
    def regions(self) -> list[dict[str, Any]]: ...

@final
class Document:
    def __new__(cls, quill_ref: str) -> Document: ...
    @staticmethod
    def parse(markdown: str) -> Document: ...
    @staticmethod
    def from_stored(json: str) -> Document: ...
    @staticmethod
    def storage_version_of(json: str) -> str | None: ...
    @staticmethod
    def current_storage_version() -> str: ...
    @staticmethod
    def markdown_rules() -> str: ...
    @staticmethod
    def quill_ref_hint() -> str: ...
    @overload
    def to_markdown(self, *, annotated: Literal[False] = False) -> str: ...
    @overload
    def to_markdown(self, *, annotated: Literal[True]) -> AnnotatedMarkdown: ...
    def to_stored(self) -> str: ...
    @property
    def quill_ref(self) -> str: ...
    def set_quill_ref(self, ref_str: str) -> None: ...
    def clone(self) -> Document: ...
    def equals(self, other: Document) -> bool: ...
    def __copy__(self) -> Document: ...
    def __deepcopy__(self, _memo: Any) -> Document: ...
    def __eq__(self, other: object, /) -> bool: ...
    def __repr__(self) -> str: ...
    @property
    def card_count(self) -> int: ...
    @property
    def warnings(self) -> list[Diagnostic]: ...
    @property
    def main(self) -> dict[str, Any]: ...
    @property
    def cards(self) -> list[dict[str, Any]]: ...
    def card(self, index: int) -> dict[str, Any]: ...
    def seed_overlay(self, kind: str) -> Any: ...
    def remove_field(self, name: str, card: int | str | None = None) -> Any: ...
    def store_ext(self, value: Any, card: int | str | None = None) -> None: ...
    def remove_ext(self, card: int | str | None = None) -> Any: ...
    def store_seed_overlay(self, card_kind: str, overlay: Any) -> None: ...
    def remove_seed_overlay(self, card_kind: str) -> Any: ...
    @property
    def unchecked(self) -> Unchecked: ...
    def remove_card(self, index: int) -> dict[str, Any] | None: ...
    def move_card(self, from_idx: int, to_idx: int) -> None: ...
    def anchor_contexts(self) -> list[AnchorContext]: ...
    def anchor_context(self, id: str) -> AnchorContext | None: ...
    def reattach(
        self,
        selectors: Sequence[AnchorContext],
        changes: dict[str, list[Patch]] | None = None,
    ) -> Reattached: ...
    def read(self, field: str | None = None, card: int | str | None = None) -> Content: ...
    @overload
    def to_markdown_at(
        self, field: str | None = None, card: int | str | None = None, *, annotated: Literal[False] = False
    ) -> str: ...
    @overload
    def to_markdown_at(
        self, field: str | None = None, card: int | str | None = None, *, annotated: Literal[True]
    ) -> AnnotatedExport: ...
    def apply(
        self,
        change: Change,
        field: str | None = None,
        card: str | None = None,
        *,
        heads: list[str] | None = None,
    ) -> Receipt: ...
    def revise(
        self,
        markdown: str,
        field: str | None = None,
        card: str | None = None,
        *,
        heads: list[str] | None = None,
    ) -> Receipt: ...
    def replace(
        self,
        content: Content,
        field: str | None = None,
        card: str | None = None,
        *,
        heads: list[str] | None = None,
    ) -> Receipt: ...
    def undo(
        self,
        receipt: Receipt,
        field: str | None = None,
        card: str | None = None,
    ) -> Receipt: ...
    def save(self) -> bytes: ...
    @staticmethod
    def load(data: bytes) -> Document: ...
    @property
    def actor(self) -> str | None: ...
    def set_actor(self, actor: str) -> None: ...
    def heads(self) -> list[str]: ...
    def save_since(self, heads: list[str]) -> bytes: ...
    def merge(self, data: bytes) -> Receipt: ...
    def generate_sync_message(self, state: SyncState) -> bytes | None: ...
    def receive_sync_message(self, state: SyncState, message: bytes) -> Receipt: ...

# What one peer of the sync protocol is known to hold. One serves one peer;
# `encode` keeps it across a restart.
@final
class SyncState:
    def __init__(self) -> None: ...
    def encode(self) -> bytes: ...
    @staticmethod
    def decode(data: bytes) -> SyncState: ...

# A content value: `text` (a `\n` opens a block, U+2028 is a hard break, U+FFFC
# an island's character), `blocks`, and the optional `containers`, `marks`,
# `anchors`, `islands` and `codec`, which a `text` field's value spells
# `"plain"`. Offsets count code points.
Content = dict[str, Any]

# An edit: `{"steps": [...]}`, each step a dict of one key naming it.
Change = dict[str, Any]

class ReceiptDroppedAnchor(AnchorContext, total=False):
    island: str
    slot: str

# One of Automerge's patches, as automerge-js spells it: `action` (`put`,
# `insert`, `splice`, `del`, `inc`, `conflict` or `mark`) and `path` from the
# document's root, a text index counting code points with each block marker
# and island one, so value offset `n` is index `n + 1` of the value's `text`.
Patch = dict[str, Any]

class Receipt(TypedDict):
    # The document's heads before the write, merge or sync message, which
    # `undo` takes a written value back to.
    before: list[str]
    # The document's heads after, the next write's `heads`.
    heads: list[str]
    # Automerge's patches for the change, in order.
    patches: list[Patch]
    dropped_anchors: list[ReceiptDroppedAnchor]
    repairs: list[dict[str, Any]]
    warnings: list[Diagnostic]

def diff(before: Content, after: Content) -> Change: ...

class Imported(TypedDict):
    content: Content
    warnings: list[Diagnostic]

class AnchorLine(TypedDict):
    id: str
    # The text of the line the anchor's start sits on, island slots removed,
    # or of the cell or note holding it.
    line: str

class AnnotatedExport(TypedDict):
    markdown: str
    anchors: list[AnchorLine]

class DocumentAnchorLine(AnchorLine):
    path: str
    card_id: str | None

class AnnotatedMarkdown(TypedDict):
    markdown: str
    anchors: list[DocumentAnchorLine]

def import_markdown(markdown: str) -> Imported: ...
@overload
def export_markdown(content: Content, *, annotated: Literal[False] = False) -> str: ...
@overload
def export_markdown(content: Content, *, annotated: Literal[True]) -> AnnotatedExport: ...
def normalize(content: Content) -> Content: ...
def is_inline(content: Content) -> bool: ...
def is_plain(content: Content) -> bool: ...

class Repaired(TypedDict):
    content: Content
    repairs: list[dict[str, Any]]
    # The change from the merged text to the content's: a position mapped
    # through it lands where repair puts an anchor there.
    change: Change

def repair(raw: dict[str, Any]) -> Repaired: ...
class AnchorContext(TypedDict):
    id: str
    path: str
    card_id: str | None
    quote: str
    prefix: str
    suffix: str

class Reattached(TypedDict):
    reattached: list[AnchorContext]
    orphaned: list[AnchorContext]

class CardAdded(TypedDict):
    id: str
    warnings: list[Diagnostic]

DocumentRevised = TypedDict(
    "DocumentRevised",
    {
        "dropped_anchors": list[AnchorContext],
        "warnings": list[Diagnostic],
        "from": list[int | None],
        "heads": list[str],
    },
)

@final
class Quill:
    @staticmethod
    def from_path(path: str | Path) -> Quill: ...
    @staticmethod
    def from_bundle(bytes: bytes) -> Quill: ...
    def to_bundle(self) -> bytes: ...
    @property
    def backend_id(self) -> str: ...
    @property
    def warnings(self) -> list[Diagnostic]: ...
    @property
    def quill_ref(self) -> str: ...
    @property
    def metadata(self) -> dict[str, Any]: ...
    @property
    def schema(self) -> Any: ...
    @property
    def authoring_schema(self) -> dict[str, Any]: ...
    def to_authoring(self, doc: Document) -> dict[str, Any]: ...
    def from_authoring(self, value: dict[str, Any]) -> Document: ...
    def writer(self, doc: Document) -> Writer: ...
    def reader(self, doc: Document) -> Reader: ...
    def validate(self, doc: Document) -> list[dict[str, Any]]: ...
    def parse(self, markdown: str) -> Document: ...
    def conform(self, doc: Document) -> list[dict[str, Any]]: ...
    def empty_document(self) -> Document: ...
    def example_document(self) -> Document | None: ...
    def seed_document(self) -> Document: ...
    def seed_main(self) -> dict[str, Any]: ...
    def seed_card(
        self, card_kind: str, overlay: Any | None = None
    ) -> dict[str, Any] | None: ...

@final
class Unchecked:
    def store_field(self, name: str, value: Any, card: int | str | None = None) -> None: ...
    def insert_card(
        self, card: Any, at: int | None = None
    ) -> str: ...

@final
class Writer:
    @property
    def document(self) -> Document: ...
    def set(self, name: str, value: Any, card: int | str | None = None) -> list[Diagnostic]: ...
    def set_all(self, fields: dict[str, Any], card: int | str | None = None) -> list[Diagnostic]: ...
    def revise(
        self,
        text: str,
        field: str | None = None,
        card: str | None = None,
        *,
        heads: list[str] | None = None,
    ) -> Receipt: ...
    def revise_document(
        self,
        markdown: str,
        *,
        heads: list[str] | None = None,
    ) -> DocumentRevised: ...
    def add_card(
        self,
        kind: str,
        fields: dict[str, Any] | None = None,
        body: str | None = None,
        at: int | None = None,
    ) -> CardAdded: ...
    def remove_card(self, index: int) -> dict[str, Any] | None: ...

@final
class Reader:
    @property
    def document(self) -> Document: ...
    def get(self, field: str | None = None, card: int | str | None = None) -> Any: ...
    def get_content(
        self, field: str | None = None, card: int | str | None = None
    ) -> dict[str, Any] | None: ...
    def get_content_at(
        self, name: str, path: Sequence[str | int], card: int | str | None = None
    ) -> dict[str, Any] | None: ...

class FontFace(TypedDict):
    family: str
    weight: int
    style: Literal["normal", "italic", "oblique"]
    index: int
    data: bytes

@final
class Engine:
    def __init__(self, fonts: list[bytes] | None = None) -> None: ...
    def render(
        self,
        quill: Quill,
        doc: Document,
        format: OutputFormat | None = None,
        ppi: float | None = None,
        pages: list[int] | None = None,
        regions: bool = False,
        today: date | None = None,
        title: str | None = None,
    ) -> RenderResult: ...
    def supported_formats(self, quill: Quill) -> list[OutputFormat]: ...
    def registered_backends(self) -> list[str]: ...
    def font_faces(self, quill: Quill) -> list[FontFace]: ...
