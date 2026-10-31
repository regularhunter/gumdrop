// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

// Reads metadata from audio files into the library database.
//
// These functions do blocking file I/O, so call them from a worker thread
// rather than the main loop.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

use lofty::{file::FileType, prelude::*, tag::Tag};
use log::{debug, warn};

use super::db::{LibraryDB, Result, Track};

const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const COVER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

const BATCH_SIZE: usize = 500;

pub fn modified_time(path: &Path) -> Option<f64> {
    let modified = path.metadata().and_then(|m| m.modified()).ok()?;
    Some(
        modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or_default(),
    )
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension().and_then(FileType::from_ext).is_some()
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn tag_strings(tag: &Tag, key: ItemKey) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    for value in tag.get_strings(key).filter_map(|s| non_empty(Some(s))) {
        if !values.contains(&value) {
            values.push(value);
        }
    }
    values
}

pub fn read_track(path: &Path) -> Option<Track> {
    let modified = modified_time(path)?;

    let tagged_file = match lofty::read_from_path(path) {
        Ok(f) => f,
        Err(e) => {
            warn!("Unable to read tags from {:?}: {}", path, e);
            return None;
        }
    };

    let duration = AudioFile::properties(&tagged_file).duration().as_secs_f64();

    let mut track = Track {
        path: path.to_path_buf(),
        duration,
        modified,
        ..Default::default()
    };

    if let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
    {
        track.title = non_empty(tag.title().as_deref()).unwrap_or_default();
        track.album = non_empty(tag.album().as_deref()).unwrap_or_default();
        track.album_artist = non_empty(tag.get_string(ItemKey::AlbumArtist));
        track.track = tag.track();
        track.disc = tag.disk();
        track.disc_subtitle = non_empty(tag.get_string(ItemKey::SetSubtitle));
        track.year = tag.date().map(|d| d.year.to_string());
        track.genres = tag_strings(tag, ItemKey::Genre);

        // Prefer the multi-valued ARTISTS tag when present, since ARTIST is
        // often a single display string like "A feat. B".
        track.artists = tag_strings(tag, ItemKey::TrackArtists);
        if track.artists.is_empty() {
            track.artists = tag_strings(tag, ItemKey::TrackArtist);
        }
        track.artist_sort = non_empty(tag.get_string(ItemKey::TrackArtistSortOrder));
    } else {
        debug!("No tags found in {:?}", path);
    }

    if track.title.is_empty() {
        track.title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
    }

    Some(track)
}

fn find_covers(dir: &Path) -> Vec<PathBuf> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_lowercase);
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .map(str::to_lowercase);
            match (stem, ext) {
                (Some(stem), Some(ext)) => {
                    COVER_NAMES.contains(&stem.as_str())
                        && COVER_EXTENSIONS.contains(&ext.as_str())
                        && p.is_file()
                }
                _ => false,
            }
        })
        .collect()
}

pub fn index_files<I>(db: &LibraryDB, files: I) -> Result<usize>
where
    I: IntoIterator<Item = PathBuf>,
{
    index(db, files, false)
}

fn index<I>(db: &LibraryDB, files: I, force: bool) -> Result<usize>
where
    I: IntoIterator<Item = PathBuf>,
{
    let now = Instant::now();
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut dirs = HashSet::new();
    let mut written = 0;

    for path in files {
        let modified = match modified_time(&path) {
            Some(m) => m,
            None => continue,
        };

        if !force && db.is_indexed(&path, modified)? {
            continue;
        }

        if let Some(track) = read_track(&path) {
            if let Some(dir) = path.parent() {
                dirs.insert(dir.to_path_buf());
            }
            batch.push(track);
        }

        if batch.len() >= BATCH_SIZE {
            written += db.insert_tracks(&batch)?;
            batch.clear();
        }
    }

    written += db.insert_tracks(&batch)?;

    let covers: Vec<PathBuf> = dirs.iter().flat_map(|d| find_covers(d)).collect();
    db.add_covers(&covers)?;

    debug!(
        "Indexed {} tracks in {} ms",
        written,
        now.elapsed().as_millis()
    );

    Ok(written)
}

pub fn audio_files_in_folder(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![folder.to_path_buf()];

    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                warn!("Unable to read folder {:?}: {}", dir, e);
                continue;
            }
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };

            if file_type.is_dir() {
                pending.push(path);
            } else if (file_type.is_file() || (file_type.is_symlink() && path.is_file()))
                && is_audio_file(&path)
            {
                files.push(path);
            }
        }
    }

    files
}

pub fn scan_folder(db: &LibraryDB, folder: &Path) -> Result<usize> {
    debug!("Scanning folder {:?}", folder);
    db.remove_missing()?;
    index_files(db, audio_files_in_folder(folder))
}

pub fn refresh(db: &LibraryDB, force: bool) -> Result<usize> {
    let removed = db.remove_missing()?;
    if removed > 0 {
        debug!("Removed {} missing tracks from the library", removed);
    }

    let paths = if force || db.needs_rebuild() {
        db.track_paths()?
    } else {
        db.outdated_tracks()?
    };
    let reindexed = index(db, paths, true)?;

    Ok(removed + reindexed)
}
