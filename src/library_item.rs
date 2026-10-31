// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use glib::{ParamSpec, ParamSpecEnum, ParamSpecObject, ParamSpecString, Value};
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use once_cell::sync::Lazy;

use crate::{
    i18n::{i18n, ni18n_f},
    library::{Album, LibraryDB, Track},
    utils,
};

/// An entry in the artists list. None stands for tracks without an artist.
pub type ArtistKey = Option<String>;

#[derive(Clone, Copy, Debug, glib::Enum, PartialEq, Default)]
#[enum_type(name = "GumdropLibraryItemKind")]
pub enum LibraryItemKind {
    #[default]
    Artist,
    Album,
    Track,
}

impl AsRef<str> for LibraryItemKind {
    fn as_ref(&self) -> &str {
        match self {
            LibraryItemKind::Artist => "artist",
            LibraryItemKind::Album => "album",
            LibraryItemKind::Track => "track",
        }
    }
}

/// What a library item stands for.
#[derive(Debug, Clone)]
pub enum LibraryItemData {
    Artist(ArtistKey),
    Album(Album),
    Track(Track),
}

impl Default for LibraryItemData {
    fn default() -> Self {
        LibraryItemData::Artist(None)
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct LibraryItem {
        pub data: RefCell<LibraryItemData>,
        pub title: RefCell<String>,
        pub subtitle: RefCell<String>,
        pub number: RefCell<Option<String>>,
        pub duration: RefCell<Option<String>>,
        pub cover: RefCell<Option<gdk::Texture>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibraryItem {
        const NAME: &'static str = "GumdropLibraryItem";
        type Type = super::LibraryItem;
    }

    impl ObjectImpl for LibraryItem {
        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecEnum::builder::<LibraryItemKind>("kind")
                        .read_only()
                        .build(),
                    ParamSpecString::builder("title").read_only().build(),
                    ParamSpecString::builder("subtitle").read_only().build(),
                    ParamSpecString::builder("number").read_only().build(),
                    ParamSpecString::builder("duration").read_only().build(),
                    ParamSpecObject::builder::<gdk::Texture>("cover")
                        .read_only()
                        .build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            let obj = self.obj();
            match pspec.name() {
                "kind" => obj.kind().to_value(),
                "title" => self.title.borrow().to_value(),
                "subtitle" => self.subtitle.borrow().to_value(),
                "number" => self.number.borrow().to_value(),
                "duration" => self.duration.borrow().to_value(),
                "cover" => self.cover.borrow().to_value(),
                _ => unimplemented!(),
            }
        }
    }
}

glib::wrapper! {
    pub struct LibraryItem(ObjectSubclass<imp::LibraryItem>);
}

impl LibraryItem {
    fn new(data: LibraryItemData, title: String, subtitle: String) -> Self {
        let item: Self = glib::Object::new();
        let imp = item.imp();
        imp.data.replace(data);
        imp.title.replace(title);
        imp.subtitle.replace(subtitle);
        item
    }

    pub fn artist(artist: ArtistKey, num_albums: u32) -> Self {
        let title = artist_display_name(artist.as_deref());
        LibraryItem::new(
            LibraryItemData::Artist(artist),
            title,
            albums_label(num_albums),
        )
    }

    /// `browsed_artist` is the artist whose albums are listed, if any; the
    /// album artist is only mentioned when it differs.
    pub fn album(album: Album, cover: Option<gdk::Texture>, browsed_artist: Option<&str>) -> Self {
        let title = album_display_title(&album.title);
        let subtitle = album_details(&album, browsed_artist);
        let item = LibraryItem::new(LibraryItemData::Album(album), title, subtitle);
        item.imp().cover.replace(cover);
        item
    }

    /// A song in the list of an album's songs.
    pub fn album_track(track: Track, album_artist: Option<&str>, multi_disc: bool) -> Self {
        // Only credit artists when they differ from the album artist
        let subtitle =
            if track.artists.len() == 1 && Some(track.artists[0].as_str()) == album_artist {
                String::new()
            } else {
                track.artists.join(", ")
            };

        let number = match (track.disc, track.track) {
            (Some(disc), Some(n)) if multi_disc => format!("{}.{}", disc, n),
            (_, Some(n)) => n.to_string(),
            _ => String::new(),
        };

        let title = track.title.clone();
        let duration = duration_text(track.duration);
        let item = LibraryItem::new(LibraryItemData::Track(track), title, subtitle);
        item.imp().number.replace(Some(number));
        item.imp().duration.replace(Some(duration));
        item
    }

    /// A song found by searching, outside of its album.
    pub fn track(track: Track) -> Self {
        let artist = if track.artists.is_empty() {
            artist_display_name(track.album_artist.as_deref())
        } else {
            track.artists.join(", ")
        };
        let subtitle = format!("{} · {}", artist, album_display_title(&track.album));

        let title = track.title.clone();
        let duration = duration_text(track.duration);
        let item = LibraryItem::new(LibraryItemData::Track(track), title, subtitle);
        item.imp().duration.replace(Some(duration));
        item
    }

    pub fn kind(&self) -> LibraryItemKind {
        match *self.imp().data.borrow() {
            LibraryItemData::Artist(_) => LibraryItemKind::Artist,
            LibraryItemData::Album(_) => LibraryItemKind::Album,
            LibraryItemData::Track(_) => LibraryItemKind::Track,
        }
    }

    pub fn data(&self) -> LibraryItemData {
        self.imp().data.borrow().clone()
    }
}

pub fn artist_display_name(artist: Option<&str>) -> String {
    match artist {
        None => i18n("Unknown Artist"),
        Some(name) if name == LibraryDB::VARIOUS_ARTISTS => i18n("Various Artists"),
        Some(name) => name.to_string(),
    }
}

pub fn album_display_title(title: &str) -> String {
    if title.is_empty() {
        i18n("Unknown Album")
    } else {
        title.to_string()
    }
}

/// The album artist, year and number of songs of an album. The album artist
/// is left out when it is `browsed_artist`.
pub fn album_details(album: &Album, browsed_artist: Option<&str>) -> String {
    let mut details = Vec::new();
    if browsed_artist.is_none() || album.album_artist.as_deref() != browsed_artist {
        details.push(artist_display_name(album.album_artist.as_deref()));
    }
    if let Some(year) = &album.year {
        details.push(year.clone());
    }
    details.push(songs_label(album.num_tracks));
    details.join(" · ")
}

pub fn artists_label(n: u32) -> String {
    ni18n_f(
        // Translators: the `{}` must be left unmodified;
        // it will be expanded to the number of artists
        "{} artist",
        "{} artists",
        n,
        &[&n.to_string()],
    )
}

pub fn albums_label(n: u32) -> String {
    ni18n_f(
        // Translators: the `{}` must be left unmodified;
        // it will be expanded to the number of albums
        "{} album",
        "{} albums",
        n,
        &[&n.to_string()],
    )
}

pub fn songs_label(n: u32) -> String {
    ni18n_f(
        // Translators: the `{}` must be left unmodified;
        // it will be expanded to the number of songs
        "{} song",
        "{} songs",
        n,
        &[&n.to_string()],
    )
}

fn duration_text(seconds: f64) -> String {
    utils::format_time(seconds.round() as i64)
}
