from contextlib import contextmanager
from pathlib import Path
import pytest

from quillmark import Quill, Quillmark, QuillmarkError

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
    """The taro fixture quill (main string fields; a `quotes` card kind)."""
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "taro")))


def richtext_form_quill():
    """The richtext_form fixture quill (headline: richtext inline, bio: richtext)."""
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "richtext_form")))


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
    return Quillmark()


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
