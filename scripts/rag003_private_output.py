"""Exclusive private publication for preregistered RAG-003 reviewer files.

A completed byte stream becomes visible at the target path atomically, never
as a partially written file. The temporary file is mode 0600 on POSIX.
"""
import os
from pathlib import Path
import tempfile


def publish_private_bytes(destination: Path, payload: bytes) -> None:
    """Write private bytes without following or overwriting an existing target."""
    fd, temporary = tempfile.mkstemp(prefix=".rag003-private-", dir=destination.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        # Same-filesystem hard link gives exclusive, atomic publication: an
        # existing regular file or symlink cannot be clobbered by the exporter.
        os.link(temporary, destination)
    finally:
        os.unlink(temporary)
