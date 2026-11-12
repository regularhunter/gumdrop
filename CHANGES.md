# Changes

All notable changes to this project will be documented in this file.

## 0.3.1 - 2026-11-12

- Change 'Queue Track' UI label to 'Queue Song'
- Properly style currently playing track when opening playlist containing it
- If the clicked song is already queued, jump to it instead of re-queuing

## 0.3.0 - 2026-11-09

- m3u playlist support
- Distinguish albums by the same artist with the same name but different year
- Various text formatting changes in library sidebar
- Currently playing track in library sidebar is now bold
- Show empty library page when clearing library and artist album page is open
- Adjust large cover art window sizing threshold
- Add support for webp cover art
- Display bitrate info for lossy tracks
- Change 'Add Folder/Song' UI labels to 'Queue Folder/Track'

## 0.2.0 - 2026-11-06

- Add rescan library shortcut (Ctrl+R)
- Find new tracks during rescan, not just removed tracks
- Optimize library scan by only computing album artists once
- Combine 'Various Artists' entries in library
- Sort 'Various Artists' and 'Unknown Artists' alphabetically
- Share cover art cache between sidebar views (artists, albums, queue)
- Display audio format information (File format, Bit-depth, Sample rate)
- Display larger cover art
- Add MPRIS volume control support
- Replace deprecated dark-style.css with media queries

## 0.1.2 - 2026-11-02

- Implement sqlite database backend for persistent music libraries
- Add UI elements to browse new persistent library database

## 0.1.1 - 2025-02-27

- Rename app-id

## 0.1.0 - 2025-02-25

- Initial 0.1.0 of Gumdrop forked from Amberol 2024.3-d711177c
