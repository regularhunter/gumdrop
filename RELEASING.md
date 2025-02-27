Releasing Gumdrop
=================

QA Plan
-------

1. Initial state
  - Open folder
  - Open file
  - Drag and drop
2. Main view
  - Play/pause
  - Previous/next
  - Scrubbing
3. Playlist
  - Shuffle
  - Repeat: all, one, continuous
  - Select
  - Remove song
  - Remove all
  - Toggle playlist

Release
-------

Checklist for a release.

- [ ] Update the [change log](./CHANGES.md)
  - [ ] **Added**: New features, settings, UI, translations
  - [ ] **Changed**: UI updates, improvements, translation updates
  - [ ] **Fixed**: bug fixes, with reference
  - [ ] **Removed**: Removed features, settings, UI; **IMPORTANT**: anything
    inside this list requires a version bump
- [ ] Update the [appdata](./data/com.github.regularhunter.Gumdrop.metainfo.xml.in)
  - [ ] New `<release>` element
  - [ ] *Optional*: new screenshots
- [ ] `git commit -m 'Release Gumdrop $VERSION'`
- [ ] `meson dist`
- [ ] `git tag -s $VERSION` (use the change log entry)
- [ ] Bump up the project version in [`meson.build`](./meson.build)
- [ ] `git push origin HEAD && git push origin $VERSION`
- [ ] Create a new release on [GitHub](https://github.com/regularhunter/gumdrop/releases/)
  - [ ] Copy the `CHANGES.md` entry
  - [ ] Attach the release tarball and the SHA256 checksum files

Flathub
-------

- [ ] Update the `com.github.regularhunter.Gumdrop.json` manifest
  - [ ] Change the archive URL
  - [ ] Change the SHA256 checksum
- [ ] `git push origin HEAD`
