// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};

use adw::subclass::prelude::*;
use gtk::{gdk, gio, glib, prelude::*, CompositeTemplate};

use crate::{
    cover_picture::CoverPicture,
    library_item::{LibraryItem, LibraryItemKind},
};

mod imp {
    use glib::{ParamSpec, ParamSpecEnum, ParamSpecObject, ParamSpecString, Value};
    use once_cell::sync::Lazy;

    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/org/gnome/Gumdrop/library-row.ui")]
    pub struct LibraryRow {
        // Template widgets
        #[template_child]
        pub number_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub cover_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub cover_image: TemplateChild<CoverPicture>,
        #[template_child]
        pub title_label: TemplateChild<gtk::Inscription>,
        #[template_child]
        pub subtitle_label: TemplateChild<gtk::Inscription>,
        #[template_child]
        pub duration_label: TemplateChild<gtk::Label>,

        pub item: RefCell<Option<LibraryItem>>,
        pub kind: Cell<LibraryItemKind>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibraryRow {
        const NAME: &'static str = "GumdropLibraryRow";
        type Type = super::LibraryRow;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            CoverPicture::static_type();

            Self::bind_template(klass);

            klass.set_layout_manager_type::<gtk::BoxLayout>();
            klass.set_css_name("libraryrow");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for LibraryRow {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.obj().update_mode();
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecObject::builder::<LibraryItem>("item").build(),
                    ParamSpecEnum::builder::<LibraryItemKind>("kind").build(),
                    ParamSpecString::builder("title").build(),
                    ParamSpecString::builder("subtitle").build(),
                    ParamSpecString::builder("number").build(),
                    ParamSpecString::builder("duration").build(),
                    ParamSpecObject::builder::<gdk::Texture>("cover").build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "item" => {
                    let item = value.get::<Option<LibraryItem>>().unwrap();
                    self.item.replace(item);
                }
                "kind" => {
                    let p = value
                        .get::<LibraryItemKind>()
                        .expect("The value needs to be a LibraryItemKind");
                    self.obj().set_kind(p);
                }
                "title" => {
                    let p = value.get::<Option<&str>>().unwrap();
                    self.obj().set_title(p.unwrap_or_default());
                }
                "subtitle" => {
                    let p = value.get::<Option<&str>>().unwrap();
                    self.obj().set_subtitle(p.unwrap_or_default());
                }
                "number" => {
                    let p = value.get::<Option<&str>>().unwrap();
                    self.obj().set_number(p);
                }
                "duration" => {
                    let p = value.get::<Option<&str>>().unwrap();
                    self.obj().set_duration(p);
                }
                "cover" => {
                    let p = value.get::<gdk::Texture>().ok();
                    self.obj().set_cover(p);
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "item" => self.item.borrow().to_value(),
                "kind" => self.kind.get().to_value(),
                "title" => self.title_label.text().to_value(),
                "subtitle" => self.subtitle_label.text().to_value(),
                "number" => self
                    .number_label
                    .is_visible()
                    .then(|| self.number_label.text())
                    .to_value(),
                "duration" => self
                    .duration_label
                    .is_visible()
                    .then(|| self.duration_label.text())
                    .to_value(),
                "cover" => self.cover_image.cover().to_value(),
                _ => unimplemented!(),
            }
        }
    }

    impl WidgetImpl for LibraryRow {}
}

glib::wrapper! {
    pub struct LibraryRow(ObjectSubclass<imp::LibraryRow>)
        @extends gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible,
                    gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for LibraryRow {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl LibraryRow {
    pub fn new() -> Self {
        Self::default()
    }

    fn set_kind(&self, kind: LibraryItemKind) {
        let old_kind = self.imp().kind.replace(kind);
        if kind != old_kind {
            self.remove_css_class(old_kind.as_ref());
            self.update_mode();
            self.notify("kind");
        }
    }

    fn update_mode(&self) {
        let imp = self.imp();
        let kind = imp.kind.get();

        self.add_css_class(kind.as_ref());
        imp.cover_stack.set_visible(kind == LibraryItemKind::Album);
    }

    fn set_title(&self, title: &str) {
        self.imp().title_label.set_text(Some(title));
    }

    fn set_subtitle(&self, subtitle: &str) {
        let imp = self.imp();
        imp.subtitle_label.set_text(Some(subtitle));
        imp.subtitle_label.set_visible(!subtitle.is_empty());
    }

    fn set_number(&self, number: Option<&str>) {
        let imp = self.imp();
        imp.number_label.set_label(number.unwrap_or_default());
        imp.number_label.set_visible(number.is_some());
    }

    fn set_duration(&self, duration: Option<&str>) {
        let imp = self.imp();
        imp.duration_label.set_label(duration.unwrap_or_default());
        imp.duration_label.set_visible(duration.is_some());
    }

    fn set_cover(&self, cover: Option<gdk::Texture>) {
        let imp = self.imp();
        if let Some(texture) = cover {
            imp.cover_image.set_cover(Some(&texture));
            imp.cover_stack.set_visible_child_name("cover");
        } else {
            imp.cover_image.set_cover(None);
            imp.cover_stack.set_visible_child_name("no-cover");
        }
    }

    pub fn item(&self) -> Option<LibraryItem> {
        self.imp().item.borrow().clone()
    }
}
