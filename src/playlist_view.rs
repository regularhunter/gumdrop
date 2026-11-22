// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};
use adw::subclass::prelude::*;
use glib::clone;
use gtk::{gdk, gio, glib, graphene, prelude::*, CompositeTemplate};
use crate::{audio::Song, queue_row::QueueRow};

type RemoveHandler = Rc<dyn Fn(Song)>;

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/org/gnome/Gumdrop/queue-view.ui")]
    pub struct QueueView {
        #[template_child]
        pub back_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub queue_list_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub queue_length_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub queue_actionbar: TemplateChild<gtk::ActionBar>,
        #[template_child]
        pub queue_select_all_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub queue_remove_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub queue_selected_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub queue_progress: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub queue_searchbar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub queue_searchentry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub context_menu: TemplateChild<gtk::PopoverMenu>,
        pub remove_handler: RefCell<Option<RemoveHandler>>,
        pub menu_target: RefCell<Option<Song>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for QueueView {
        const NAME: &'static str = "GumdropQueueView";
        type Type = super::QueueView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_accessible_role(gtk::AccessibleRole::Group);

            klass.install_action("queue-view.remove-from-queue", None, |this, _, _| {
                this.remove_menu_target();
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for QueueView {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.obj().setup_searchbar();
            self.obj().add_menu_shortcut();
        }
    }

    impl WidgetImpl for QueueView {}
}

glib::wrapper! {
    pub struct QueueView(ObjectSubclass<imp::QueueView>)
        @extends gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible,
                    gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for QueueView {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl QueueView {
    pub fn new() -> Self {
        Self::default()
    }

    fn setup_searchbar(&self) {
        let entry = self.imp().queue_searchentry.get();
        self.imp().queue_searchbar.connect_entry(&entry);
    }

    pub fn back_button(&self) -> gtk::Button {
        self.imp().back_button.get()
    }

    pub fn queue_actionbar(&self) -> gtk::ActionBar {
        self.imp().queue_actionbar.get()
    }

    pub fn queue_remove_button(&self) -> gtk::Button {
        self.imp().queue_remove_button.get()
    }

    pub fn queue_select_all_button(&self) -> gtk::Button {
        self.imp().queue_select_all_button.get()
    }

    pub fn queue_selected_label(&self) -> gtk::Label {
        self.imp().queue_selected_label.get()
    }

    pub fn queue_list_view(&self) -> gtk::ListView {
        self.imp().queue_list_view.get()
    }

    pub fn queue_length_label(&self) -> gtk::Label {
        self.imp().queue_length_label.get()
    }

    pub fn queue_searchbar(&self) -> gtk::SearchBar {
        self.imp().queue_searchbar.get()
    }

    pub fn queue_searchentry(&self) -> gtk::SearchEntry {
        self.imp().queue_searchentry.get()
    }

    pub fn begin_loading(&self) {
        self.imp().queue_progress.set_fraction(0.0);
        self.imp().queue_progress.set_visible(true);
    }

    pub fn end_loading(&self) {
        self.imp().queue_progress.set_visible(false);
    }

    pub fn update_loading(&self, cur: u32, max: u32) {
        let step = cur as f64 / max as f64;
        self.imp().queue_progress.set_fraction(step);
    }

    pub fn set_search(&self, search: bool) {
        self.imp().queue_searchbar.set_search_mode(search);
    }

    pub fn set_remove_handler<F: Fn(Song) + 'static>(&self, handler: F) {
        self.imp().remove_handler.replace(Some(Rc::new(handler)));
    }

    pub fn add_context_menu(&self, row: &QueueRow) {
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
                this.popup_menu(&row, Some((x, y)));
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
                this.popup_menu(&row, Some((x, y)));
            }
        ));
        row.add_controller(long_press);
    }

    fn add_menu_shortcut(&self) {
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
                    this.popup_menu(&row, None);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        ));

        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
        self.imp().queue_list_view.add_controller(shortcuts);
    }

    fn popup_menu(&self, row: &QueueRow, point: Option<(f64, f64)>) {
        let song = match row.property::<Option<Song>>("song") {
            Some(song) => song,
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
        imp.menu_target.replace(Some(song));
        imp.context_menu.set_pointing_to(rect.as_ref());
        imp.context_menu.popup();
    }

    fn remove_menu_target(&self) {
        let song = match self.imp().menu_target.take() {
            Some(song) => song,
            None => return,
        };

        let handler = self.imp().remove_handler.borrow().clone();
        if let Some(handler) = handler {
            handler(song);
        }
    }
}

fn focused_row(view: &gtk::Widget) -> Option<QueueRow> {
    let focus = view.root()?.focus()?;
    if !focus.is_ancestor(view) {
        return None;
    }

    focus
        .clone()
        .downcast::<QueueRow>()
        .ok()
        .or_else(|| focus.first_child().and_downcast::<QueueRow>())
}
