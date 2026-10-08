# toyou

A lightweight YouTube Music desktop client built on [GPUI Kit](https://gpui-kit.com)
(Zed's GPUI + GPUI Component), inspired by [Zuno](https://github.com/noFAYZ/zuno).

- **Sign in with Google** in a small built-in browser window (WebKitGTK via wry).
  Alternatively paste a Cookie header. Guest mode works too.
- **Home**: your personalized suggestions (Quick picks, mixes, recommendations) when signed in.
- **Explore**: new releases, charts, moods & genres, trending.
- **Library**: your playlists and Liked music.
- **Search**: top result, songs, artists, albums & playlists.
- Album, playlist and artist pages with Play / Shuffle.
- Clicking a song starts a radio of suggestions; the queue keeps extending itself when it runs out.
- Up-next panel, seek and volume sliders, album art, dark theme.

Audio is played by toyou's own player: each song is resolved to YouTube's AAC stream through
the InnerTube `player` endpoint, downloaded in chunks, decoded by
[symphonia](https://github.com/pdeljanov/Symphonia) and played through
[rodio](https://github.com/RustAudio/rodio)/cpal. No external programs (no mpv, no yt-dlp).

## Requirements

Build (Debian/Ubuntu):

```sh
sudo apt install libasound2-dev libxkbcommon-x11-dev libwebkit2gtk-4.1-dev libgtk-3-dev
cargo run --release
```

Only YouTube cookies are kept, in `~/.config/toyou/cookies.txt` (mode 600); signing out
deletes it. The sign-in window keeps its own browser profile in `~/.config/toyou/webview`
so Google recognizes it next time.
Your up-next queue, current song, position and volume are kept in `~/.config/toyou/state.json`,
so reopening toyou picks up where you left off (paused; press play to continue). The sign-in window is a separate program, `toyou-login`, built alongside `toyou`.

## Code layout

A Cargo workspace with one crate per domain under `crates/`:

| Crate | What it does |
| --- | --- |
| `toyou` | The app binary (plus `toyou-login`, the sign-in window) |
| `views` | `MusicApp` and every screen: `root`, `sidebar`, `pages`, `login`, `player_bar`, `now_playing`, `palette` |
| `state` | Plain state types: page loading, on-demand fetches, Now playing tabs, FPS meter |
| `router` | The pages toyou can show (`Route`) |
| `ui` | Shared UI pieces: theme palette, lazy album art (`ImageCache`), resize edges |
| `input` | Actions and key bindings |
| `icons` | Bundled icons (asset source) |
| `music` | YouTube Music (InnerTube) client: home, explore, search, pages, radio, lyrics |
| `player` | Audio engine: InnerTube stream → chunked download → symphonia → rodio |
| `auth` | Session cookies and Google sign-in validation |
| `storage` | Saves the queue, position and volume between runs |
| `webview` | The sign-in window (WebKitGTK via wry) |

`cargo run --release` builds and runs the app (`toyou-login` is built alongside it).
Live API/player tests: `cargo test --workspace -- --ignored --nocapture`.

## Keys

| Key          | Action          |
| ------------ | --------------- |
| Ctrl+P       | Search songs (palette) |
| Ctrl+Shift+P | Command palette |
| Ctrl+K       | Focus search    |
| Enter        | Search          |
| Ctrl+Space   | Play / pause    |
| Ctrl+.       | Next track      |
| Ctrl+,       | Previous track  |
| Alt+Left     | Back            |
| F12          | FPS counter     |

