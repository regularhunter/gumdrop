-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

SELECT
  a.title,
  a.albumartist,
  a.duration,
  a.year,
  (SELECT c.path FROM covers c WHERE same_dir(c.path, a.sample_track)
    ORDER BY c.path LIMIT 1) AS cover,
  a.num_tracks,
  a.artists,
  a.genres,
  a.sample_track
FROM (
  SELECT
    t.album AS title,
    t.albumartist,
    SUM(t.duration) AS duration,
    t.year AS year,
    COUNT(t.path) AS num_tracks,
    (SELECT json_group_array(name)
      FROM (SELECT DISTINCT a.name
          FROM artists a JOIN tracks t2 ON a.path = t2.path
          WHERE t2.album = t.album AND t2.albumartist IS t.albumartist
            AND t2.year IS t.year
          ORDER BY a.name)) AS artists,
    (SELECT json_group_array(name)
      FROM (SELECT DISTINCT g.name
          FROM genres g JOIN tracks t2 ON g.path = t2.path
          WHERE t2.album = t.album AND t2.albumartist IS t.albumartist
            AND t2.year IS t.year
          ORDER BY g.name)) AS genres,
    MIN(t.path) AS sample_track
  FROM
    tracks t
  {} -- WHERE clause must be filled by format! macro
  GROUP BY
    t.album,
    t.albumartist,
    t.year
) AS a
ORDER BY
  a.albumartist COLLATE NOCASE,
  a.title COLLATE NOCASE,
  a.year
