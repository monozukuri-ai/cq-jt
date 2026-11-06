"""Errors shared with the native model boundary."""


class JtError(Exception):
    """Base error carrying a stable code and optional source information."""

    def __init__(self, message: str, code: str = "jt_error", source=None):
        super().__init__(message)
        self.code = code
        self.source = source

    @property
    def offset(self) -> int | None:
        return None if self.source is None else self.source.start_offset


class JtModelError(JtError, ValueError):
    """Invalid model field, source identity, byte domain, or metadata."""


class JtParseError(JtError, ValueError):
    """Invalid JT data, with a source range when available."""


class JtLimitError(JtParseError):
    """Input or declared count exceeds the configured resource limit."""


class JtUnsupportedError(JtError, NotImplementedError):
    """The requested version or feature is outside the supported profile."""


class JtReferenceError(JtParseError):
    """A required graph/property/segment reference cannot be resolved."""


class JtExportError(JtError, ValueError):
    """The requested output cannot be represented within the export contract."""
