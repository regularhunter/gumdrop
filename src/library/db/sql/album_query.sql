-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

SELECT
  t.album AS title,
  t.albumartist,
  SUM(t.duration) AS duration,
  t.year,
  (SELECT c.path FROM covers c WHERE same_dir(c.path, t.path) LIMIT 1) AS cover,
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
ORDER BY
  t.albumartist COLLATE NOCASE,
  t.album COLLATE NOCASE,
  t.year
