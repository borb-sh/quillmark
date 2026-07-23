"""Quillmark - Python bindings for Quillmark."""

from ._quillmark import (
    Artifact,
    CardReader,
    CardWriter,
    Diagnostic,
    Document,
    Location,
    OutputFormat,
    Quill,
    Quillmark,
    QuillmarkError,
    Reader,
    RenderResult,
    Severity,
    Writer,
)

__all__ = [
    "Artifact",
    "CardReader",
    "CardWriter",
    "Diagnostic",
    "Document",
    "Location",
    "OutputFormat",
    "Quill",
    "Quillmark",
    "QuillmarkError",
    "Reader",
    "RenderResult",
    "Severity",
    "Writer",
]

try:
    from importlib.metadata import version as _version

    __version__ = _version("quillmark")
except Exception:  # pragma: no cover — source tree without installed metadata
    __version__ = "0.0.0"

