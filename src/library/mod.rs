// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

// The persistent music library.
//
// LibraryDB stores track metadata in an SQLite database in the user data
// directory, and the scanner module reads tags from audio files and keeps the
// database in sync with the files on disk. Both are safe to use from worker
// threads; LibraryDB is a cheap handle to a shared connection pool.

// Parts of the API (genres, folder scanning, removal) aren't used by the UI yet.
#![allow(dead_code)]

pub mod db;
pub mod scanner;

#[allow(unused_imports)]
pub use db::{Album, Artist, LibraryDB, Track};
