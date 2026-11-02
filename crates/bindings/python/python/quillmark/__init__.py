"""Python bindings for Quillmark, a schema-driven document framework."""

from ._quillmark import (
    Artifact,
    Diagnostic,
    Document,
    Engine,
    Location,
    OutputFormat,
    Quill,
    QuillmarkError,
    Reader,
    RenderResult,
    Severity,
    Unchecked,
    Writer,
    diff,
    invert,
    repair,
)

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
    "Unchecked",
    "Writer",
    "diff",
    "invert",
    "repair",
]

try:
    from importlib.metadata import version as _version

    __version__ = _version("quillmark")
except Exception:  # pragma: no cover, source tree without installed metadata
    __version__ = "0.0.0"

