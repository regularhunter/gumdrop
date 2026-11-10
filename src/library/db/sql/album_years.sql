-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-License-Identifier: GPL-3.0-or-later

-- Albums are told apart by year as well as by title and album artist so that
-- an album and its remaster are two albums. Count them that way; json_array()
-- keeps a missing year apart from every year
DROP VIEW [Album Artists];
CREATE VIEW [Album Artists] AS
SELECT
    t.albumartist AS name,
    (SELECT a.sort FROM artists a
        WHERE a.name = t.albumartist AND a.sort IS NOT NULL LIMIT 1) AS sort,
    COUNT(DISTINCT json_array(t.album, t.year)) AS num_albums
FROM tracks t
WHERE t.albumartist IS NOT NULL
GROUP BY t.albumartist;

DROP VIEW [All Artists];
CREATE VIEW [All Artists] AS
SELECT
    a.name AS name,
    MAX(a.sort) AS sort,
    COUNT(DISTINCT json_array(t.album, t.albumartist, t.year)) AS num_albums
FROM artists a JOIN tracks t ON a.path = t.path
GROUP BY a.name;
