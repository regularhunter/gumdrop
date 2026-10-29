-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

SELECT
    t.path,
    t.title,
    t.track,
    t.discnumber AS disc,
    t.discsubtitle,
    t.album,
    t.albumartist,
    t.year,
    t.duration,
    t.modified,
    -- Keep the order the artists and genres were inserted in so the main artist comes first
    (SELECT json_group_array(name) FROM
        (SELECT a.name FROM artists a WHERE a.path = t.path ORDER BY a.rowid)) AS artists,
    (SELECT json_group_array(name) FROM
        (SELECT g.name FROM genres g WHERE g.path = t.path ORDER BY g.rowid)) AS genres
FROM
    tracks t
{} -- WHERE clause must be filled by format! macro
ORDER BY
    {} -- ORDER BY terms are filled in by the format! macro
