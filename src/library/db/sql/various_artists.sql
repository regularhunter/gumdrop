-- SPDX-FileCopyrightText: 2025  Hunter Wardlaw
-- SPDX-License-Identifier: GPL-3.0-or-later

-- Album artist tags that say "Various Artists" mean the same as the
-- compilations found by update_albumartists.sql, so store them the same way
-- and show them as one artist. The '[Various Artists]' literal must match
-- VARIOUS_ARTISTS in librarydb.rs
UPDATE tracks SET albumartist = '[Various Artists]'
WHERE albumartist = 'Various Artists' COLLATE NOCASE;
