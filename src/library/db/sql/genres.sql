-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-FileCopyrightText: 2023  Emmett de St. Croix
-- SPDX-License-Identifier: GPL-3.0-or-later

CREATE TABLE IF NOT EXISTS genres(
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (name, path) ON CONFLICT REPLACE,
    FOREIGN KEY (path) REFERENCES tracks(path) ON DELETE CASCADE
);
