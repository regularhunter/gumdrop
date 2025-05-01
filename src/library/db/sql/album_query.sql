-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

SELECT
  DISTINCT t.album AS title,
  t.albumartist,
  SUM(t.duration) AS duration,
  t.year,
  (SELECT DISTINCT path FROM covers c WHERE same_dir(c.path, t.path) LIMIT 1) AS cover,
  COUNT(t.path) as num_tracks,
  (SELECT json_group_array(name)
    FROM (SELECT DISTINCT a.name
        FROM artists a JOIN tracks ON a.path = tracks.path
        WHERE t.album = tracks.album AND t.albumartist = tracks.albumartist)) AS artists,
    (SELECT json_group_array(name)
     FROM (SELECT DISTINCT g.name FROM genres g WHERE g.path = t.path)) AS genres
FROM
  tracks t
{} -- WHERE clause must be filled by format! macro
GROUP BY
  t.album,
  t.albumartist,
  t.year;

