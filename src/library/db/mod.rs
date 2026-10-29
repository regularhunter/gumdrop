// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-FileCopyrightText: 2023  Emmett de St. Croix
// SPDX-License-Identifier: GPL-3.0-or-later

mod librarydb;
mod models;

#[allow(unused_imports)]
pub use librarydb::{Error, LibraryDB, Result};
#[allow(unused_imports)]
pub use models::{Album, Artist, Track};
