// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-FileCopyrightText: 2023  Emmett de St. Croix
// SPDX-License-Identifier: GPL-3.0-or-later

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};
use std::{path::PathBuf, time::SystemTime};

#[derive(Debug, Clone)]
pub struct LibraryDB {
    pub needs_rebuild: bool,
    pool: r2d2::Pool<SqliteConnectionManager>,
}

impl LibraryDB {
    pub fn new() -> Self {
        let db_path = crate::paths::DATA.join("gumdrop.db");

        let manager = SqliteConnectionManager::file(&db_path).with_init(attach_functions);
        // Don't need the default 10 connections, but setting it to 1 seems to deadlock
        // the application so use 4 and close any that sit idle for more than a minute.
        let pool = r2d2::Pool::builder()
            .max_size(4)
            .min_idle(Some(0))
            .idle_timeout(Some(std::time::Duration::from_secs(30)))
            .build(manager)
            .unwrap();

        let migrations = Migrations::new(vec![
            M::up(include_str!("sql/schema.sql")),
            M::up(include_str!("sql/covers.sql")),
            M::up(include_str!("sql/genres.sql")),
            M::up("ALTER TABLE tracks ADD COLUMN lyrics_changed BOOL"),
            M::up(include_str!("sql/album_indec.sql")),
        ]);

        // change to WAL if not already
        let mut conn = pool.get().expect("Failed to get connection from pool");

        if !matches!(
            conn.query_row("PRAGMA journal_mode", [], |r| Ok(r.get::<_, String>(0)?))
                .as_deref(),
            Ok("wal")
        ) {
            log::info!("Changing journal mode to WAL");
            conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        }

        let old_version = migrations.current_version(&conn).unwrap();
        migrations.to_latest(&mut conn).unwrap();
        let new_version = migrations.current_version(&conn).unwrap();

        Self {
            needs_rebuild: old_version != new_version,
            pool,
        }
    }

    pub fn pool(&self) -> &r2d2::Pool<SqliteConnectionManager> {
        &self.pool
    }

    pub fn connection(&self) -> PooledConnection<SqliteConnectionManager> {
        self.pool.get().expect("Failed to get connection from pool")
    }
}

fn attach_functions(connection: &mut Connection) -> Result<(), rusqlite::Error> {
    connection.create_scalar_function(
        "is_subdir",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            assert_eq!(ctx.len(), 2, "incorrect number of arguments");
            let path1 = ctx.get_raw(0).as_str()?;
            let path2 = ctx.get_raw(1).as_str()?;
            Ok(path1.starts_with(path2))
        },
    )?;

    connection
        .create_scalar_function(
            "same_dir",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            move |ctx| {
                assert_eq!(ctx.len(), 2, "incorrect number of arguments");
                let path1 = PathBuf::from(ctx.get_raw(0).as_str()?);
                let path2 = PathBuf::from(ctx.get_raw(1).as_str()?);
                Ok(path1.parent() == path2.parent())
            },
        )
        .unwrap();

    connection.create_scalar_function(
        "was_modified",
        2,
        FunctionFlags::SQLITE_UTF8,
        move |ctx| {
            assert_eq!(ctx.len(), 2, "incorrect number of arguments");
            let path = PathBuf::from(ctx.get_raw(0).as_str()?);
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
            let path = PathBuf::from(ctx.get_raw(0).as_str()?);
            Ok(path.exists())
        },
    )?;

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
