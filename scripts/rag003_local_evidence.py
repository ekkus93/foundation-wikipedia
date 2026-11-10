"""Strict local byte receipts for RAG-003 submissions; never publisher authentication.

The caller provides a separate inventory mapping reviewed source identities and
case IDs to local files. These files are not trusted merely because they match a
reviewer's SHA-256 claims; exact pinned acquisition provenance remains external.
"""
import codecs
import hashlib
import hmac
import os
import stat
import unicodedata

MAX_SOURCE_BYTES = 256 * 1024 * 1024
MAX_ANSWER_BYTES = 16 * 1024 * 1024


def _read_regular(root_fd, relative, limit, capture, needles=(), validate_utf8=False):
    """Secure openat walk and bounded single-descriptor hash; refuse symlinks."""
    if (not isinstance(relative, str) or not 0 < len(relative) <= 512
            or relative.startswith("/") or "\\" in relative or "\0" in relative):
        raise ValueError("invalid relative evidence path")
    # A local inventory is operator-controlled, but misleading path labels can
    # conceal which evidence was checked in receipts or terminal diagnostics.
    # Reject invisible formatting, bidi controls and non-printable components.
    if any(unicodedata.category(char) in ("Cc", "Cf", "Cs")
           for char in relative):
        raise ValueError("evidence path contains invisible or control characters")
    parts = relative.split("/")
    if any(part in ("", ".", "..") for part in parts):
        raise ValueError("invalid relative evidence path")
    directory = root_fd
    try:
        for name in parts[:-1]:
            next_fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                              dir_fd=directory)
            if directory != root_fd:
                os.close(directory)
            directory = next_fd
        fd = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                     dir_fd=directory)
        try:
            before = os.fstat(fd)
            if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= limit:
                raise ValueError("empty, oversized or non-regular evidence file")
            digest = hashlib.sha256()
            decoder = codecs.getincrementaldecoder("utf-8")("strict") if validate_utf8 else None
            chunks = [] if capture else None
            # Search source quotations incrementally; do not buffer 256 MiB
            # per source revision. Retain only the longest needle's boundary.
            matches = {needle: False for needle in needles}
            overlap = max((len(needle) for needle in needles), default=1) - 1
            tail = b""
            total = 0
            while True:
                block = os.read(fd, min(65536, limit - total + 1))
                if not block:
                    break
                total += len(block)
                if total > limit:
                    raise ValueError("evidence file exceeds byte budget")
                digest.update(block)
                if decoder is not None:
                    decoder.decode(block, final=False)
                if matches:
                    window = tail + block
                    for needle in matches:
                        if not matches[needle] and needle in window:
                            matches[needle] = True
                    tail = window[-overlap:] if overlap else b""
                if capture:
                    chunks.append(block)
            if decoder is not None:
                decoder.decode(b"", final=True)
            after = os.fstat(fd)
            fields = ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns")
            if total != before.st_size or any(getattr(before, f) != getattr(after, f)
                                               for f in fields):
                raise ValueError("evidence file changed while being verified")
            return digest.hexdigest(), (after.st_dev, after.st_ino), (
                b"".join(chunks) if capture else None), matches
        finally:
            os.close(fd)
    finally:
        if directory != root_fd:
            os.close(directory)


def _mapping_rows(value, required, kind, limit):
    if not isinstance(value, list) or not 0 < len(value) <= limit:
        raise ValueError(f"invalid {kind} evidence inventory")
    selected = {}
    for i, record in enumerate(value):
        if not isinstance(record, dict) or set(record) != required:
            raise ValueError(f"{kind}[{i}]: unexpected or missing evidence fields")
        key = ((record["project"], record["page_id"], record["revision_id"])
               if kind == "sources" else record["case_id"])
        if kind == "sources" and (
                type(key[0]) is not str or type(key[1]) is not int
                or type(key[2]) is not int):
            raise ValueError(f"{kind}[{i}]: invalid source key types")
        if kind == "answers" and (not isinstance(key, str) or not key):
            raise ValueError(f"{kind}[{i}]: invalid case key")
        if key in selected:
            raise ValueError(f"{kind}[{i}]: duplicated source or case")
        selected[key] = record["path"]
    return selected


def verify_local_bytes(submission, artifact_map, root):
    """Verify all and only submitted source and answer files and UTF-8 spans."""
    if (not hasattr(os, "O_NOFOLLOW") or not hasattr(os, "O_DIRECTORY")
            or not hasattr(os, "O_NONBLOCK")):
        raise ValueError("strict evidence verification requires POSIX openat")
    if (not isinstance(artifact_map, dict) or
            set(artifact_map) != {"schema_version", "sources", "answers"} or
            type(artifact_map["schema_version"]) is not int
            or artifact_map["schema_version"] != 1):
        raise ValueError("invalid evidence map schema")
    sources = _mapping_rows(artifact_map["sources"],
                            {"project", "page_id", "revision_id", "path"},
                            "sources", 10000)
    answers = _mapping_rows(artifact_map["answers"], {"case_id", "path"},
                            "answers", 50000)
    records = submission["judgments"]
    expected_sources = {(item["project"], item["page_id"], item["revision_id"])
                        for item in records}
    expected_cases = {item["case_id"] for item in records}
    if set(sources) != expected_sources or set(answers) != expected_cases:
        raise ValueError("evidence map must contain exactly submitted sources and cases")
    expected_source_digests = {(item["project"], item["page_id"], item["revision_id"]):
                               item["source_artifact_sha256"] for item in records}
    expected_answer_digests = {item["case_id"]: item["answer_sha256"]
                              for item in records}
    quotations = {}
    for item in records:
        key = (item["project"], item["page_id"], item["revision_id"])
        quotations.setdefault(key, set()).add(item["source_span"].encode("utf-8"))
    # Literal-search work scales with distinct quotations times source blocks.
    # Fail before opening any evidence files if an adversarial submission
    # requests an unbounded number of comparisons against a 256 MiB source.
    MAX_DISTINCT_QUOTES_PER_SOURCE = 1024
    MAX_QUOTE_BYTES_PER_SOURCE = 1024 * 1024
    for key, spans in quotations.items():
        if (len(spans) > MAX_DISTINCT_QUOTES_PER_SOURCE or
                sum(map(len, spans)) > MAX_QUOTE_BYTES_PER_SOURCE):
            raise ValueError("source quotation search budget exceeded")
    root_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    seen_inodes = {}
    try:
        for key, path in sources.items():
            actual, inode, _, matched = _read_regular(
                root_fd, path, MAX_SOURCE_BYTES, False, quotations[key], validate_utf8=True)
            if inode in seen_inodes:
                raise ValueError("evidence files alias the same physical inode")
            seen_inodes[inode] = ("source", key)
            if not hmac.compare_digest(actual, expected_source_digests[key]):
                raise ValueError("source artifact SHA-256 mismatch")
            if not all(matched.values()):
                raise ValueError("quoted source span absent from source bytes")
        # Keep at most one answer in memory at a time, rather than retaining
        # every case's 16 MiB payload for the duration of the submission.
        case_claims = {}
        for index, item in enumerate(records):
            case_claims.setdefault(item["case_id"], []).append((index, item))
        for key, path in answers.items():
            actual, inode, raw, _ = _read_regular(root_fd, path, MAX_ANSWER_BYTES, True)
            if inode in seen_inodes:
                raise ValueError("evidence files alias the same physical inode")
            seen_inodes[inode] = ("answer", key)
            if not hmac.compare_digest(actual, expected_answer_digests[key]):
                raise ValueError("answer artifact SHA-256 mismatch")
            # Decode the complete answer before inspecting any of its claims.
            raw.decode("utf-8")
            for index, item in case_claims[key]:
                start, end = item["claim_start_byte"], item["claim_end_byte"]
                if end > len(raw):
                    raise ValueError(f"judgments[{index}]: claim exceeds actual answer bytes")
                try:
                    span = raw[start:end].decode("utf-8")
                except UnicodeDecodeError as exc:
                    raise ValueError(f"judgments[{index}]: claim splits UTF-8 character") from exc
                if not any(char.isalnum() for char in span):
                    raise ValueError(f"judgments[{index}]: claim contains no lexical content")
    finally:
        os.close(root_fd)
    return len(sources), len(answers), len(records)
