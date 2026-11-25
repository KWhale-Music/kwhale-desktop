<div align="center">

**English** | [Русский](README.ru.md)

<br/>

<img src="docs/images/cover.png" alt="KWhale Desktop" width="100%"/>

# KWhale Desktop

**Native desktop client for KWhale — one adaptive Material You UI for Linux and Windows.**

[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.8x-DEA584.svg)](https://www.rust-lang.org/)
[![Slint](https://img.shields.io/badge/Slint-Material%20You-2379C4.svg)](https://slint.dev/)
[![Flatpak](https://img.shields.io/badge/Flatpak-ready-4A90D9.svg)](#-build--install)

</div>

Cross-platform desktop client for the [KWhale Server](https://github.com/KWhale-Music/kwhale-server),
built on a single Rust core with a **Material You 3** interface (Slint) that adapts to
GNOME, KDE Plasma, neutral window managers and Windows. GStreamer playback with a local
Range/caching proxy, MPRIS2 integration, synced lyrics panel, and `kwhale://` deep links.

## ✨ Features

- **Adaptive Material You 3 UI** — tonal surfaces, 129 accents plus the system accent of
  your desktop, metrics adapted for GNOME / KDE / Windows.
- **Serious audio stack** — GStreamer `playbin3`, localhost Range/caching proxy,
  prefetcher (5 tracks ahead), gapless-friendly buffering.
- **Desktop integration** — MPRIS2 (media keys, players applets), single-instance via
  D-Bus activation, `kwhale://track/{id}` deep links, launcher that forwards links to the
  running instance.
- **Player UX** — Now Playing fullscreen (F11) with blurred album art and karaoke-style
  lyrics, synchronized lyrics panel (Ctrl+L), queue panel (Ctrl+Q).
- **Same `/v1` API** — single-flight token refresh, caches, reactions, downloads;
  identical semantics to the Android client.

## 📸 Screenshots

<img src="docs/images/screens-app-window.png" alt="Main window" width="100%"/>

<img src="docs/images/screens-app-lyrics.png" alt="Lyrics panel" width="100%"/>

## 🚀 Build & install (Flatpak)

Requires `org.flatpak.Builder`, `org.kde.Platform//6.11`, `org.kde.Sdk//6.11` and
`org.freedesktop.Sdk.Extension.rust-stable//25.08`:

```bash
git clone https://github.com/KWhale-Music/kwhale-desktop.git
cd kwhale-desktop
./packaging/flatpak/build-flatpak.sh
```

Prebuilt `.flatpak` bundles are published in [GitHub Releases](https://github.com/KWhale-Music/kwhale-desktop/releases).

The script vendors dependencies (`cargo vendor --locked`), builds `kwhale` and
`kwhale-ui` on the KDE runtime, exports a single-file bundle `target/kwhale-desktop.flatpak`
and installs it into your user profile.

```bash
flatpak run org.dueattendant149.Kwhale                  # launch
flatpak run org.dueattendant149.Kwhale kwhale://track/42  # open a deep link
```

## ⌨️ Hotkeys

| Key | Action |
|---|---|
| `Space` | Play / pause |
| `Ctrl+←` / `Ctrl+→` | Previous / next track |
| `Shift+←` / `Shift+→` | Seek −5s / +5s |
| `Ctrl+↑` / `Ctrl+↓` | Volume ±5% |
| `M` | Mute |
| `Ctrl+F` / `Ctrl+K` | Focus search |
| `Ctrl+L` | Synchronized lyrics panel |
| `Ctrl+Q` | Queue panel |
| `F11` | Fullscreen Now Playing |

## 🏗 Structure

```
crates/
├── kwhale-core/      # REST /v1 client, single-flight refresh, caches, 129 accents + PlatformStyle
├── kwhale-audio/     # GStreamer playbin3, localhost Range/cache proxy, prefetcher, MPRIS2
├── kwhale-ui/        # the adaptive Material You 3 Slint interface
└── kwhale-launcher/  # entry point + D-Bus activation
packaging/flatpak/    # manifest, desktop file, metainfo, build script
```

## 🧭 Author's review

*Honest notes from the author — trade-offs and open rough edges.*

What I'm genuinely happy with:

- **The audio layer.** A localhost Range/caching proxy plus a 5-track prefetcher is the
  kind of boring infrastructure that makes playback feel instant and survive flaky networks.
- **One UI across desktops.** The same Slint interface adapts to GNOME, KDE and Windows
  with per-platform metrics instead of forking three toolkits.
- **Single-instance + D-Bus deep links.** `kwhale://track/{id}` just works, from the
  terminal or a browser.

What's honestly still rough:

- **UI coverage is partial.** The Slint interface is a growing vertical slice — Home and
  the mini-player are solid, some screens are still stubs compared to the Android client.
- **Windows polish lags Linux.** It runs, but the Windows theming pass is shallow.
- **Cache tuning is manual.** Prefetch depth and cache budget are constants, not settings — yet.

Next up: finishing the remaining screens (playlists, settings), a Windows theming pass,
and moving cache knobs into preferences.

## 🗺 Status

Beta — Linux daily-driver quality for core playback flows; UI still expanding.

## 📄 License

[AGPL-3.0](LICENSE) — same terms as the server.

Third-party notes: the UI references VK X's visual language and ships VK Sans fonts,
which remain the property of their owners — treat them as replaceable design assets.
