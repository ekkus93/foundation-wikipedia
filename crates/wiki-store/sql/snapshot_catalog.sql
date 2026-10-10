-- Immutable article snapshot catalog v1. Open read-only after verification.
-- The builder must verify shard digests before publishing this catalog.
PRAGMA application_id = 0x4657494B;
PRAGMA user_version = 1;
PRAGMA foreign_keys = ON;

CREATE TABLE shards (
  shard_id INTEGER PRIMARY KEY,
  filename TEXT NOT NULL UNIQUE
    CHECK (length(filename) BETWEEN 1 AND 255
      AND instr(filename, '/') = 0
      AND instr(filename, char(92)) = 0
      AND filename NOT IN ('.', '..')),
  sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
  bytes INTEGER NOT NULL CHECK (bytes > 0)
);

CREATE TABLE pages (
  project TEXT NOT NULL CHECK (length(project) BETWEEN 1 AND 64),
  page_id INTEGER NOT NULL CHECK (page_id > 0),
  revision_id INTEGER NOT NULL CHECK (revision_id >= 0),
  kind TEXT NOT NULL CHECK (kind IN ('article', 'redirect')),
  title TEXT NOT NULL CHECK (length(title) > 0),
  wikidata_id TEXT,
  shard_id INTEGER NOT NULL REFERENCES shards(shard_id),
  frame_offset INTEGER NOT NULL CHECK (frame_offset >= 0),
  frame_bytes INTEGER NOT NULL CHECK (frame_bytes BETWEEN 40 AND 33554472),
  frame_sha256 TEXT NOT NULL CHECK (length(frame_sha256) = 64 AND frame_sha256 NOT GLOB '*[^0-9a-f]*'),
  PRIMARY KEY (project, page_id, revision_id),
  UNIQUE (shard_id, frame_offset),
  CHECK ((kind = 'article' AND revision_id > 0) OR (kind = 'redirect' AND revision_id = 0))
) WITHOUT ROWID;

CREATE INDEX pages_title_lookup ON pages(project, title COLLATE NOCASE);
CREATE INDEX pages_wikidata_lookup ON pages(wikidata_id) WHERE wikidata_id IS NOT NULL;

CREATE TABLE aliases (
  project TEXT NOT NULL,
  page_id INTEGER NOT NULL,
  revision_id INTEGER NOT NULL,
  alias TEXT NOT NULL CHECK (length(alias) > 0),
  PRIMARY KEY (project, page_id, revision_id, alias),
  FOREIGN KEY (project, page_id, revision_id)
    REFERENCES pages(project, page_id, revision_id)
) WITHOUT ROWID;
CREATE INDEX aliases_title_lookup ON aliases(project, alias COLLATE NOCASE);

CREATE TABLE redirects (
  project TEXT NOT NULL,
  page_id INTEGER NOT NULL,
  revision_id INTEGER NOT NULL DEFAULT 0 CHECK (revision_id = 0),
  target_project TEXT NOT NULL CHECK (length(target_project) > 0),
  target_page_id INTEGER NOT NULL CHECK (target_page_id > 0),
  PRIMARY KEY (project, page_id, revision_id),
  FOREIGN KEY (project, page_id, revision_id)
    REFERENCES pages(project, page_id, revision_id)
) WITHOUT ROWID;
