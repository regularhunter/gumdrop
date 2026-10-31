-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

-- Fills in the albumartist column for tracks whose files have no album artist
-- tag. It is set to the name of a single artist that is present on all tracks
-- in the album, or to '[Various Artists]' if there isn't one. Tracks without an
-- album are left alone. The '[Various Artists]' literal must match
-- VARIOUS_ARTISTS in librarydb.rs

-- Forget previously derived album artists, since the tracks of an album may
-- have changed since they were computed
UPDATE tracks SET albumartist = NULL, derived_albumartist = 0
WHERE derived_albumartist = 1;

-- Compute the values first so the UPDATE below doesn't read rows it is changing
DROP TABLE IF EXISTS temp.derived_albumartists;
CREATE TEMP TABLE derived_albumartists AS
SELECT
  albums.album AS album,
  CASE
    WHEN COUNT(shared.name) = 1 THEN MIN(shared.name)
    -- No artist tags at all so there is nothing to derive from
    WHEN NOT EXISTS (SELECT 1 FROM tracks t JOIN artists a ON a.path = t.path
        WHERE t.album = albums.album AND t.albumartist IS NULL) THEN NULL
    ELSE '[Various Artists]'
  END AS albumartist
FROM
  (SELECT album, COUNT(*) AS num_tracks FROM tracks
    WHERE albumartist IS NULL AND album != '' GROUP BY album) AS albums
  LEFT JOIN (
    -- Artists credited on every untagged track of the album
    SELECT t.album AS album, a.name AS name, COUNT(DISTINCT t.path) AS num_tracks
    FROM tracks t JOIN artists a ON a.path = t.path
    WHERE t.albumartist IS NULL AND t.album != ''
    GROUP BY t.album, a.name
  ) AS shared
  ON shared.album = albums.album AND shared.num_tracks = albums.num_tracks
GROUP BY albums.album;

UPDATE tracks SET
  derived_albumartist = 1,
  albumartist = (SELECT d.albumartist FROM temp.derived_albumartists d
    WHERE d.album = tracks.album)
WHERE albumartist IS NULL AND album != '';

DROP TABLE temp.derived_albumartists;
