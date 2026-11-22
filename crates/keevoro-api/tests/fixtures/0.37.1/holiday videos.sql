-- The catalog of a second backup, "Holiday videos", as Keevoro 0.37.1
-- wrote it: nothing found yet, set to send the originals alone, with no
-- previews, and for videos only.

-- Schema step 1, exactly as 0.37.1 ran it.
CREATE TABLE sources (
         id          INTEGER PRIMARY KEY,
         name        TEXT NOT NULL,
         root        TEXT NOT NULL UNIQUE,
         other_files INTEGER NOT NULL DEFAULT 0,
         scanned_at  INTEGER NOT NULL
     );
     CREATE TABLE items (
         id          INTEGER PRIMARY KEY,
         source_id   INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
         path        TEXT NOT NULL,
         kind        TEXT NOT NULL,
         mime        TEXT NOT NULL,
         bytes       INTEGER NOT NULL,
         blake3      TEXT,
         date        TEXT,
         date_source TEXT,
         album       TEXT NOT NULL,
         folder_date TEXT,
         width       INTEGER,
         height      INTEGER,
         junk        TEXT
     );
     CREATE INDEX items_by_source ON items(source_id);
     CREATE INDEX items_by_hash ON items(blake3);
     CREATE TABLE skipped (
         source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
         path      TEXT NOT NULL,
         reason    TEXT NOT NULL
     );
     CREATE TABLE unreadable (
         source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
         path      TEXT NOT NULL,
         error     TEXT NOT NULL
     );
     CREATE TABLE choices (
         blake3 TEXT PRIMARY KEY,
         keep   INTEGER NOT NULL
     );;

-- Schema step 2, exactly as 0.37.1 ran it.
CREATE TABLE collected (
         blake3 TEXT PRIMARY KEY,
         path   TEXT NOT NULL,
         bytes  INTEGER NOT NULL
     );;

-- Schema step 3, exactly as 0.37.1 ran it.
CREATE TABLE renditions (
         blake3   TEXT PRIMARY KEY,
         ok       INTEGER NOT NULL,
         width    INTEGER,
         height   INTEGER,
         blurhash TEXT,
         reason   TEXT
     );
     CREATE TABLE uploaded (
         target TEXT NOT NULL,
         key    TEXT NOT NULL,
         bytes  INTEGER NOT NULL,
         PRIMARY KEY (target, key)
     );
     CREATE TABLE multipart (
         target    TEXT NOT NULL,
         key       TEXT NOT NULL,
         upload_id TEXT NOT NULL,
         PRIMARY KEY (target, key)
     );
     CREATE TABLE settings (
         key   TEXT PRIMARY KEY,
         value TEXT NOT NULL
     );;

-- Schema step 4, exactly as 0.37.1 ran it.
CREATE TABLE videos (
         blake3   TEXT PRIMARY KEY,
         ok       INTEGER NOT NULL,
         duration REAL,
         width    INTEGER,
         height   INTEGER,
         poster   INTEGER NOT NULL DEFAULT 0,
         mp4      INTEGER NOT NULL DEFAULT 0,
         reason   TEXT
     );;

-- Schema step 5, exactly as 0.37.1 ran it.
ALTER TABLE items ADD COLUMN hashed INTEGER NOT NULL DEFAULT 1;
     ALTER TABLE items ADD COLUMN quick TEXT;
     CREATE INDEX items_unhashed ON items(hashed) WHERE hashed = 0;;

-- Schema step 6, exactly as 0.37.1 ran it.
ALTER TABLE collected ADD COLUMN here INTEGER NOT NULL DEFAULT 1;;

-- Schema step 7, exactly as 0.37.1 ran it.
UPDATE items SET junk = NULL, width = NULL, height = NULL
     WHERE (lower(path) LIKE '%.dng' OR lower(path) LIKE '%.cr2' OR lower(path) LIKE '%.cr3' OR lower(path) LIKE '%.crw' OR lower(path) LIKE '%.nef' OR lower(path) LIKE '%.nrw' OR lower(path) LIKE '%.arw' OR lower(path) LIKE '%.srf' OR lower(path) LIKE '%.sr2' OR lower(path) LIKE '%.orf' OR lower(path) LIKE '%.rw2' OR lower(path) LIKE '%.raf' OR lower(path) LIKE '%.pef' OR lower(path) LIKE '%.srw' OR lower(path) LIKE '%.x3f' OR lower(path) LIKE '%.3fr' OR lower(path) LIKE '%.erf' OR lower(path) LIKE '%.kdc' OR lower(path) LIKE '%.mrw' OR lower(path) LIKE '%.raw' OR lower(path) LIKE '%.iiq' OR lower(path) LIKE '%.dcr' OR lower(path) LIKE '%.mef' OR lower(path) LIKE '%.mos' OR lower(path) LIKE '%.rwl');;

-- Schema step 8, exactly as 0.37.1 ran it.
CREATE INDEX IF NOT EXISTS collected_by_path ON collected(path);
     CREATE INDEX IF NOT EXISTS uploaded_by_key ON uploaded(key);;

INSERT INTO settings (key, value) VALUES ('previews', 'off');
INSERT INTO settings (key, value) VALUES ('wants', '"videos"');
PRAGMA user_version = 8;
