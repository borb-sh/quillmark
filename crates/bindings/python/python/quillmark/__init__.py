"""Quillmark - Python bindings for Quillmark."""

from ._quillmark import (
    Artifact,
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
except Exception:  # pragma: no cover, source tree without installed metadata
    __version__ = "0.0.0"

