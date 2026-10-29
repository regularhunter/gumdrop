// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

/// A single audio file in the library.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub disc_subtitle: Option<String>,
    /// Empty if the file has no album tag.
    pub album: String,
    /// The album artist tag or, when that tag is missing, an artist derived
    /// from the other tracks of the album (see `LibraryDB::VARIOUS_ARTISTS`).
    pub album_artist: Option<String>,
    pub artists: Vec<String>,
    /// Sort name of the first entry in `artists`. Only used when inserting.
    pub artist_sort: Option<String>,
    pub genres: Vec<String>,
    pub year: Option<String>,
    /// Duration in seconds.
    pub duration: f64,
    /// File modification time, in seconds since the Unix epoch.
    pub modified: f64,
}

/// A group of tracks sharing an album title and album artist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Album {
    pub title: String,
    pub album_artist: Option<String>,
    pub artists: Vec<String>,
    pub genres: Vec<String>,
    pub year: Option<String>,
    /// Total duration in seconds.
    pub duration: f64,
    pub num_tracks: u32,
    /// A cover image file in the album's folder, if one was found.
    pub cover: Option<PathBuf>,
    /// One of the album's tracks, for reading embedded cover art.
    pub sample_track: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Artist {
    pub name: String,
    pub sort: Option<String>,
    pub num_albums: u32,
}
