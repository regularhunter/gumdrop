Gumdrop
=======

<img src="https://raw.githubusercontent.com/regularhunter/gumdrop/304b07872c81d686119a56a60a095de7d71ddc8e/data/icons/hicolor/scalable/apps/org.gnome.Gumdrop.svg">

Gtk4 music player written in rust. Forked from [Amberol](https://gitlab.gnome.org/World/amberol).


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
- ninja
- rust
- cargo
- gtk4
- libadwaita-1
- libadwaita-devel
- gstreamer-1.0
```

Recommended
```
- gnome-builder
```

How to obtain debugging information
-----------------------------------

Run Gumdrop from your terminal using:

    RUST_BACKTRACE=1 RUST_LOG=gumdrop=debug flatpak run org.gnome.Gumdrop

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
