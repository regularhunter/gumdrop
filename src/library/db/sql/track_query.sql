-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

SELECT
    track,
    title,
    discnumber AS disc,
    discsubtitle,
    albumartist,
    duration,
    path,
    album,
    GROUP_CONCAT(
        CASE WHEN name = albumartist THEN NULL ELSE name END, ", "
    ) AS artists
FROM
    tracks t
    NATURAL JOIN artists
{} -- WHERE clause must filled by format! macro
GROUP BY
    path
ORDER BY
    disc, track
