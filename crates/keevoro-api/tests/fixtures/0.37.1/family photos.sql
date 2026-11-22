-- The catalog of a backup called "Family photos", as Keevoro 0.37.1 wrote
-- it: schema version 8, and every kind of thing a person sets or Keevoro
-- records, each in the form 0.37.1 kept it. Placeholders in braces are
-- filled in by the test: {CARD} is a path as it is, {json:CARD} the
-- same written inside JSON.

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

INSERT INTO sources (id, name, root, other_files, scanned_at) VALUES (1, 'Mum''s card', '{CARD}', 2, 1794000000);
INSERT INTO sources (id, name, root, other_files, scanned_at) VALUES (2, 'Beach', '{BEACH}', 0, 1794000100);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (1, '{CARD}/DCIM/IMG_20240601_100000.jpg', 'photo', 'image/jpeg', 4400000, 'a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1', '2024-06-01T10:00:00', 'exif', '["DCIM"]', NULL, 4000, 3000, NULL, 1, NULL);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (1, '{CARD}/DCIM/IMG_20240601_100500.jpg', 'photo', 'image/jpeg', 4400000, 'b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2', '2024-06-01T10:05:00', 'exif', '["DCIM"]', NULL, 4000, 3000, NULL, 1, NULL);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (1, '{CARD}/DCIM/IMG_20240602_090000.jpg', 'photo', 'image/jpeg', 4300000, 'c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3', '2024-06-02T09:00:00', 'exif', '["DCIM"]', NULL, 4000, 3000, NULL, 1, NULL);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (1, '{CARD}/DCIM/screen.png', 'photo', 'image/png', 900000, 'd4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4', NULL, NULL, '["DCIM"]', NULL, 1920, 1080, NULL, 1, NULL);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (1, '{CARD}/DCIM/VID_20240601_101000.mp4', 'video', 'video/mp4', 52000000, 'e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5', '2024-06-01T10:10:00', 'video', '["DCIM"]', NULL, 1920, 1080, NULL, 1, NULL);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (1, '{CARD}/Old/IMG_20100704_120000.jpg', 'photo', 'image/jpeg', 2100000, 'f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6', '2010-07-04T12:00:00', 'exif', '["Old"]', NULL, 2048, 1536, NULL, 1, NULL);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (2, '{BEACH}/DJI_0001.MP4', 'video', 'video/mp4', 310000000, '0707070707070707070707070707070707070707070707070707070707070707', '2024-06-01T11:00:00', 'video', '[]', NULL, 3840, 2160, NULL, 1, NULL);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (2, '{BEACH}/DJI_0002.MP4', 'video', 'video/mp4', 290000000, '1818181818181818181818181818181818181818181818181818181818181818', '2024-06-01T11:20:00', 'video', '[]', NULL, 3840, 2160, NULL, 1, NULL);
INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk, hashed, quick) VALUES (2, '{BEACH}/DJI_0003.JPG', 'photo', 'image/jpeg', 8100000, '2929292929292929292929292929292929292929292929292929292929292929', '2024-06-01T11:30:00', 'exif', '[]', NULL, 4000, 3000, NULL, 1, NULL);
INSERT INTO skipped (source_id, path, reason) VALUES (1, '{CARD}/.Trashes', 'recycle_bin');
INSERT INTO unreadable (source_id, path, error) VALUES (1, '{CARD}/DCIM/IMG_0013.jpg', 'The file is damaged');
-- One photo the person chose not to keep.
INSERT INTO choices (blake3, keep) VALUES ('c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3', 0);
-- Where the two photos backed up were placed, and that they reached the folder.
INSERT INTO collected (blake3, path, bytes, here) VALUES ('a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1', 'originals/DCIM/IMG_20240601_100000.jpg', 4400000, 0);
INSERT INTO collected (blake3, path, bytes, here) VALUES ('b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2', 'originals/DCIM/IMG_20240601_100500.jpg', 4400000, 0);
INSERT INTO uploaded (target, key, bytes) VALUES ('folder:{USB}', 'originals/DCIM/IMG_20240601_100000.jpg', 4400000);
INSERT INTO uploaded (target, key, bytes) VALUES ('folder:{USB}', 'originals/DCIM/IMG_20240601_100500.jpg', 4400000);
INSERT INTO renditions (blake3, ok, width, height, blurhash, reason) VALUES ('a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1', 1, 1600, 1200, 'LEHV6nWB2yk8pyo0adR*.7kCMdnj', NULL);
INSERT INTO renditions (blake3, ok, width, height, blurhash, reason) VALUES ('b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2', 0, NULL, NULL, NULL, 'This JPEG is damaged');
-- The backup's settings.
INSERT INTO settings (key, value) VALUES ('destinations', '[{"kind":"folder","place":"{json:USB}","target":"folder:{json:USB}"},{"kind":"r2","place":"0123456789abcdef0123456789abcdef","bucket":"family-photos","target":"r2:0123456789abcdef0123456789abcdef/family-photos"}]');
INSERT INTO settings (key, value) VALUES ('destination', '{"kind":"folder","place":"{json:USB}","target":"folder:{json:USB}"}');
INSERT INTO settings (key, value) VALUES ('index_key:folder:{USB}', 'index/Q9kTt3yXo0bVqUe2Ww1zRa.kvx');
INSERT INTO settings (key, value) VALUES ('wants', '"photos"');
INSERT INTO settings (key, value) VALUES ('wants:{BEACH}', '"videos"');
INSERT INTO settings (key, value) VALUES ('left_out_folders', '["{json:CARD}/Old"]');
INSERT INTO settings (key, value) VALUES ('left_out_types:{CARD}', '["png"]');
INSERT INTO settings (key, value) VALUES ('online_only:{CARD}', '3');
INSERT INTO settings (key, value) VALUES ('send_history', '[{"finishedAt":1794000500,"seconds":96,"bytes":9000000,"to":"the folder {json:USB}","stopped":false,"detail":{"startedAt":1794000404,"pausedSeconds":0,"ended":"finished","startedBy":"the app","version":"0.37.1","destinations":[{"to":"the folder {json:USB}","overNetwork":false,"reached":true,"seconds":90,"bytes":9000000,"originals":{"sent":2,"sentBytes":8800000,"alreadyThere":0,"notSent":[],"notSentMore":0},"indexed":2}],"stages":[{"kind":"sendingOriginals","to":"the folder {json:USB}","unit":"bytes","done":8800000,"seconds":80,"finished":true,"perSecond":110000.0,"slowest":null,"fastest":null}],"duplicates":1,"duplicateBytes":4000000,"summary":["Safely backed up 2 files (8.8 MB).","That took about 2 minutes: 9.0 MB at an average of 93.8 KB a second."]}},{"finishedAt":1793900000,"seconds":40,"bytes":0,"to":"the folder {json:USB}","stopped":true}]');
INSERT INTO settings (key, value) VALUES ('last_backup_said', '{"finished":true,"summary":["Safely backed up 2 files (8.8 MB).","That took about 2 minutes: 9.0 MB at an average of 93.8 KB a second."],"outcome":"worked"}');
PRAGMA user_version = 8;
