// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

// Browses the music library: artists, then an artist's albums, then an
// album's songs, plus a search across all three. In albums mode, browsing
// starts from a list of every album instead.
//
// Each list is a GtkListView of LibraryItems shown by LibraryRows, the same
// way the queue shows Songs with QueueRows, so both can be styled with CSS:
// see 'libraryrow' in style.css.
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
    i18n::i18n,
    library::{db, Album, Artist, LibraryDB, Track},
    library_item::{
        album_details, album_display_title, albums_label, artist_display_name, artists_label,
        ArtistKey, LibraryItem, LibraryItemData, LibraryItemKind,
    },
    library_row::LibraryRow,
};

/// Covers are decoded at twice the size of a small CoverPicture for HiDPI displays
const COVER_DECODE_SIZE: i32 = 96;

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

/// What the root page of a library view lists
#[derive(Clone, Copy, Debug, glib::Enum, PartialEq, Default)]
#[enum_type(name = "GumdropLibraryMode")]
pub enum LibraryMode {
    #[default]
    #[enum_value(name = "artists")]
    Artists,
    #[enum_value(name = "albums")]
    Albums,
}

/// What the context menu acts on.
#[derive(Clone)]
enum MenuTarget {
    Tracks(Vec<PathBuf>),
    Album(Album),
    Artist(ArtistKey),
}

#[derive(Clone, Copy)]
enum Query {
    Root,
    Covers,
    Albums,
    Tracks,
    Search,
    Play,
}

const N_QUERIES: usize = 6;

struct SearchResults {
    artists: Vec<Artist>,
    albums: Vec<(Album, Option<gdk::Texture>)>,
    tracks: Vec<Track>,
}

mod imp {
    use glib::{ParamSpec, ParamSpecEnum, Value};
    use once_cell::sync::Lazy;

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
        pub library_searchbar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub library_searchentry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub root_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub root_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub root_title_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub root_subtitle_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub root_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub root_model: TemplateChild<gio::ListStore>,
        #[template_child]
        pub search_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub search_sections: TemplateChild<gio::ListStore>,
        #[template_child]
        pub search_artists_model: TemplateChild<gio::ListStore>,
        #[template_child]
        pub search_albums_model: TemplateChild<gio::ListStore>,
        #[template_child]
        pub search_tracks_model: TemplateChild<gio::ListStore>,
        #[template_child]
        pub albums_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub albums_title_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub albums_subtitle_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub albums_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub albums_model: TemplateChild<gio::ListStore>,
        #[template_child]
        pub tracks_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub tracks_title_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub tracks_subtitle_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub play_album_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub tracks_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub tracks_selection: TemplateChild<gtk::MultiSelection>,
        #[template_child]
        pub tracks_model: TemplateChild<gio::ListStore>,
        #[template_child]
        pub context_menu: TemplateChild<gtk::PopoverMenu>,

        pub mode: Cell<LibraryMode>,
        pub db: RefCell<Option<LibraryDB>>,
        /// None until the root page has been loaded once
        pub has_tracks: Cell<Option<bool>>,
        /// The artist whose albums are shown in albums_page
        pub current_artist: RefCell<Option<ArtistKey>>,
        /// The album whose tracks are shown in tracks_page
        pub current_album: RefCell<Option<Album>>,
        pub current_tracks: RefCell<Vec<Track>>,
        pub generations: [Cell<u64>; N_QUERIES],
        pub queue_handler: RefCell<Option<QueueHandler>>,
        pub(super) menu_target: RefCell<Option<MenuTarget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibraryView {
        const NAME: &'static str = "GumdropLibraryView";
        type Type = super::LibraryView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            LibraryItem::static_type();
            LibraryRow::static_type();

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

            self.obj().setup_searchbar();
            self.obj().setup_views();
            self.obj().setup_mode();
            self.obj().update_root_page();
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![ParamSpecEnum::builder::<LibraryMode>("mode")
                    .construct_only()
                    .build()]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "mode" => {
                    let p = value
                        .get::<LibraryMode>()
                        .expect("The value needs to be a LibraryMode");
                    self.mode.set(p);
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "mode" => self.mode.get().to_value(),
                _ => unimplemented!(),
            }
        }
    }

    impl WidgetImpl for LibraryView {}
}

glib::wrapper! {
    pub struct LibraryView(ObjectSubclass<imp::LibraryView>)
        @extends gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible,
                    gtk::Buildable, gtk::ConstraintTarget;
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

    fn setup_mode(&self) {
        let imp = self.imp();
        let title = match imp.mode.get() {
            LibraryMode::Artists => i18n("Artists"),
            LibraryMode::Albums => i18n("Albums"),
        };
        imp.root_page.set_title(&title);
        imp.root_title_label.set_label(&title);
        imp.root_view
            .update_property(&[gtk::accessible::Property::Label(&title)]);
    }

    pub fn mode(&self) -> LibraryMode {
        self.imp().mode.get()
    }

    fn setup_searchbar(&self) {
        let imp = self.imp();

        let entry = imp.library_searchentry.get();
        imp.library_searchbar.connect_entry(&entry);
        imp.search_button
            .bind_property(
                "active",
                &imp.library_searchbar.get(),
                "search-mode-enabled",
            )
            .bidirectional()
            .sync_create()
            .build();

        imp.library_searchentry.connect_search_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.update_search()
        ));
        imp.library_searchbar.connect_notify_local(
            Some("search-mode-enabled"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| this.update_search()
            ),
        );
    }

    fn setup_views(&self) {
        let imp = self.imp();

        // Each search section is a list of its own
        imp.search_sections.append(&imp.search_artists_model.get());
        imp.search_sections.append(&imp.search_albums_model.get());
        imp.search_sections.append(&imp.search_tracks_model.get());
        imp.search_view
            .set_header_factory(Some(&self.section_header_factory()));

        for view in [
            imp.root_view.get(),
            imp.albums_view.get(),
            imp.search_view.get(),
        ] {
            view.set_factory(Some(&self.row_factory(false)));
            view.connect_activate(clone!(
                #[weak(rename_to = this)]
                self,
                move |view, pos| {
                    if let Some(item) = view.model().and_then(|m| m.item(pos)).and_downcast() {
                        this.activate_item(&item);
                    }
                }
            ));
            self.add_menu_shortcut(&view, false);
        }

        // Mouse clicks are handled by the rows (this is for the keyboard)
        imp.tracks_view.set_factory(Some(&self.row_factory(true)));
        imp.tracks_view.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |_, pos| this.play_current_album(pos as usize)
        ));
        self.add_menu_shortcut(&imp.tracks_view.get(), true);

        imp.play_album_button.connect_clicked(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.play_current_album(0)
        ));
    }

    /// 'album_tracks' is set for the list of an album's songs whose rows
    fn row_factory(&self, album_tracks: bool) -> gtk::ListItemFactory {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(clone!(
            #[weak(rename_to = this)]
            self,
            move |_, item| {
                let row = LibraryRow::default();
                let list_item = item.downcast_ref::<gtk::ListItem>().unwrap();
                list_item.set_child(Some(&row));

                list_item
                    .bind_property("item", &row, "item")
                    .sync_create()
                    .build();

                list_item
                    .property_expression("item")
                    .chain_property::<LibraryItem>("kind")
                    .bind(&row, "kind", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<LibraryItem>("title")
                    .bind(&row, "title", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<LibraryItem>("subtitle")
                    .bind(&row, "subtitle", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<LibraryItem>("number")
                    .bind(&row, "number", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<LibraryItem>("duration")
                    .bind(&row, "duration", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<LibraryItem>("cover")
                    .bind(&row, "cover", gtk::Widget::NONE);

                this.add_context_menu(&row, album_tracks);
                if album_tracks {
                    this.add_track_clicks(&row);
                }
            }
        ));
        factory.upcast()
    }

    fn section_header_factory(&self) -> gtk::ListItemFactory {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let header = item.downcast_ref::<gtk::ListHeader>().unwrap();
            let label = gtk::Label::builder()
                .xalign(0.0)
                .css_classes(["heading"])
                .build();
            header.set_child(Some(&label));
        });
        factory.connect_bind(|_, item| {
            let header = item.downcast_ref::<gtk::ListHeader>().unwrap();
            let kind = header
                .item()
                .and_downcast::<LibraryItem>()
                .map(|item| item.kind());
            let title = match kind {
                Some(LibraryItemKind::Artist) => i18n("Artists"),
                Some(LibraryItemKind::Album) => i18n("Albums"),
                Some(LibraryItemKind::Track) => i18n("Songs"),
                None => String::new(),
            };
            if let Some(label) = header.child().and_downcast::<gtk::Label>() {
                label.set_label(&title);
            }
        });
        factory.upcast()
    }

    fn activate_item(&self, item: &LibraryItem) {
        match item.data() {
            LibraryItemData::Artist(artist) => self.show_artist(artist),
            LibraryItemData::Album(album) => self.show_album(album),
            LibraryItemData::Track(track) => self.play_track_in_album(track),
        }
    }

    /// A plain click on a song plays its album from there. Ctrl+Click and
    /// Shift+Click are left to the list which selects songs for the context
    /// menu; so do Shift with the arrow keys and Ctrl+Space
    fn add_track_clicks(&self, row: &LibraryRow) {
        let gesture = gtk::GestureClick::builder()
            .button(gdk::BUTTON_PRIMARY)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        gesture.connect_pressed(|gesture, _, _, _| {
            let state = gesture.current_event_state();
            if !state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK) {
                gesture.set_state(gtk::EventSequenceState::Claimed);
            }
        });
        gesture.connect_released(clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            row,
            move |_, n_press, _, _| {
                if n_press != 1 {
                    return;
                }
                if let Some(pos) = row.item().and_then(|item| this.track_position(&item)) {
                    this.play_current_album(pos as usize);
                }
            }
        ));
        row.add_controller(gesture);
    }

    /// Opens the context menu on secondary click or long press.
    fn add_context_menu(&self, row: &LibraryRow, album_tracks: bool) {
        let click = gtk::GestureClick::builder()
            .button(gdk::BUTTON_SECONDARY)
            .build();
        click.connect_pressed(clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            row,
            move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                this.popup_menu(&row, album_tracks, Some((x, y)));
            }
        ));
        row.add_controller(click);

        let long_press = gtk::GestureLongPress::builder().touch_only(true).build();
        long_press.connect_pressed(clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            row,
            move |gesture, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                this.popup_menu(&row, album_tracks, Some((x, y)));
            }
        ));
        row.add_controller(long_press);
    }

    /// Opens the context menu of the focused row with the Menu key
    fn add_menu_shortcut(&self, view: &gtk::ListView, album_tracks: bool) {
        let trigger = match gtk::ShortcutTrigger::parse_string("Menu|<Shift>F10") {
            Some(trigger) => trigger,
            None => return,
        };
        let action = gtk::CallbackAction::new(clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |view, _| match focused_row(view) {
                Some(row) => {
                    this.popup_menu(&row, album_tracks, None);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        ));

        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
        view.add_controller(shortcuts);
    }

    /// Shows the context menu for 'row' at 'point' in row coordinates or
    /// below the row if there is no point
    fn popup_menu(&self, row: &LibraryRow, album_tracks: bool, point: Option<(f64, f64)>) {
        let target = match row
            .item()
            .and_then(|item| self.menu_target(&item, album_tracks))
        {
            Some(target) => target,
            None => return,
        };

        let rect = match point {
            Some((x, y)) => row
                .compute_point(self, &graphene::Point::new(x as f32, y as f32))
                .map(|p| gdk::Rectangle::new(p.x() as i32, p.y() as i32, 1, 1)),
            None => row.compute_bounds(self).map(|b| {
                gdk::Rectangle::new(
                    b.x() as i32,
                    b.y() as i32,
                    b.width() as i32,
                    b.height() as i32,
                )
            }),
        };

        let imp = self.imp();
        imp.menu_target.replace(Some(target));
        imp.context_menu.set_pointing_to(rect.as_ref());
        imp.context_menu.popup();
    }

    fn menu_target(&self, item: &LibraryItem, album_tracks: bool) -> Option<MenuTarget> {
        match item.data() {
            LibraryItemData::Artist(artist) => Some(MenuTarget::Artist(artist)),
            LibraryItemData::Album(album) => Some(MenuTarget::Album(album)),
            LibraryItemData::Track(_) if album_tracks => {
                self.track_menu_target(self.track_position(item)?)
            }
            LibraryItemData::Track(track) => Some(MenuTarget::Tracks(vec![track.path])),
        }
    }

    /// The songs the context menu of the song at 'pos' acts on; the selected
    /// songs, if that song is one of them, or else just that song
    fn track_menu_target(&self, pos: u32) -> Option<MenuTarget> {
        let imp = self.imp();
        let selection = imp.tracks_selection.get();

        if !selection.is_selected(pos) {
            selection.select_item(pos, true);
        }

        let tracks = imp.current_tracks.borrow();
        let paths: Vec<PathBuf> = (0..selection.n_items())
            .filter(|&i| selection.is_selected(i))
            .filter_map(|i| tracks.get(i as usize))
            .map(|t| t.path.clone())
            .collect();
        Some(MenuTarget::Tracks(paths))
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

    fn track_position(&self, item: &LibraryItem) -> Option<u32> {
        let model = self.imp().tracks_model.get();
        (0..model.n_items()).find(|&i| model.item(i).as_ref() == Some(item.upcast_ref()))
    }

    fn clear_track_selection(&self) {
        self.imp().tracks_selection.unselect_all();
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

        self.load_root();
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

    pub fn library_searchbar(&self) -> gtk::SearchBar {
        self.imp().library_searchbar.get()
    }

    pub fn library_searchentry(&self) -> gtk::SearchEntry {
        self.imp().library_searchentry.get()
    }

    pub fn set_search(&self, search: bool) {
        let imp = self.imp();
        if search {
            // Searching only makes sense from the root page
            imp.navigation_view.pop_to_tag("root");
        }
        imp.library_searchbar.set_search_mode(search);
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

    fn load_root(&self) {
        match self.imp().mode.get() {
            LibraryMode::Artists => self.load_artists(),
            LibraryMode::Albums => self.load_all_albums(),
        }
    }

    /// Shows the items of the root page and whether the library has songs
    fn show_root_items(&self, items: &[LibraryItem], subtitle: &str) {
        let imp = self.imp();

        imp.root_model.splice(0, imp.root_model.n_items(), items);
        imp.root_subtitle_label.set_label(subtitle);
        imp.has_tracks.set(Some(!items.is_empty()));

        self.update_root_page();
    }

    // ---- Artists ----

    fn load_artists(&self) {
        self.query(
            Query::Root,
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
        let items: Vec<LibraryItem> = artists
            .into_iter()
            .map(|(artist, num_albums)| LibraryItem::artist(artist, num_albums))
            .collect();
        self.show_root_items(&items, &artists_label(items.len() as u32));
    }

    fn show_artist(&self, artist: ArtistKey) {
        let imp = self.imp();

        let name = artist_display_name(artist.as_deref());
        imp.albums_page.set_title(&name);
        imp.albums_title_label.set_label(&name);
        imp.albums_subtitle_label.set_label("");
        imp.albums_model.remove_all();
        imp.current_artist.replace(Some(artist.clone()));
        self.load_albums(artist);

        imp.navigation_view.replace_with_tags(&["root", "albums"]);
    }

    // ---- Albums ----

    /// Lists every album for albums mode. The covers are loaded afterwards
    fn load_all_albums(&self) {
        self.query(
            Query::Root,
            |db| {
                let mut albums = db.albums()?;
                // Albums without a title go at the end
                albums.sort_by_cached_key(|a| {
                    (
                        a.title.is_empty(),
                        a.title.to_lowercase(),
                        a.album_artist.as_deref().map(str::to_lowercase),
                    )
                });
                Ok(albums)
            },
            |this, albums| this.show_all_albums(albums),
        );
    }

    fn show_all_albums(&self, albums: Vec<Album>) {
        let items: Vec<LibraryItem> = albums
            .iter()
            .map(|album| LibraryItem::album(album.clone(), None, None))
            .collect();
        self.show_root_items(&items, &albums_label(items.len() as u32));

        self.query(
            Query::Covers,
            move |_| Ok(albums.iter().map(load_album_cover).collect::<Vec<_>>()),
            move |_, covers| {
                for (item, cover) in items.iter().zip(covers) {
                    if cover.is_some() {
                        item.set_cover(cover);
                    }
                }
            },
        );
    }

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

        let items: Vec<LibraryItem> = albums
            .into_iter()
            .map(|(album, cover)| LibraryItem::album(album, cover, artist.as_deref()))
            .collect();
        imp.albums_model
            .splice(0, imp.albums_model.n_items(), &items);
        imp.albums_subtitle_label
            .set_label(&albums_label(items.len() as u32));
    }

    fn show_album(&self, album: Album) {
        let imp = self.imp();

        let title = album_display_title(&album.title);
        imp.tracks_page.set_title(&title);
        imp.tracks_title_label.set_label(&title);
        imp.tracks_subtitle_label
            .set_label(&album_details(&album, None));
        imp.tracks_model.remove_all();
        imp.current_tracks.borrow_mut().clear();
        imp.current_album.replace(Some(album.clone()));
        self.load_tracks(album);

        let visible_tag = imp.navigation_view.visible_page().and_then(|p| p.tag());
        if visible_tag.as_deref() == Some("albums") {
            imp.navigation_view.push_by_tag("tracks");
        } else {
            imp.navigation_view.replace_with_tags(&["root", "tracks"]);
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

        let items: Vec<LibraryItem> = tracks
            .iter()
            .map(|track| {
                LibraryItem::album_track(track.clone(), album_artist.as_deref(), multi_disc)
            })
            .collect();
        imp.tracks_model
            .splice(0, imp.tracks_model.n_items(), &items);
        imp.play_album_button.set_sensitive(!tracks.is_empty());
        imp.current_tracks.replace(tracks);
    }

    // ---- Search ----

    fn search_text(&self) -> String {
        self.imp().library_searchentry.text().trim().to_string()
    }

    fn is_searching(&self) -> bool {
        self.imp().library_searchbar.is_search_mode() && !self.search_text().is_empty()
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
        let imp = self.imp();
        imp.search_artists_model.remove_all();
        imp.search_albums_model.remove_all();
        imp.search_tracks_model.remove_all();
    }

    fn show_search_results(&self, results: SearchResults) {
        let imp = self.imp();

        if results.artists.is_empty() && results.albums.is_empty() && results.tracks.is_empty() {
            self.clear_search_results();
            imp.root_stack.set_visible_child_name("no-results");
            return;
        }

        let artists: Vec<LibraryItem> = results
            .artists
            .into_iter()
            .map(|artist| LibraryItem::artist(Some(artist.name), artist.num_albums))
            .collect();
        let albums: Vec<LibraryItem> = results
            .albums
            .into_iter()
            .map(|(album, cover)| LibraryItem::album(album, cover, None))
            .collect();
        let tracks: Vec<LibraryItem> = results.tracks.into_iter().map(LibraryItem::track).collect();

        imp.search_artists_model
            .splice(0, imp.search_artists_model.n_items(), &artists);
        imp.search_albums_model
            .splice(0, imp.search_albums_model.n_items(), &albums);
        imp.search_tracks_model
            .splice(0, imp.search_tracks_model.n_items(), &tracks);

        imp.root_stack.set_visible_child_name("search");
    }

    /// Shows the artists, or an empty state unless search results are shown
    fn update_root_page(&self) {
        if self.is_searching() {
            return;
        }

        // Avoid flashing the empty state before the first load finishes
        let imp = self.imp();
        if imp.has_tracks.get() == Some(false) {
            imp.root_stack.set_visible_child_name("empty");
        } else {
            imp.root_stack.set_visible_child_name("list");
        }
    }
}

/// The row that has the keyboard focus in 'view' if any
fn focused_row(view: &gtk::Widget) -> Option<LibraryRow> {
    let focus = view.root()?.focus()?;
    if !focus.is_ancestor(view) {
        return None;
    }

    // The focus is on the list item that holds the row
    focus
        .clone()
        .downcast::<LibraryRow>()
        .ok()
        .or_else(|| focus.first_child().and_downcast::<LibraryRow>())
}

fn search_library(db: &LibraryDB, query: &str) -> db::Result<SearchResults> {
    let matcher = SkimMatcherV2::default().ignore_case();

    // Album artists and track artists without duplicates
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

/// The albums of an artist in the order the albums page shows them
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

/// The songs a context menu acts on in playing order
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

/// Loads the album's cover image file or, failing that, the cover art embedded
/// in one of its tracks. Blocking; call from a worker thread
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
