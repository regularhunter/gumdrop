-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-License-Identifier: GPL-3.0-or-later

-- Folders that were added to the library. Rescans search them again,
-- subfolders included, to find songs that were added since
CREATE TABLE folders(
    path TEXT NOT NULL,
    PRIMARY KEY (path) ON CONFLICT REPLACE
);
