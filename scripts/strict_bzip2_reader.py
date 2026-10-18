"""Bounded streaming bzip2 reader which validates every compressed frame.

Unlike bz2.BZ2File, a trailing invalid frame is never silently ignored.
The caller must read through EOF (or use this context manager).
"""
import bz2


class StrictBzip2Reader:
    """Expand concatenated bzip2 frames with bounded per-read memory."""

    def __init__(self, source):
        self.source = source
        self.decoder = bz2.BZ2Decompressor()
        self.buffered = b""
        self.finished = False

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        if exc_type is None and not self.finished:
            raise EOFError("compressed bzip2 stream was not fully consumed")
        return False

    def read(self, size=-1):
        if size < 0:
            size = 65536
        if size == 0:
            return b""
        output = bytearray()
        while len(output) < size and not self.finished:
            if self.decoder.eof:
                self.buffered = self.decoder.unused_data
                if not self.buffered:
                    self.buffered = self.source.read(65536)
                if not self.buffered:
                    self.finished = True
                    break
                self.decoder = bz2.BZ2Decompressor()
            if self.decoder.needs_input:
                block = self.buffered or self.source.read(65536)
                self.buffered = b""
                if not block:
                    raise EOFError("truncated bzip2 frame")
            else:
                block = b""
            output.extend(self.decoder.decompress(block, max_length=size - len(output)))
        return bytes(output)
