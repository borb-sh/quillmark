from contextlib import contextmanager
from pathlib import Path
import pytest

from quillmark import Engine, Quill, QuillmarkError

WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
RESOURCES_PATH = WORKSPACE_ROOT / "crates" / "fixtures" / "resources"
QUILLS_PATH = RESOURCES_PATH / "quills"


def field(card, key):
    """Return the value of a named field from a card's payload_items list."""
    for item in card["payload_items"]:
        if item["type"] == "field" and item["key"] == key:
            return item["value"]
    return None


def has_field(card, key):
    """True when a named field exists in a card's payload_items."""
    return any(
        i["type"] == "field" and i["key"] == key for i in card["payload_items"]
    )


def field_keys(card):
    """Iterable of all field keys in a card, in source order."""
    return [i["key"] for i in card["payload_items"] if i["type"] == "field"]


def by_place(content):
    """`content` with each block, container and island id named by its place,
    as `Content::by_place` names them, so values of other histories that read
    alike compare alike."""
    ids = [b["id"] for b in content.get("blocks", [])]
    ids += [c["id"] for c in content.get("containers", [])]
    ids += [i["id"] for i in content.get("islands", [])]
    names = {}
    for n, i in enumerate(ids):
        names.setdefault(i, f"p{n}")
    name = lambda i: names.get(i, i)
    out = dict(content)
    if "blocks" in out:
        out["blocks"] = [{**b, "id": name(b["id"]), **({"path": [name(p) for p in b["path"]]} if "path" in b else {})} for b in out["blocks"]]
    if "containers" in out:
        out["containers"] = [{**c, "id": name(c["id"])} for c in out["containers"]]
    if "islands" in out:
        out["islands"] = [
            {**i, "id": name(i["id"]), **({"slots": {k: by_place(v) for k, v in i["slots"].items()}} if "slots" in i else {})}
            for i in out["islands"]
        ]
    return out


@contextmanager
def raises_edit_code(code):
    """Assert the block raises `QuillmarkError` whose primary diagnostic carries
    the given namespaced `edit::` code. Mutator identity rides on `code`, not on
    message text, so tests route on it; see prose/canon/ERROR.md."""
    with pytest.raises(QuillmarkError) as exc_info:
        yield exc_info
    assert exc_info.value.diagnostics[0].code == code


def make_card(kind, fields=None, body=""):
    """A card dict from a kind, a flat field mapping and a body — the shape a
    host writes for `insert_card`. `payload_items` is the wire's own form: one
    `{"type": "field", "key", "value"}` per field, in insertion order."""
    return {
        "kind": kind,
        "payload_items": [
            {"type": "field", "key": k, "value": v} for k, v in (fields or {}).items()
        ],
        "body": body,
    }


def _latest_version(quill_dir: Path) -> Path:
    """Return the latest versioned subdirectory of a quill, or the dir itself."""
    if (quill_dir / "Quill.yaml").exists():
        return quill_dir
    versions = sorted(
        (p.name for p in quill_dir.iterdir() if p.is_dir()),
        key=lambda v: [int(x) for x in v.split(".") if x.isdigit()],
    )
    if versions:
        return quill_dir / versions[-1]
    return quill_dir


def taro_quill():
    """The taro fixture quill (main text fields; a `quotes` card kind)."""
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "taro")))


def richtext_quill():
    """The sample_form fixture quill, reached for its richtext fields (headline:
    one-paragraph richtext, bio: multiline richtext)."""
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "sample_form")))


@pytest.fixture
def taro_quill_dir():
    """Return the path to the taro fixture quill (latest version).

    Tests must not mutate it in place.
    """
    fixture_path = _latest_version(QUILLS_PATH / "taro")

    assert fixture_path.exists(), f"Preferred fixture not found: {fixture_path}"

    return fixture_path


@pytest.fixture
def engine():
    return Engine()


TARO_MARKDOWN = '''~~~card-yaml
$quill: taro@0.1
$kind: main
author: Nibs
ice_cream: Taro
title: "My Favorite Ice Cream Flavor"
~~~

I love Taro ice cream for its subtly sweet, nutty flavor and creamy, earthy undertones.

~~~card-yaml
$kind: quotes
author: Albert Einstein
~~~
Without taro ice cream, life would be a mistake.
'''


@pytest.fixture
def taro_md():
    """Return a sample taro markdown document.

    The test owns its input: it does not depend on a bundled fixture file.
    """
    return TARO_MARKDOWN
