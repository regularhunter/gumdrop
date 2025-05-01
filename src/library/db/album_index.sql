-- Querying tracks based on their albumartist and album values is significantly sped
-- up with the addition of this index, and querying the tracks table directly instead of
-- using the [Full Tracks] view greatly speeds up query times as well, so that view is
-- dropped because it is no longer used.
CREATE INDEX album_index ON tracks(albumartist, album);
DROP VIEW IF EXISTS [Full Tracks];

DROP VIEW IF EXISTS [Albums];
DROP VIEW [Album Artists];
DROP VIEW [All Artists];


ALTER TABLE tracks RENAME TO old_tracks;
CREATE TABLE tracks(
    title TEXT NOT NULL,
    track INTEGER NOT NULL,
    discnumber TEXT,
    discsubtitle TEXT,
    album TEXT NOT NULL,
    albumartist TEXT,
    year TEXT,
    duration REAL NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (path)
);

INSERT INTO tracks SELECT 
  title, track, discnumber, discsubtitle, album, 
  albumartist, year, duration, path, FROM old_tracks;

DROP TABLE old_tracks;

DROP TABLE covers;
CREATE TABLE covers (
  path TEXT NOT NULL,
  PRIMARY KEY (path) ON CONFLICT REPLACE
);

CREATE VIEW [Album Artists] AS
SELECT DISTINCT albumartist, 
  (CASE WHEN albumartist = "[Various Artists]" THEN NULL ELSE sort END) AS sort,
  COUNT(DISTINCT album)
FROM tracks NATURAL JOIN artists GROUP BY albumartist;

CREATE VIEW IF NOT EXISTS [All Artists] AS
SELECT DISTINCT name, sort, COUNT(DISTINCT album)
FROM tracks NATURAL JOIN artists GROUP BY name;
