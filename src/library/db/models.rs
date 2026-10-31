// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub disc_subtitle: Option<String>,
    pub album: String,
    pub album_artist: Option<String>,
    pub artists: Vec<String>,
    pub artist_sort: Option<String>,
    pub genres: Vec<String>,
    pub year: Option<String>,
    pub duration: f64,
    pub modified: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Album {
    pub title: String,
    pub album_artist: Option<String>,
    pub artists: Vec<String>,
    pub genres: Vec<String>,
    pub year: Option<String>,
    pub duration: f64,
    pub num_tracks: u32,
    pub cover: Option<PathBuf>,
    pub sample_track: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Artist {
    pub name: String,
    pub sort: Option<String>,
    pub num_albums: u32,
}
