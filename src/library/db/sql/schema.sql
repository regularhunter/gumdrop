-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

-- Initial library schema version 1
-- Later schema changes must be added as new migrations in librarydb.rs
-- instead of editing this file so existing databases can be upgraded

CREATE TABLE tracks(
    path TEXT NOT NULL,
    title TEXT NOT NULL,
    track INTEGER,
    discnumber INTEGER,
    discsubtitle TEXT,
    album TEXT NOT NULL DEFAULT '',
    albumartist TEXT,
    derived_albumartist INTEGER NOT NULL DEFAULT 0,
    year TEXT,
    duration REAL NOT NULL,
    modified REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (path)
);

CREATE INDEX album_index ON tracks(albumartist, album);
CREATE INDEX album_title_index ON tracks(album);

CREATE TABLE artists(
    name TEXT NOT NULL,
    sort TEXT,
    path TEXT NOT NULL,
    PRIMARY KEY (path, name) ON CONFLICT REPLACE,
    FOREIGN KEY (path) REFERENCES tracks(path) ON DELETE CASCADE
);
CREATE INDEX artist_name_index ON artists(name);

CREATE TABLE genres(
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (name, path) ON CONFLICT REPLACE,
    FOREIGN KEY (path) REFERENCES tracks(path) ON DELETE CASCADE
);
CREATE INDEX genre_path_index ON genres(path);

CREATE TABLE covers(
    path TEXT NOT NULL,
    PRIMARY KEY (path) ON CONFLICT REPLACE
);

CREATE VIEW [Album Artists] AS
SELECT
    t.albumartist AS name,
    (SELECT a.sort FROM artists a
        WHERE a.name = t.albumartist AND a.sort IS NOT NULL LIMIT 1) AS sort,
    COUNT(DISTINCT t.album) AS num_albums
FROM tracks t
WHERE t.albumartist IS NOT NULL
GROUP BY t.albumartist;

CREATE VIEW [All Artists] AS
SELECT
    a.name AS name,
    MAX(a.sort) AS sort,
    COUNT(DISTINCT t.album) AS num_albums
FROM artists a JOIN tracks t ON a.path = t.path
GROUP BY a.name;
