// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

// Browses the music library: artists, then an artist's albums, then an
// album's songs, plus a search across all three.
//
// Database queries and cover art decoding run on the gio thread pool; the
// results are applied back on the main thread. Each kind of query has a
// generation counter so that a slow, outdated result never replaces a newer
// one.

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

use adw::{prelude::*, subclass::prelude::*};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use glib::clone;
use gtk::{gdk, gio, glib, graphene, CompositeTemplate};
use lofty::{file::TaggedFileExt, picture::PictureType};
use log::warn;

use crate::{
    i18n::{i18n, ni18n_f},
    library::{db, Album, Artist, LibraryDB, Track},
    utils,
};

/// Size of album covers in lists, in logical pixels
const COVER_SIZE: i32 = 48;
/// Covers are decoded at twice their size, for HiDPI displays
const COVER_DECODE_SIZE: i32 = COVER_SIZE * 2;

const MAX_ARTIST_RESULTS: usize = 10;
const MAX_ALBUM_RESULTS: usize = 20;
const MAX_SONG_RESULTS: usize = 50;

/// What to do with songs picked in the library.
#[derive(Clone, Copy, Debug)]
pub enum QueueAction {
    /// Replace the queue with the songs, and play the one at this index
    Play(usize),
    /// Insert the songs after the current song
    PlayNext,
    /// Add the songs to the end of the queue
    Append,
}

/// Called with the paths of the songs to queue, and what to do with them.
type QueueHandler = Rc<dyn Fn(Vec<PathBuf>, QueueAction)>;

/// An entry in the artists list. None stands for tracks without an artist.
type ArtistKey = Option<String>;

/// What the context menu acts on.
#[derive(Clone)]
enum MenuTarget {
    Tracks(Vec<PathBuf>),
    Album(Album),
    Artist(ArtistKey),
}

#[derive(Clone, Copy)]
enum Query {
    Artists,
    Albums,
    Tracks,
    Search,
    Play,
}

const N_QUERIES: usize = 5;

struct SearchResults {
    artists: Vec<Artist>,
    albums: Vec<(Album, Option<gdk::Texture>)>,
    tracks: Vec<Track>,
}

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/org/gnome/Gumdrop/library-view.ui")]
    pub struct LibraryView {
        #[template_child]
        pub navigation_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub search_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub close_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub searchbar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub root_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub artists_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub search_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub albums_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub albums_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub tracks_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub tracks_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub play_album_button: TemplateChild<gtk::Button>,

        pub db: RefCell<Option<LibraryDB>>,
        /// None until the artists have been loaded once
        pub has_tracks: Cell<Option<bool>>,
        /// The artist whose albums are shown in albums_page
        pub current_artist: RefCell<Option<ArtistKey>>,
        /// The album whose tracks are shown in tracks_page
        pub current_album: RefCell<Option<Album>>,
        pub current_tracks: RefCell<Vec<Track>>,
        pub generations: [Cell<u64>; N_QUERIES],
        pub queue_handler: RefCell<Option<QueueHandler>>,

        pub context_menu: RefCell<Option<gtk::PopoverMenu>>,
        pub(super) menu_target: RefCell<Option<MenuTarget>>,
        /// Where a shift-click selection of tracks starts
        pub selection_anchor: Cell<Option<i32>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibraryView {
        const NAME: &'static str = "GumdropLibraryView";
        type Type = super::LibraryView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_css_name("libraryview");
            klass.set_accessible_role(gtk::AccessibleRole::Group);

            klass.install_action("library.play-next", None, |this, _, _| {
                this.queue_menu_target(QueueAction::PlayNext);
            });
            klass.install_action("library.add-to-queue", None, |this, _, _| {
                this.queue_menu_target(QueueAction::Append);
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for LibraryView {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.obj().setup();
        }
    }

    impl WidgetImpl for LibraryView {}
}

glib::wrapper! {
    pub struct LibraryView(ObjectSubclass<imp::LibraryView>)
        @extends gtk::Widget;
}

impl Default for LibraryView {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl LibraryView {
    pub fn new() -> Self {
        Self::default()
    }

    fn setup(&self) {
        let imp = self.imp();

        imp.searchbar.connect_entry(&imp.search_entry.get());
        imp.search_button
            .bind_property("active", &imp.searchbar.get(), "search-mode-enabled")
            .bidirectional()
            .sync_create()
            .build();

        imp.search_entry.connect_search_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.update_search()
        ));
        imp.searchbar.connect_notify_local(
            Some("search-mode-enabled"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| this.update_search()
            ),
        );

        imp.play_album_button.connect_clicked(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.play_current_album(0)
        ));

        self.setup_context_menu();
        self.setup_track_selection();
        self.update_root_page();
    }

    fn setup_context_menu(&self) {
        let menu = gio::Menu::new();
        menu.append(Some(&i18n("Play Next")), Some("library.play-next"));
        menu.append(Some(&i18n("Add to Queue")), Some("library.add-to-queue"));

        // One menu for every row; parented to the view, which has a layout
        // manager, so it gets positioned without extra work
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        popover.set_parent(self);
        self.imp().context_menu.replace(Some(popover));
    }

    /// Ctrl+click and Shift+click select songs in an album, for the context
    /// menu, instead of playing them. The arrow keys with Shift and
    /// Ctrl+Space work too, as in any GtkListBox.
    fn setup_track_selection(&self) {
        let gesture = gtk::GestureClick::builder()
            .button(gdk::BUTTON_PRIMARY)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        gesture.connect_pressed(clone!(
            #[weak(rename_to = this)]
            self,
            move |gesture, _, _, y| {
                let state = gesture.current_event_state();
                let toggle = state.contains(gdk::ModifierType::CONTROL_MASK);
                let extend = state.contains(gdk::ModifierType::SHIFT_MASK);
                if !toggle && !extend {
                    return;
                }

                if let Some(row) = this.imp().tracks_list.row_at_y(y as i32) {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    this.select_track_row(&row, extend);
                }
            }
        ));
        self.imp().tracks_list.add_controller(gesture);
    }

    fn select_track_row(&self, row: &gtk::ListBoxRow, extend: bool) {
        let imp = self.imp();
        let list = imp.tracks_list.get();
        let index = row.index();

        if extend {
            let anchor = imp.selection_anchor.get().unwrap_or(index);
            list.unselect_all();
            for i in anchor.min(index)..=anchor.max(index) {
                if let Some(r) = list.row_at_index(i) {
                    list.select_row(Some(&r));
                }
            }
        } else {
            if row.is_selected() {
                list.unselect_row(row);
            } else {
                list.select_row(Some(row));
            }
            imp.selection_anchor.set(Some(index));
        }
    }

    fn clear_track_selection(&self) {
        self.imp().tracks_list.unselect_all();
        self.imp().selection_anchor.set(None);
    }

    /// Opens the context menu on secondary click, long press, or the Menu key.
    /// `target` is called when the menu opens, to find what it acts on.
    fn add_context_menu<F>(&self, row: &adw::ActionRow, target: F)
    where
        F: Fn(&Self) -> Option<MenuTarget> + 'static,
    {
        let target = Rc::new(target);

        let click = gtk::GestureClick::builder()
            .button(gdk::BUTTON_SECONDARY)
            .build();
        click.connect_pressed(clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            row,
            #[strong]
            target,
            move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                this.popup_menu(row.upcast_ref(), target(&this), Some((x, y)));
            }
        ));
        row.add_controller(click);

        let long_press = gtk::GestureLongPress::builder().touch_only(true).build();
        long_press.connect_pressed(clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            row,
            #[strong]
            target,
            move |gesture, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                this.popup_menu(row.upcast_ref(), target(&this), Some((x, y)));
            }
        ));
        row.add_controller(long_press);

        if let Some(trigger) = gtk::ShortcutTrigger::parse_string("Menu|<Shift>F10") {
            let action = gtk::CallbackAction::new(clone!(
                #[weak(rename_to = this)]
                self,
                #[weak]
                row,
                #[strong]
                target,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, _| {
                    this.popup_menu(row.upcast_ref(), target(&this), None);
                    glib::Propagation::Stop
                }
            ));
            let shortcuts = gtk::ShortcutController::new();
            shortcuts.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
            row.add_controller(shortcuts);
        }
    }

    /// Shows the context menu at `point`, in `widget` coordinates, or below
    /// `widget` if there is no point.
    fn popup_menu(
        &self,
        widget: &gtk::Widget,
        target: Option<MenuTarget>,
        point: Option<(f64, f64)>,
    ) {
        let imp = self.imp();
        let (target, popover) = match (target, imp.context_menu.borrow().clone()) {
            (Some(target), Some(popover)) => (target, popover),
            _ => return,
        };

        let rect = match point {
            Some((x, y)) => widget
                .compute_point(self, &graphene::Point::new(x as f32, y as f32))
                .map(|p| gdk::Rectangle::new(p.x() as i32, p.y() as i32, 1, 1)),
            None => widget.compute_bounds(self).map(|b| {
                gdk::Rectangle::new(
                    b.x() as i32,
                    b.y() as i32,
                    b.width() as i32,
                    b.height() as i32,
                )
            }),
        };

        imp.menu_target.replace(Some(target));
        popover.set_pointing_to(rect.as_ref());
        popover.popup();
    }

    /// Queues what the context menu was opened on.
    fn queue_menu_target(&self, action: QueueAction) {
        let target = match self.imp().menu_target.take() {
            Some(target) => target,
            None => return,
        };
        self.clear_track_selection();

        match target {
            MenuTarget::Tracks(paths) => self.queue(paths, action),
            target => self.run_query(
                None,
                move |db| target_paths(db, &target),
                move |this, paths| this.queue(paths, action),
            ),
        }
    }

    /// The songs the context menu of the song at `index` acts on: the
    /// selected songs, if that song is one of them, or else just that song.
    fn track_menu_target(&self, index: usize) -> Option<MenuTarget> {
        let imp = self.imp();
        let list = imp.tracks_list.get();
        let row = list.row_at_index(index as i32)?;

        if !row.is_selected() {
            self.clear_track_selection();
            list.select_row(Some(&row));
            imp.selection_anchor.set(Some(row.index()));
        }

        let mut indices: Vec<i32> = list.selected_rows().iter().map(|r| r.index()).collect();
        indices.sort_unstable();

        let tracks = imp.current_tracks.borrow();
        let paths: Vec<PathBuf> = indices
            .into_iter()
            .filter_map(|i| tracks.get(i as usize))
            .map(|t| t.path.clone())
            .collect();
        Some(MenuTarget::Tracks(paths))
    }

    /// Sets the database to browse, and loads it.
    pub fn set_library(&self, db: Option<LibraryDB>) {
        self.imp().db.replace(db);
        self.reload();
    }

    /// Sets the function called when the user picks songs to play or queue.
    pub fn set_queue_handler<F: Fn(Vec<PathBuf>, QueueAction) + 'static>(&self, handler: F) {
        self.imp().queue_handler.replace(Some(Rc::new(handler)));
    }

    /// Reloads every page from the database, e.g. after a scan finished.
    pub fn reload(&self) {
        let imp = self.imp();

        self.load_artists();
        self.update_search();

        let artist = imp.current_artist.borrow().clone();
        if let Some(artist) = artist {
            self.load_albums(artist);
        }

        let album = imp.current_album.borrow().clone();
        if let Some(album) = album {
            self.load_tracks(album);
        }
    }

    pub fn close_button(&self) -> gtk::Button {
        self.imp().close_button.get()
    }

    pub fn searchbar(&self) -> gtk::SearchBar {
        self.imp().searchbar.get()
    }

    pub fn set_search(&self, search: bool) {
        let imp = self.imp();
        if search {
            // Searching only makes sense from the root page
            imp.navigation_view.pop_to_tag("artists");
        }
        imp.searchbar.set_search_mode(search);
    }

    /// Runs `query` against the database on a worker thread, then passes the
    /// result to `update` on the main thread, unless a newer query of the same
    /// kind was started in the meantime.
    fn query<T, Q, U>(&self, kind: Query, query: Q, update: U)
    where
        T: Send + 'static,
        Q: FnOnce(&LibraryDB) -> db::Result<T> + Send + 'static,
        U: FnOnce(&Self, T) + 'static,
    {
        self.run_query(Some(kind), query, update);
    }

    /// Like query(), but with `kind` None the result is always applied.
    fn run_query<T, Q, U>(&self, kind: Option<Query>, query: Q, update: U)
    where
        T: Send + 'static,
        Q: FnOnce(&LibraryDB) -> db::Result<T> + Send + 'static,
        U: FnOnce(&Self, T) + 'static,
    {
        let db = match self.imp().db.borrow().clone() {
            Some(db) => db,
            None => return,
        };

        let generation = kind.map(|kind| (kind, self.next_generation(kind)));
        let this = self.downgrade();
        glib::spawn_future_local(async move {
            let res = gio::spawn_blocking(move || query(&db)).await;

            let this = match this.upgrade() {
                Some(this) => this,
                None => return,
            };
            if let Some((kind, generation)) = generation {
                if this.imp().generations[kind as usize].get() != generation {
                    return;
                }
            }

            match res {
                Ok(Ok(value)) => update(&this, value),
                Ok(Err(e)) => warn!("Unable to query the music library: {}", e),
                Err(_) => warn!("Music library query panicked"),
            }
        });
    }

    /// Invalidates pending queries of this kind and returns the new generation.
    fn next_generation(&self, kind: Query) -> u64 {
        let generation = &self.imp().generations[kind as usize];
        generation.set(generation.get().wrapping_add(1));
        generation.get()
    }

    fn queue(&self, paths: Vec<PathBuf>, action: QueueAction) {
        if paths.is_empty() {
            return;
        }

        let handler = self.imp().queue_handler.borrow().clone();
        if let Some(handler) = handler {
            handler(paths, action);
        }
    }

    fn play(&self, paths: Vec<PathBuf>, index: usize) {
        self.queue(paths, QueueAction::Play(index));
    }

    fn play_current_album(&self, index: usize) {
        self.clear_track_selection();

        let paths: Vec<PathBuf> = self
            .imp()
            .current_tracks
            .borrow()
            .iter()
            .map(|t| t.path.clone())
            .collect();
        self.play(paths, index);
    }

    /// Plays the album containing `track`, starting from `track`.
    fn play_track_in_album(&self, track: Track) {
        self.query(
            Query::Play,
            move |db| {
                let tracks = db.album_tracks(&track.album, track.album_artist.as_deref())?;
                Ok((tracks, track.path))
            },
            |this, (tracks, path)| match tracks.iter().position(|t| t.path == path) {
                Some(index) => this.play(tracks.into_iter().map(|t| t.path).collect(), index),
                None => this.play(vec![path], 0),
            },
        );
    }

    // ---- Artists ----

    fn load_artists(&self) {
        self.query(
            Query::Artists,
            |db| {
                let mut artists: Vec<(ArtistKey, u32)> = Vec::new();
                let mut various = None;
                for artist in db.album_artists()? {
                    if artist.name == LibraryDB::VARIOUS_ARTISTS {
                        various = Some(artist.num_albums);
                    } else {
                        artists.push((Some(artist.name), artist.num_albums));
                    }
                }

                // Compilations and unknown artists go at the end of the list
                if let Some(num_albums) = various {
                    artists.push((Some(LibraryDB::VARIOUS_ARTISTS.to_string()), num_albums));
                }
                let unknown = db.albums_without_album_artist()?.len() as u32;
                if unknown > 0 {
                    artists.push((None, unknown));
                }

                Ok(artists)
            },
            |this, artists| this.show_artists(artists),
        );
    }

    fn show_artists(&self, artists: Vec<(ArtistKey, u32)>) {
        let imp = self.imp();

        imp.artists_list.remove_all();
        imp.has_tracks.set(Some(!artists.is_empty()));
        for (artist, num_albums) in artists {
            imp.artists_list
                .append(&self.artist_row(artist, num_albums));
        }

        self.update_root_page();
    }

    fn artist_row(&self, artist: ArtistKey, num_albums: u32) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(artist_display_name(artist.as_deref()))
            .subtitle(ni18n_f(
                // Translators: the `{}` must be left unmodified;
                // it will be expanded to the number of albums
                "{} album",
                "{} albums",
                num_albums,
                &[&num_albums.to_string()],
            ))
            .use_markup(false)
            .activatable(true)
            .build();
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));

        let menu_artist = artist.clone();
        self.add_context_menu(&row, move |_| Some(MenuTarget::Artist(menu_artist.clone())));
        row.connect_activated(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.show_artist(artist.clone())
        ));

        row
    }

    fn show_artist(&self, artist: ArtistKey) {
        let imp = self.imp();

        imp.albums_page
            .set_title(&artist_display_name(artist.as_deref()));
        imp.albums_list.remove_all();
        imp.current_artist.replace(Some(artist.clone()));
        self.load_albums(artist);

        imp.navigation_view
            .replace_with_tags(&["artists", "albums"]);
    }

    // ---- Albums ----

    fn load_albums(&self, artist: ArtistKey) {
        self.query(
            Query::Albums,
            move |db| {
                Ok(artist_albums(db, &artist)?
                    .into_iter()
                    .map(|album| {
                        let cover = load_album_cover(&album);
                        (album, cover)
                    })
                    .collect::<Vec<_>>())
            },
            |this, albums| this.show_albums(albums),
        );
    }

    fn show_albums(&self, albums: Vec<(Album, Option<gdk::Texture>)>) {
        let imp = self.imp();
        let artist = imp.current_artist.borrow().clone().flatten();

        imp.albums_list.remove_all();
        for (album, cover) in albums {
            imp.albums_list
                .append(&self.album_row(album, cover.as_ref(), artist.as_deref()));
        }
    }

    /// `artist` is the artist being browsed, if any; the album artist is only
    /// shown when it differs.
    fn album_row(
        &self,
        album: Album,
        cover: Option<&gdk::Texture>,
        artist: Option<&str>,
    ) -> adw::ActionRow {
        let mut details = Vec::new();
        if artist.is_none() || album.album_artist.as_deref() != artist {
            details.push(artist_display_name(album.album_artist.as_deref()));
        }
        if let Some(year) = &album.year {
            details.push(year.clone());
        }
        details.push(ni18n_f(
            // Translators: the `{}` must be left unmodified;
            // it will be expanded to the number of songs
            "{} song",
            "{} songs",
            album.num_tracks,
            &[&album.num_tracks.to_string()],
        ));

        let row = adw::ActionRow::builder()
            .title(album_display_title(&album.title))
            .subtitle(details.join(" · "))
            .use_markup(false)
            .activatable(true)
            .build();
        row.add_prefix(&cover_image(cover));
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));

        let menu_album = album.clone();
        self.add_context_menu(&row, move |_| Some(MenuTarget::Album(menu_album.clone())));
        row.connect_activated(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.show_album(album.clone())
        ));

        row
    }

    fn show_album(&self, album: Album) {
        let imp = self.imp();

        imp.tracks_page
            .set_title(&album_display_title(&album.title));
        imp.tracks_list.remove_all();
        imp.current_tracks.borrow_mut().clear();
        imp.current_album.replace(Some(album.clone()));
        self.load_tracks(album);

        let visible_tag = imp.navigation_view.visible_page().and_then(|p| p.tag());
        if visible_tag.as_deref() == Some("albums") {
            imp.navigation_view.push_by_tag("tracks");
        } else {
            imp.navigation_view
                .replace_with_tags(&["artists", "tracks"]);
        }
    }

    // ---- Tracks ----

    fn load_tracks(&self, album: Album) {
        self.query(
            Query::Tracks,
            move |db| db.album_tracks(&album.title, album.album_artist.as_deref()),
            |this, tracks| this.show_tracks(tracks),
        );
    }

    fn show_tracks(&self, tracks: Vec<Track>) {
        let imp = self.imp();
        let album_artist = imp
            .current_album
            .borrow()
            .as_ref()
            .and_then(|a| a.album_artist.clone());
        let multi_disc = tracks.iter().filter_map(|t| t.disc).max().unwrap_or(0) > 1;

        imp.tracks_list.remove_all();
        imp.selection_anchor.set(None);
        for (index, track) in tracks.iter().enumerate() {
            imp.tracks_list.append(&self.track_row(
                track,
                index,
                album_artist.as_deref(),
                multi_disc,
            ));
        }
        imp.play_album_button.set_sensitive(!tracks.is_empty());
        imp.current_tracks.replace(tracks);
    }

    fn track_row(
        &self,
        track: &Track,
        index: usize,
        album_artist: Option<&str>,
        multi_disc: bool,
    ) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(&track.title)
            .use_markup(false)
            .activatable(true)
            .build();

        // Only credit artists when they differ from the album artist
        if !track.artists.is_empty()
            && !(track.artists.len() == 1 && Some(track.artists[0].as_str()) == album_artist)
        {
            row.set_subtitle(&track.artists.join(", "));
        }

        let number = match (track.disc, track.track) {
            (Some(disc), Some(n)) if multi_disc => format!("{}.{}", disc, n),
            (_, Some(n)) => n.to_string(),
            _ => String::new(),
        };
        let number_label = gtk::Label::builder()
            .label(number)
            .width_chars(3)
            .xalign(1.0)
            .css_classes(["dim-label", "numeric"])
            .build();
        row.add_prefix(&number_label);
        row.add_suffix(&duration_label(track.duration));

        self.add_context_menu(&row, move |this| this.track_menu_target(index));
        row.connect_activated(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.play_current_album(index)
        ));

        row
    }

    // ---- Search ----

    fn search_text(&self) -> String {
        self.imp().search_entry.text().trim().to_string()
    }

    fn is_searching(&self) -> bool {
        self.imp().searchbar.is_search_mode() && !self.search_text().is_empty()
    }

    fn update_search(&self) {
        if !self.is_searching() {
            // Drop any search still in flight
            self.next_generation(Query::Search);
            self.clear_search_results();
            self.update_root_page();
            return;
        }

        let text = self.search_text();
        self.query(
            Query::Search,
            move |db| search_library(db, &text),
            |this, results| this.show_search_results(results),
        );
    }

    fn clear_search_results(&self) {
        let search_box = self.imp().search_box.get();
        while let Some(child) = search_box.first_child() {
            search_box.remove(&child);
        }
    }

    fn show_search_results(&self, results: SearchResults) {
        let imp = self.imp();
        self.clear_search_results();

        if results.artists.is_empty() && results.albums.is_empty() && results.tracks.is_empty() {
            imp.root_stack.set_visible_child_name("no-results");
            return;
        }

        if !results.artists.is_empty() {
            let group = adw::PreferencesGroup::builder()
                .title(i18n("Artists"))
                .build();
            for artist in results.artists {
                group.add(&self.artist_row(Some(artist.name), artist.num_albums));
            }
            imp.search_box.append(&group);
        }

        if !results.albums.is_empty() {
            let group = adw::PreferencesGroup::builder()
                .title(i18n("Albums"))
                .build();
            for (album, cover) in results.albums {
                group.add(&self.album_row(album, cover.as_ref(), None));
            }
            imp.search_box.append(&group);
        }

        if !results.tracks.is_empty() {
            let group = adw::PreferencesGroup::builder()
                .title(i18n("Songs"))
                .build();
            for track in results.tracks {
                group.add(&self.search_track_row(track));
            }
            imp.search_box.append(&group);
        }

        imp.root_stack.set_visible_child_name("search");
    }

    fn search_track_row(&self, track: Track) -> adw::ActionRow {
        let artist = if track.artists.is_empty() {
            artist_display_name(track.album_artist.as_deref())
        } else {
            track.artists.join(", ")
        };

        let row = adw::ActionRow::builder()
            .title(&track.title)
            .subtitle(format!(
                "{} · {}",
                artist,
                album_display_title(&track.album)
            ))
            .use_markup(false)
            .activatable(true)
            .build();
        row.add_suffix(&duration_label(track.duration));

        let menu_path = track.path.clone();
        self.add_context_menu(&row, move |_| {
            Some(MenuTarget::Tracks(vec![menu_path.clone()]))
        });
        row.connect_activated(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.play_track_in_album(track.clone())
        ));

        row
    }

    /// Shows the artists, or an empty state, unless search results are shown.
    fn update_root_page(&self) {
        if self.is_searching() {
            return;
        }

        // Avoid flashing the empty state before the first load finishes
        let imp = self.imp();
        if imp.has_tracks.get() == Some(false) {
            imp.root_stack.set_visible_child_name("empty");
        } else {
            imp.root_stack.set_visible_child_name("artists");
        }
    }
}

fn search_library(db: &LibraryDB, query: &str) -> db::Result<SearchResults> {
    let matcher = SkimMatcherV2::default().ignore_case();

    // Album artists and track artists, without duplicates
    let mut candidates = db.album_artists()?;
    for artist in db.artists()? {
        if !candidates.iter().any(|a| a.name == artist.name) {
            candidates.push(artist);
        }
    }
    let mut artists: Vec<(i64, Artist)> = candidates
        .into_iter()
        .filter_map(|a| {
            let name = artist_display_name(Some(&a.name));
            matcher.fuzzy_match(&name, query).map(|score| (score, a))
        })
        .collect();
    artists.sort_by(|a, b| b.0.cmp(&a.0));

    let mut albums: Vec<(i64, Album)> = db
        .albums()?
        .into_iter()
        .filter_map(|a| {
            let key = format!(
                "{} {}",
                a.title,
                artist_display_name(a.album_artist.as_deref())
            );
            matcher.fuzzy_match(&key, query).map(|score| (score, a))
        })
        .collect();
    albums.sort_by(|a, b| b.0.cmp(&a.0));

    let mut tracks = db.search_tracks(query)?;
    tracks.truncate(MAX_SONG_RESULTS);

    Ok(SearchResults {
        artists: artists
            .into_iter()
            .take(MAX_ARTIST_RESULTS)
            .map(|(_, a)| a)
            .collect(),
        albums: albums
            .into_iter()
            .take(MAX_ALBUM_RESULTS)
            .map(|(_, album)| {
                let cover = load_album_cover(&album);
                (album, cover)
            })
            .collect(),
        tracks,
    })
}

/// The albums of an artist, in the order the albums page shows them.
fn artist_albums(db: &LibraryDB, artist: &ArtistKey) -> db::Result<Vec<Album>> {
    let mut albums = match artist {
        Some(name) => db.artist_albums(name)?,
        None => db.albums_without_album_artist()?,
    };
    albums.sort_by(|a, b| {
        a.year
            .cmp(&b.year)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    Ok(albums)
}

/// The songs a context menu acts on, in playing order.
fn target_paths(db: &LibraryDB, target: &MenuTarget) -> db::Result<Vec<PathBuf>> {
    let albums = match target {
        MenuTarget::Tracks(paths) => return Ok(paths.clone()),
        MenuTarget::Album(album) => vec![album.clone()],
        MenuTarget::Artist(artist) => artist_albums(db, artist)?,
    };

    let mut paths = Vec::new();
    for album in albums {
        let tracks = db.album_tracks(&album.title, album.album_artist.as_deref())?;
        paths.extend(tracks.into_iter().map(|t| t.path));
    }
    Ok(paths)
}

fn artist_display_name(artist: Option<&str>) -> String {
    match artist {
        None => i18n("Unknown Artist"),
        Some(name) if name == LibraryDB::VARIOUS_ARTISTS => i18n("Various Artists"),
        Some(name) => name.to_string(),
    }
}

fn album_display_title(title: &str) -> String {
    if title.is_empty() {
        i18n("Unknown Album")
    } else {
        title.to_string()
    }
}

fn duration_label(seconds: f64) -> gtk::Label {
    gtk::Label::builder()
        .label(utils::format_time(seconds.round() as i64))
        .css_classes(["dim-label", "numeric"])
        .build()
}

fn cover_image(cover: Option<&gdk::Texture>) -> gtk::Image {
    let image = match cover {
        Some(texture) => gtk::Image::from_paintable(Some(texture)),
        None => gtk::Image::from_icon_name("audio-x-generic-symbolic"),
    };
    image.set_pixel_size(COVER_SIZE);
    image.add_css_class("album-cover");
    if cover.is_none() {
        image.add_css_class("missing");
    }
    image
}

/// Loads the album's cover image file or, failing that, the cover art embedded
/// in one of its tracks. Blocking; call from a worker thread.
fn load_album_cover(album: &Album) -> Option<gdk::Texture> {
    let pixbuf = album
        .cover
        .as_ref()
        .and_then(|path| {
            gdk_pixbuf::Pixbuf::from_file_at_scale(path, COVER_DECODE_SIZE, COVER_DECODE_SIZE, true)
                .ok()
        })
        .or_else(|| load_embedded_cover(&album.sample_track))?;

    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

fn load_embedded_cover(path: &Path) -> Option<gdk_pixbuf::Pixbuf> {
    let tagged_file = lofty::read_from_path(path).ok()?;
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())?;
    let picture = tag
        .get_picture_type(PictureType::CoverFront)
        .or_else(|| tag.pictures().first())?;

    let bytes = glib::Bytes::from(picture.data());
    let stream = gio::MemoryInputStream::from_bytes(&bytes);
    gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        COVER_DECODE_SIZE,
        COVER_DECODE_SIZE,
        true,
        gio::Cancellable::NONE,
    )
    .ok()
}
