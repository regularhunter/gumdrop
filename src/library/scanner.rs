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

pub fn is_playlist_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
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
    let mut written = 0;
    let res = index_batches(db, files, force, &mut written);

    // Album artists are derived once per scan
    let updated = if written > 0 {
        db.update_album_artists()
    } else {
        Ok(())
    };
    res?;
    updated?;

    debug!(
        "Indexed {} tracks in {} ms",
        written,
        now.elapsed().as_millis()
    );

    Ok(written)
}

fn index_batches<I>(db: &LibraryDB, files: I, force: bool, written: &mut usize) -> Result<()>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut dirs = HashSet::new();

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
            *written += db.insert_tracks_deferred(&batch)?;
            batch.clear();
        }
    }

    *written += db.insert_tracks_deferred(&batch)?;

    let covers: Vec<PathBuf> = dirs.iter().flat_map(|d| find_covers(d)).collect();
    db.add_covers(&covers)
}

pub fn audio_files_in_folder(folder: &Path) -> Vec<PathBuf> {
    audio_files(folder, true)
}

fn audio_files(folder: &Path, recursive: bool) -> Vec<PathBuf> {
    files_in_folder(folder, recursive, is_audio_file)
}

fn playlist_files(folder: &Path, recursive: bool) -> Vec<PathBuf> {
    files_in_folder(folder, recursive, is_playlist_file)
}

fn files_in_folder(folder: &Path, recursive: bool, wanted: fn(&Path) -> bool) -> Vec<PathBuf> {
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
                if recursive {
                    pending.push(path);
                }
            } else if (file_type.is_file() || (file_type.is_symlink() && path.is_file()))
                && wanted(&path)
            {
                files.push(path);
            }
        }
    }

    files
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ScanSummary {
    pub songs: usize,
    pub playlists: usize,
}

pub fn scan_folder(db: &LibraryDB, folder: &Path) -> Result<ScanSummary> {
    debug!("Scanning folder {:?}", folder);
    db.add_folders(&[folder.to_path_buf()])?;
    db.remove_missing()?;
    let songs = index_files(db, audio_files_in_folder(folder))?;
    let (playlists, listed_songs) = index_playlists(db, playlist_files(folder, true), false)?;
    Ok(ScanSummary {
        songs: songs + listed_songs,
        playlists,
    })
}

fn index_playlists<I>(db: &LibraryDB, files: I, force: bool) -> Result<(usize, usize)>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut playlists = 0;
    let mut songs = 0;

    for path in files {
        let modified = match modified_time(&path) {
            Some(m) => m,
            None => continue,
        };
        if !force && db.is_playlist_indexed(&path, modified)? {
            continue;
        }

        let (name, entries) = match read_playlist(&path) {
            Some(playlist) => playlist,
            None => continue,
        };
        let songs_on_disk: Vec<PathBuf> = entries
            .iter()
            .filter(|p| is_audio_file(p) && p.is_file())
            .cloned()
            .collect();
        songs += index_files(db, songs_on_disk)?;

        db.set_playlist(&path, &name, modified, &entries)?;
        playlists += 1;
    }

    Ok((playlists, songs))
}

pub fn read_playlist(path: &Path) -> Option<(String, Vec<PathBuf>)> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            warn!("Unable to read playlist {:?}: {}", path, e);
            return None;
        }
    };
    // .m3u files are often not UTF-8; keep what can be read
    let text = String::from_utf8_lossy(&bytes);
    let folder = path.parent().unwrap_or_else(|| Path::new(""));

    let mut name = None;
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        if let Some(title) = line.strip_prefix("#PLAYLIST:") {
            name = non_empty(Some(title));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        if let Some(entry) = playlist_entry_path(line) {
            let entry = if entry.is_absolute() {
                entry
            } else {
                folder.join(entry)
            };
            // Rebuilding the path from its components evens out separators
            // and drops "." segments, so that it matches the library's paths
            entries.push(entry.components().collect());
        }
    }

    let name = name.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    Some((name, entries))
}

fn playlist_entry_path(entry: &str) -> Option<PathBuf> {
    if let Some(uri) = entry.strip_prefix("file://") {
        let uri = uri.strip_prefix("localhost").unwrap_or(uri);
        return Some(PathBuf::from(percent_decode(uri)));
    }
    if entry.contains("://") {
        return None;
    }

    if cfg!(unix) {
        Some(PathBuf::from(entry.replace('\\', "/")))
    } else {
        Some(PathBuf::from(entry))
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn library_folders(db: &LibraryDB) -> Result<Vec<PathBuf>> {
    let mut folders: Vec<PathBuf> = db.folders()?.into_iter().filter(|f| f.is_dir()).collect();
    // Shorter paths first, so that parents come before their subfolders
    folders.sort_by_key(|f| f.components().count());

    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if !roots.iter().any(|root| folder.starts_with(root)) {
            roots.push(folder);
        }
    }
    Ok(roots)
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct RescanSummary {
    pub removed: usize,
    pub added: usize,
}

pub fn rescan(db: &LibraryDB) -> Result<RescanSummary> {
    let removed = db.remove_missing()?;

    let known: HashSet<PathBuf> = db.track_paths()?.into_iter().collect();
    index(db, known.iter().cloned(), true)?;

    let mut new_files: HashSet<PathBuf> = HashSet::new();
    for folder in library_folders(db)? {
        new_files.extend(audio_files(&folder, true));
    }
    let song_folders: HashSet<&Path> = known.iter().filter_map(|p| p.parent()).collect();
    for folder in song_folders {
        new_files.extend(audio_files(folder, false));
    }
    new_files.retain(|path| !known.contains(path));
    let mut added = index_files(db, new_files)?;

    let known_playlists: HashSet<PathBuf> = db.playlist_paths()?.into_iter().collect();
    let mut new_playlists: HashSet<PathBuf> = HashSet::new();
    for folder in library_folders(db)? {
        new_playlists.extend(playlist_files(&folder, true));
    }
    let playlist_folders: HashSet<&Path> = known
        .iter()
        .chain(known_playlists.iter())
        .filter_map(|p| p.parent())
        .collect();
    for folder in playlist_folders {
        new_playlists.extend(playlist_files(folder, false));
    }
    new_playlists.retain(|path| !known_playlists.contains(path));
    added += index_playlists(db, known_playlists, true)?.1;
    added += index_playlists(db, new_playlists, false)?.1;

    debug!("Rescan removed {} and added {} tracks", removed, added);
    Ok(RescanSummary { removed, added })
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
    let (playlists, listed) = index_playlists(db, db.playlist_paths()?, force)?;

    Ok(removed + reindexed + playlists + listed)
}
