-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

CREATE TABLE IF NOT EXISTS covers(
    album TEXT NOT NULL,
    albumartist TEXT NOT NULL,
    cover TEXT NOT NULL,
    source_path TEXT,
    FOREIGN KEY (album, albumartist) REFERENCES tracks (album, albumartist) ON DELETE CASCADE
);

-- Swap the date field to TEXT from DATE.
ALTER TABLE tracks RENAME TO old_tracks;
CREATE TABLE tracks(
    title TEXT NOT NULL,
    track TEXT NOT NULL,
    discnumber TEXT,
    discsubtitle TEXT,
    album TEXT NOT NULL,
    albumartist TEXT,
    year TEXT,
    duration REAL NOT NULL,
    thumb TEXT,
    cover TEXT,
    path TEXT NOT NULL,
    PRIMARY KEY (path)
);
INSERT INTO tracks SELECT * FROM old_tracks;

ALTER TABLE tracks ADD COLUMN unique_cover BOOLEAN DEFAULT 0;

DROP VIEW IF EXISTS [Full Tracks];
DROP VIEW IF EXISTS [Albums];
DROP VIEW IF EXISTS [Album Artists];
DROP VIEW IF EXISTS [All Artists];

DROP TABLE old_tracks;

CREATE VIEW IF NOT EXISTS [Full Tracks] AS
SELECT track, title, discnumber as disc, discsubtitle, albumartist,
    duration, path, cover, album, unique_cover, GROUP_CONCAT(
        CASE WHEN name = albumartist THEN NULL ELSE name END, ", "
    ) AS artists
FROM tracks NATURAL JOIN artists
GROUP BY path ORDER BY disc, track;

CREATE VIEW IF NOT EXISTS [Albums] AS
SELECT DISTINCT album as title, albumartist, SUM(duration) as duration, year
FROM tracks GROUP BY album, albumartist;

CREATE VIEW IF NOT EXISTS [Album Artists] AS
SELECT DISTINCT albumartist, sort, COUNT(DISTINCT album)
FROM tracks NATURAL JOIN artists WHERE albumartist IS NOT NULL GROUP BY albumartist;

CREATE VIEW IF NOT EXISTS [All Artists] AS
SELECT DISTINCT name, sort, COUNT(DISTINCT album)
FROM tracks NATURAL JOIN artists GROUP BY name;

DROP TABLE IF EXISTS meta;
