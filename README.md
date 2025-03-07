Gumdrop
=======

<img src="data/icons/hicolor/scalable/apps/com.github.regularhunter.Gumdrop.svg">

Gtk4 music player written in rust. Forked from [Amberol](https://gitlab.gnome.org/World/amberol).

![Application screenshot](data/screenshots/gumdrop.png)


Why fork Amberol?
-----------------------------------

Amberol is an intentionally minimal application. Gumdrop's goals are of a larger scope including 
persistent music organization of large numbers of files and integration with online services.

------------

Please, see the [contribution guide](./CONTRIBUTING.md) if you wish to report
and issue, fix a bug, or implement a new feature.


Building
-----------------------------------

Dependencies
```
- meson
- rustc
- cargo
- gtk4 >= 4.16.0
- glib2
- libadwaita-1 >= 1.5
- libpng
- gstreamer-1.0 >= 1.20
```

Recommended
```
- gnome-builder
```

How to obtain debugging information
-----------------------------------

Run Gumdrop from your terminal using:

    RUST_BACKTRACE=1 RUST_LOG=gumdrop=debug flatpak run com.github.regularhunter.Gumdrop

to obtain a full debug log.

Code of Conduct
-----------------------------------

Gumdrop follows the GNOME project [Code of Conduct](./code-of-conduct.md). All
communications in project spaces such as the issue tracker are expected to follow it.

Why is it called "Gumdrop"?
-----------------------------------

The name is inspired by the [Lollypop](https://gitlab.gnome.org/World/lollypop) music player.

Copyright
-----------------------------------

© 2025 Hunter Wardlaw

Gumdrop is released under the terms of the GNU General Public License, either
version 3.0 or, at your option, any later version.
