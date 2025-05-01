CREATE TABLE IF NOT EXISTS tracks(
    title TEXT,
    track TEXT,
    discnumber TEXT,
    discsubtitle TEXT,
    album TEXT,
    albumartist TEXT,
    year TEXT,
    duration REAL NOT NULL,
    filetype TEXT,
    genre TEXT,
    cover TEXT,
    liked INTEGER NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (path)
);

CREATE TABLE IF NOT EXISTS artists(
    name TEXT NOT NULL,
    sort TEXT,
    path TEXT NOT NULL,
    PRIMARY KEY (path, name) ON CONFLICT REPLACE,
    FOREIGN KEY (path) REFERENCES tracks(path) ON DELETE CASCADE
);

CREATE VIEW IF NOT EXISTS [Full Tracks] AS
SELECT track, title, discnumber as disc, discsubtitle, albumartist,
    duration, path, cover, album, GROUP_CONCAT(
        CASE WHEN name = albumartist THEN NULL ELSE name END, ", "
    ) AS artists
FROM tracks NATURAL JOIN artists GROUP BY path ORDER BY disc, track;

CREATE VIEW IF NOT EXISTS [Albums] AS
SELECT DISTINCT album as title, albumartist, SUM(duration) as duration, year
FROM tracks GROUP BY album, albumartist;

CREATE VIEW IF NOT EXISTS [Album Artists] AS
SELECT DISTINCT albumartist, sort, COUNT(DISTINCT album)
FROM tracks NATURAL JOIN artists WHERE albumartist IS NOT NULL GROUP BY albumartist;

CREATE VIEW IF NOT EXISTS [All Artists] AS
SELECT DISTINCT name, sort, COUNT(DISTINCT album)
FROM tracks GROUP BY name;
