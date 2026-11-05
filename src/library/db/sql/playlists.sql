-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-License-Identifier: GPL-3.0-or-later

CREATE TABLE playlists(
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    modified REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (path)
);

CREATE TABLE playlist_entries(
    playlist TEXT NOT NULL,
    position INTEGER NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (playlist, position),
    FOREIGN KEY (playlist) REFERENCES playlists(path) ON DELETE CASCADE
);
CREATE INDEX playlist_entry_path_index ON playlist_entries(path);
