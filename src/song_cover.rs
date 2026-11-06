// SPDX-FileCopyrightText: 2023  Emmanuele Bassi
// SPDX-FileCopyrightText: 2025  Hunter Wardlaw
// SPDX-License-Identifier: GPL-3.0-or-later

use adw::subclass::prelude::*;
use gtk::{glib, prelude::*, CompositeTemplate};

use crate::cover_picture::{CoverPicture, CoverSize};

mod imp {
    use glib::{ParamSpec, ParamSpecEnum, Value};
    use once_cell::sync::Lazy;
    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/org/gnome/Gumdrop/song-cover.ui")]
    pub struct SongCover {
        // Template widgets
        #[template_child]
        pub cover_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub album_image: TemplateChild<CoverPicture>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SongCover {
        const NAME: &'static str = "GumdropSongCover";
        type Type = super::SongCover;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_css_name("songcover");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            CoverPicture::static_type();
            obj.init_template();
        }
    }

    impl ObjectImpl for SongCover {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![ParamSpecEnum::builder::<CoverSize>("cover-size")
                    .explicit_notify()
                    .build()]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "cover-size" => self
                    .obj()
                    .set_cover_size(value.get::<CoverSize>().expect("Required CoverSize")),
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "cover-size" => self.album_image.cover_size().to_value(),
                _ => unimplemented!(),
            }
        }
    }

    impl WidgetImpl for SongCover {}
}

glib::wrapper! {
    pub struct SongCover(ObjectSubclass<imp::SongCover>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for SongCover {
    fn default() -> Self {
        glib::Object::new::<Self>()
    }
}

impl SongCover {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn album_image(&self) -> CoverPicture {
        self.imp().album_image.get()
    }

    pub fn set_cover_size(&self, cover_size: CoverSize) {
        let picture = self.imp().album_image.get();
        if picture.cover_size() != cover_size {
            picture.set_cover_size(cover_size);
            self.notify("cover-size");
        }
    }

    pub fn show_cover_image(&self, has_image: bool) {
        let cover_stack = self.imp().cover_stack.get();
        if has_image {
            cover_stack.set_visible_child_name("cover-image");
        } else {
            cover_stack.set_visible_child_name("no-image");
        }
    }
}
