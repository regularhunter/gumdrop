// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-FileCopyrightText: 2023  Emmett de St. Croix
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use gtk::glib;
use log::{debug, info, warn};
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{
    functions::FunctionFlags, params, Connection, OptionalExtension, Params, Row, Transaction,
    TransactionBehavior,
};

use super::models::{Album, Artist, Track};

// Schema migrations. Each entry upgrades the database by one version and the
// 'user_version' pragma records how many have been applied. Only ever append
// to this list
const MIGRATIONS: &[&str] = &[include_str!("sql/schema.sql")];

const INSERT_TRACK: &str = "
INSERT INTO tracks (path, title, track, discnumber, discsubtitle, album,
    albumartist, derived_albumartist, year, duration, modified)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10)
ON CONFLICT (path) DO UPDATE SET
    title = excluded.title,
    track = excluded.track,
    discnumber = excluded.discnumber,
    discsubtitle = excluded.discsubtitle,
    album = excluded.album,
    albumartist = excluded.albumartist,
    derived_albumartist = 0,
    year = excluded.year,
    duration = excluded.duration,
    modified = excluded.modified";

// Default ordering for track lists
const TRACK_ORDER: &str = "t.albumartist COLLATE NOCASE, t.album COLLATE NOCASE, \
                           t.discnumber, t.track, t.path";

// Ordering for the tracks of a single album
const ALBUM_TRACK_ORDER: &str = "t.discnumber, t.track, t.path";

// The text that search_tracks() matches against
const SEARCH_KEY: &str = "t.title || ' ' || t.album || ' ' || IFNULL(t.albumartist, '') || ' ' \
     || IFNULL((SELECT group_concat(a.name, ' ') FROM artists a WHERE a.path = t.path), '')";

#[derive(Debug)]
pub enum Error {
    Pool(r2d2::Error),
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Pool(e) => write!(f, "database connection error: {e}"),
            Error::Sqlite(e) => write!(f, "database error: {e}"),
            Error::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<r2d2::Error> for Error {
    fn from(e: r2d2::Error) -> Self {
        Error::Pool(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// Handle to the library database
#[derive(Debug, Clone)]
pub struct LibraryDB {
    needs_rebuild: bool,
    pool: r2d2::Pool<SqliteConnectionManager>,
    // Sqlite only allows one writer at a time
    write_lock: Arc<Mutex<()>>,
}

impl LibraryDB {
    // Album artist given to albums without an album artist tag whose tracks
    // don't share a common artist. Must match update_albumartists.sql
    pub const VARIOUS_ARTISTS: &'static str = "[Various Artists]";

    // Opens (or creates) the library database in the user data directory
    pub fn new() -> Result<Self> {
        let mut data_dir = glib::user_data_dir();
        data_dir.push("gumdrop");
        std::fs::create_dir_all(&data_dir)?;

        Self::open(data_dir.join("gumdrop.db"))
    }

    // Opens (or creates) the database at 'db_path' and brings its schema up to date
    pub fn open<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db_path = db_path.as_ref();
        debug!("Opening library database at {:?}", db_path);

        let manager = SqliteConnectionManager::file(db_path).with_init(init_connection);
        // Don't need the default 10 connections; so set to 4 and close any that sit idle
        let pool = r2d2::Pool::builder()
            .max_size(4)
            .min_idle(Some(0))
            .idle_timeout(Some(Duration::from_secs(30)))
            .connection_timeout(Duration::from_secs(10))
            .build(manager)?;

        let mut conn = pool.get()?;

        // Change to WAL if not already. This setting persists in the database file
        let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            info!("Changing journal mode to WAL");
            conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))?;
        }

        let (old_version, new_version) = migrate(&mut conn)?;
        drop(conn);

        Ok(Self {
            needs_rebuild: old_version != new_version,
            pool,
            write_lock: Arc::default(),
        })
    }

    // Every track should be read again when schema is updated
    pub fn needs_rebuild(&self) -> bool {
        self.needs_rebuild
    }

    pub fn pool(&self) -> &r2d2::Pool<SqliteConnectionManager> {
        &self.pool
    }

    pub fn connection(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        Ok(self.pool.get()?)
    }

    // Runs 'f' inside a write transaction and committing if it succeeds
    fn write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let res = f(&tx)?;
        tx.commit()?;
        Ok(res)
    }

    // LIBRARY MODIFICATION

    // Adds tracks to the library and replaces any existing entries with the
    // same path. Returns the number of tracks written
    pub fn insert_tracks(&self, tracks: &[Track]) -> Result<usize> {
        if tracks.is_empty() {
            return Ok(0);
        }

        self.write(|tx| {
            let mut insert_track = tx.prepare_cached(INSERT_TRACK)?;
            let mut delete_artists = tx.prepare_cached("DELETE FROM artists WHERE path = ?1")?;
            let mut insert_artist =
                tx.prepare_cached("INSERT INTO artists (name, sort, path) VALUES (?1, ?2, ?3)")?;
            let mut delete_genres = tx.prepare_cached("DELETE FROM genres WHERE path = ?1")?;
            let mut insert_genre =
                tx.prepare_cached("INSERT INTO genres (name, path) VALUES (?1, ?2)")?;

            let mut inserted = 0;
            for track in tracks {
                let path = match path_to_str(&track.path) {
                    Some(p) => p,
                    None => continue,
                };

                insert_track.execute(params![
                    path,
                    track.title,
                    track.track,
                    track.disc,
                    track.disc_subtitle,
                    track.album,
                    track.album_artist,
                    track.year,
                    track.duration,
                    track.modified,
                ])?;

                delete_artists.execute([path])?;
                for (i, name) in track.artists.iter().enumerate() {
                    let sort = if i == 0 {
                        track.artist_sort.as_deref()
                    } else {
                        None
                    };
                    insert_artist.execute(params![name, sort, path])?;
                }

                delete_genres.execute([path])?;
                for name in &track.genres {
                    insert_genre.execute(params![name, path])?;
                }

                inserted += 1;
            }

            update_album_artists(tx)?;
            Ok(inserted)
        })
    }

    // Removes tracks from the library and returns the number of tracks removed
    pub fn remove_tracks(&self, paths: &[PathBuf]) -> Result<usize> {
        self.write(|tx| {
            let mut stmt = tx.prepare_cached("DELETE FROM tracks WHERE path = ?1")?;
            let mut removed = 0;
            for path in paths.iter().filter_map(|p| path_to_str(p)) {
                removed += stmt.execute([path])?;
            }
            update_album_artists(tx)?;
            Ok(removed)
        })
    }

    // Removes all tracks and covers inside 'folder' from library and returns the number of tracks removed
    pub fn remove_folder(&self, folder: &Path) -> Result<usize> {
        let folder = match path_to_str(folder) {
            Some(f) => f,
            None => return Ok(0),
        };

        self.write(|tx| {
            let removed = tx.execute("DELETE FROM tracks WHERE is_subdir(path, ?1)", [folder])?;
            tx.execute("DELETE FROM covers WHERE is_subdir(path, ?1)", [folder])?;
            update_album_artists(tx)?;
            Ok(removed)
        })
    }

    // Removes tracks and covers whose files no longer exist and returns the number of tracks removed
    pub fn remove_missing(&self) -> Result<usize> {
        self.write(|tx| {
            let removed = tx.execute("DELETE FROM tracks WHERE NOT path_exists(path)", [])?;
            tx.execute("DELETE FROM covers WHERE NOT path_exists(path)", [])?;
            if removed > 0 {
                update_album_artists(tx)?;
            }
            Ok(removed)
        })
    }

    // Removes every track, artist, genre and cover from the library and returns
    // the number of tracks removed. The audio files themselves are not touched
    pub fn clear(&self) -> Result<usize> {
        self.write(|tx| {
            tx.execute("DELETE FROM artists", [])?;
            tx.execute("DELETE FROM genres", [])?;
            tx.execute("DELETE FROM covers", [])?;
            let removed = tx.execute("DELETE FROM tracks", [])?;
            Ok(removed)
        })
    }

    // Records cover image files
    pub fn add_covers(&self, paths: &[PathBuf]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }

        self.write(|tx| {
            let mut stmt = tx.prepare_cached("INSERT INTO covers (path) VALUES (?1)")?;
            for path in paths.iter().filter_map(|p| path_to_str(p)) {
                stmt.execute([path])?;
            }
            Ok(())
        })
    }

    // LIBRARY SYNCHRONIZATION

    // Whether 'path' is in the library and was indexed when the file had the given modification time
    pub fn is_indexed(&self, path: &Path, modified: f64) -> Result<bool> {
        let path = match path_to_str(path) {
            Some(p) => p,
            None => return Ok(false),
        };

        let conn = self.connection()?;
        let indexed: Option<f64> = conn
            .query_row("SELECT modified FROM tracks WHERE path = ?1", [path], |r| {
                r.get(0)
            })
            .optional()?;

        Ok(indexed == Some(modified))
    }

    // Paths of all tracks in the library
    pub fn track_paths(&self) -> Result<Vec<PathBuf>> {
        self.query_paths("SELECT path FROM tracks")
    }

    // Paths of tracks whose files were modified (or removed) since they were indexed
    pub fn outdated_tracks(&self) -> Result<Vec<PathBuf>> {
        self.query_paths("SELECT path FROM tracks WHERE was_modified(path, modified)")
    }

    fn query_paths(&self, sql: &str) -> Result<Vec<PathBuf>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare_cached(sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0).map(PathBuf::from))?;
        let paths = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(paths)
    }

    // TRACKS

    pub fn track_count(&self) -> Result<u64> {
        let conn = self.connection()?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?;
        Ok(count as u64)
    }

    // All tracks, ordered by album artist, album, disc and track number
    pub fn tracks(&self) -> Result<Vec<Track>> {
        self.query_tracks("", TRACK_ORDER, [])
    }

    pub fn track(&self, path: &Path) -> Result<Option<Track>> {
        let path = match path_to_str(path) {
            Some(p) => p,
            None => return Ok(None),
        };

        let tracks = self.query_tracks("WHERE t.path = ?1", "t.path", [path])?;
        Ok(tracks.into_iter().next())
    }

    // The tracks of an album, in disc and track order
    pub fn album_tracks(&self, album: &str, album_artist: Option<&str>) -> Result<Vec<Track>> {
        self.query_tracks(
            "WHERE t.album = ?1 AND t.albumartist IS ?2",
            ALBUM_TRACK_ORDER,
            params![album, album_artist],
        )
    }

    // Tracks by an artist, either as the album artist or a track artist
    pub fn artist_tracks(&self, artist: &str) -> Result<Vec<Track>> {
        self.query_tracks(
            "WHERE t.albumartist = ?1 OR EXISTS \
             (SELECT 1 FROM artists a WHERE a.path = t.path AND a.name = ?1)",
            TRACK_ORDER,
            [artist],
        )
    }

    pub fn genre_tracks(&self, genre: &str) -> Result<Vec<Track>> {
        self.query_tracks(
            "WHERE EXISTS (SELECT 1 FROM genres g WHERE g.path = t.path AND g.name = ?1)",
            TRACK_ORDER,
            [genre],
        )
    }

    // Fuzzy search over track titles, albums and artists, best matches first
    pub fn search_tracks(&self, query: &str) -> Result<Vec<Track>> {
        let query = query.trim();
        if query.is_empty() {
            return self.tracks();
        }

        let filter = format!("WHERE string_diff({}, ?1) > 0", SEARCH_KEY);
        let order = format!("string_diff({}, ?1) DESC, {}", SEARCH_KEY, TRACK_ORDER);
        self.query_tracks(&filter, &order, [query])
    }

    // 'filter' and 'order' must be constant sql; values go in 'params'
    fn query_tracks<P: Params>(&self, filter: &str, order: &str, params: P) -> Result<Vec<Track>> {
        let sql = format!(include_str!("sql/track_query.sql"), filter, order);
        let conn = self.connection()?;
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params, row_to_track)?;
        let tracks = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tracks)
    }

    // ALBUMS

    // All albums, ordered by album artist and title
    pub fn albums(&self) -> Result<Vec<Album>> {
        self.query_albums("", [])
    }

    // Albums by an artist, including albums where they only appear on some of the tracks
    pub fn artist_albums(&self, artist: &str) -> Result<Vec<Album>> {
        self.query_albums(
            "WHERE t.albumartist = ?1 OR EXISTS \
             (SELECT 1 FROM tracks t3 JOIN artists a ON a.path = t3.path \
              WHERE t3.album = t.album AND t3.albumartist IS t.albumartist AND a.name = ?1)",
            [artist],
        )
    }

    // Albums that have no album artist, either from tags or derived from the track artists
    pub fn albums_without_album_artist(&self) -> Result<Vec<Album>> {
        self.query_albums("WHERE t.albumartist IS NULL", [])
    }

    pub fn genre_albums(&self, genre: &str) -> Result<Vec<Album>> {
        self.query_albums(
            "WHERE EXISTS \
             (SELECT 1 FROM tracks t3 JOIN genres g ON g.path = t3.path \
              WHERE t3.album = t.album AND t3.albumartist IS t.albumartist AND g.name = ?1)",
            [genre],
        )
    }

    // 'filter' must be constant SQL; values go in 'params'
    fn query_albums<P: Params>(&self, filter: &str, params: P) -> Result<Vec<Album>> {
        let sql = format!(include_str!("sql/album_query.sql"), filter);
        let conn = self.connection()?;
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params, row_to_album)?;
        let albums = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(albums)
    }

    // ARTISTS AND GENRES

    // Artists credited as an album artist, ordered by sort name
    pub fn album_artists(&self) -> Result<Vec<Artist>> {
        self.query_artists(
            "SELECT name, sort, num_albums FROM [Album Artists] \
             ORDER BY IFNULL(sort, name) COLLATE NOCASE",
        )
    }

    // All artists credited on any track, ordered by sort name
    pub fn artists(&self) -> Result<Vec<Artist>> {
        self.query_artists(
            "SELECT name, sort, num_albums FROM [All Artists] \
             ORDER BY IFNULL(sort, name) COLLATE NOCASE",
        )
    }

    fn query_artists(&self, sql: &str) -> Result<Vec<Artist>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare_cached(sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(Artist {
                name: r.get(0)?,
                sort: r.get(1)?,
                num_albums: r.get(2)?,
            })
        })?;
        let artists = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(artists)
    }

    pub fn genres(&self) -> Result<Vec<String>> {
        let conn = self.connection()?;
        let mut stmt =
            conn.prepare_cached("SELECT DISTINCT name FROM genres ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        let genres = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(genres)
    }
}

// Applies any pending migrations. Returns the schema version before and after
fn migrate(conn: &mut Connection) -> Result<(usize, usize)> {
    let current = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;

    if current > MIGRATIONS.len() {
        warn!(
            "Library database schema version {} is newer than this version of Gumdrop supports ({})",
            current,
            MIGRATIONS.len()
        );
        return Ok((current, current));
    }

    for (version, migration) in MIGRATIONS.iter().enumerate().skip(current) {
        info!(
            "Migrating library database to schema version {}",
            version + 1
        );
        let tx = conn.transaction()?;
        tx.execute_batch(migration)?;
        tx.pragma_update(None, "user_version", (version + 1) as i64)?;
        tx.commit()?;
    }

    Ok((current, MIGRATIONS.len()))
}

fn update_album_artists(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(include_str!("sql/update_albumartists.sql"))
}

// Paths are stored as text so paths that aren't valid UTF-8 can't be stored
fn path_to_str(path: &Path) -> Option<&str> {
    let s = path.to_str();
    if s.is_none() {
        warn!("Skipping path that isn't valid UTF-8: {:?}", path);
    }
    s
}

fn json_list(json: String) -> Vec<String> {
    serde_json::from_str(&json).unwrap_or_default()
}

// Reads a row produced by track_query.sql
fn row_to_track(row: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        path: PathBuf::from(row.get::<_, String>(0)?),
        title: row.get(1)?,
        track: row.get(2)?,
        disc: row.get(3)?,
        disc_subtitle: row.get(4)?,
        album: row.get(5)?,
        album_artist: row.get(6)?,
        year: row.get(7)?,
        duration: row.get(8)?,
        modified: row.get(9)?,
        artists: json_list(row.get(10)?),
        artist_sort: None,
        genres: json_list(row.get(11)?),
    })
}

// Reads a row produced by album_query.sql
fn row_to_album(row: &Row<'_>) -> rusqlite::Result<Album> {
    Ok(Album {
        title: row.get(0)?,
        album_artist: row.get(1)?,
        duration: row.get(2)?,
        year: row.get(3)?,
        cover: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
        num_tracks: row.get(5)?,
        artists: json_list(row.get(6)?),
        genres: json_list(row.get(7)?),
        sample_track: PathBuf::from(row.get::<_, String>(8)?),
    })
}

// Sets up each new connection in the pool
fn init_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    // Foreign key enforcement (and ON DELETE CASCADE) is off by default
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    attach_functions(connection)
}

fn attach_functions(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.create_scalar_function(
        "is_subdir",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            assert_eq!(ctx.len(), 2, "incorrect number of arguments");
            let path1 = Path::new(ctx.get_raw(0).as_str()?);
            let path2 = Path::new(ctx.get_raw(1).as_str()?);
            Ok(path1.starts_with(path2))
        },
    )?;

    connection.create_scalar_function(
        "same_dir",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            assert_eq!(ctx.len(), 2, "incorrect number of arguments");
            let path1 = Path::new(ctx.get_raw(0).as_str()?);
            let path2 = Path::new(ctx.get_raw(1).as_str()?);
            Ok(path1.parent() == path2.parent())
        },
    )?;

    // Whether the file at the path has a different modification time than
    // the one given or no longer exists. Must compute the time the same way
    // as scanner::modified_time()
    connection.create_scalar_function(
        "was_modified",
        2,
        FunctionFlags::SQLITE_UTF8,
        move |ctx| {
            assert_eq!(ctx.len(), 2, "incorrect number of arguments");
            let path = Path::new(ctx.get_raw(0).as_str()?);
            let last_modified = ctx.get_raw(1).as_f64()?;
            if let Ok(current_modified) = path.metadata().and_then(|m| m.modified()) {
                Ok(current_modified
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or_default()
                    != last_modified)
            } else {
                Ok(true)
            }
        },
    )?;

    connection.create_scalar_function(
        "path_exists",
        1,
        FunctionFlags::SQLITE_UTF8,
        move |ctx| {
            assert_eq!(ctx.len(), 1, "incorrect number of arguments");
            let path = Path::new(ctx.get_raw(0).as_str()?);
            Ok(path.exists())
        },
    )?;

    // Fuzzy match score of the query (argument 2) against the term (argument 1); 0 if it doesn't match
    connection.create_scalar_function(
        "string_diff",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            assert_eq!(ctx.len(), 2, "incorrect number of arguments");
            let term = ctx.get_raw(0).as_str()?;
            let query = ctx.get_raw(1).as_str()?;
            let matcher = SkimMatcherV2::default().ignore_case();
            let score = matcher.fuzzy_match(term, query).unwrap_or(0);
            Ok(score)
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDb {
        db: LibraryDB,
        dir: PathBuf,
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn temp_db(name: &str) -> TempDb {
        let dir =
            std::env::temp_dir().join(format!("gumdrop-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = LibraryDB::open(dir.join("test.db")).unwrap();
        TempDb { db, dir }
    }

    fn track(path: &str, album: &str, album_artist: Option<&str>, artists: &[&str]) -> Track {
        Track {
            path: PathBuf::from(path),
            title: path.to_string(),
            track: Some(1),
            album: album.to_string(),
            album_artist: album_artist.map(String::from),
            artists: artists.iter().map(|s| s.to_string()).collect(),
            genres: vec!["Rock".to_string()],
            duration: 60.0,
            modified: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn migrates_once() {
        let t = temp_db("migrate");
        assert!(t.db.needs_rebuild());
        let reopened = LibraryDB::open(t.dir.join("test.db")).unwrap();
        assert!(!reopened.needs_rebuild());
    }

    #[test]
    fn insert_and_query() {
        let t = temp_db("insert");
        let db = &t.db;
        db.insert_tracks(&[
            track("/m/a/1.flac", "Album", None, &["X", "Y"]),
            track("/m/a/2.flac", "Album", None, &["X"]),
            track("/m/b/1.flac", "Comp", None, &["P"]),
            track("/m/b/2.flac", "Comp", None, &["Q"]),
            track("/m/c/1.flac", "Tagged", Some("AA"), &["Z"]),
        ])
        .unwrap();

        let albums = db.albums().unwrap();
        assert_eq!(albums.len(), 3);
        let album = albums.iter().find(|a| a.title == "Album").unwrap();
        assert_eq!(album.album_artist.as_deref(), Some("X"));
        assert_eq!(album.num_tracks, 2);
        assert_eq!(album.artists, vec!["X", "Y"]);
        assert_eq!(album.genres, vec!["Rock"]);

        let comp = albums.iter().find(|a| a.title == "Comp").unwrap();
        assert_eq!(
            comp.album_artist.as_deref(),
            Some(LibraryDB::VARIOUS_ARTISTS)
        );

        let tracks = db.album_tracks("Album", Some("X")).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].artists, vec!["X", "Y"]);

        assert_eq!(db.artist_albums("Y").unwrap().len(), 1);
        assert_eq!(db.artist_tracks("Z").unwrap().len(), 1);
        assert_eq!(db.artists().unwrap().len(), 5);
        assert_eq!(db.album_artists().unwrap().len(), 3);
        assert_eq!(db.genres().unwrap(), vec!["Rock"]);
        assert_eq!(db.search_tracks("comp").unwrap().len(), 2);
        assert_eq!(db.track_count().unwrap(), 5);
        assert!(db.albums_without_album_artist().unwrap().is_empty());
        assert_eq!(album.sample_track, PathBuf::from("/m/a/1.flac"));

        assert!(db.is_indexed(Path::new("/m/a/1.flac"), 1.0).unwrap());
        assert!(!db.is_indexed(Path::new("/m/a/1.flac"), 2.0).unwrap());

        // Reinsertion replaces old artists and the derived album artist follows
        db.insert_tracks(&[track("/m/a/1.flac", "Album", None, &["W"])])
            .unwrap();
        let album = db
            .albums()
            .unwrap()
            .into_iter()
            .find(|a| a.title == "Album")
            .unwrap();
        assert_eq!(
            album.album_artist.as_deref(),
            Some(LibraryDB::VARIOUS_ARTISTS)
        );
        assert_eq!(album.artists, vec!["W", "X"]);

        // Removing tracks cascades to artists and genres
        assert_eq!(db.remove_folder(Path::new("/m/a")).unwrap(), 2);
        assert_eq!(db.track_paths().unwrap().len(), 3);
        assert!(db.artist_tracks("W").unwrap().is_empty());

        // None of these files exist
        assert_eq!(db.remove_missing().unwrap(), 3);
        assert!(db.tracks().unwrap().is_empty());
    }
}
