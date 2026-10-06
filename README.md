# toyou

A lightweight YouTube Music desktop client built on [GPUI Kit](https://gpui-kit.com)
(Zed's GPUI + GPUI Component), inspired by [Zuno](https://github.com/noFAYZ/zuno).

- **Sign in with Google** in a small built-in browser window (WebKitGTK via wry).
  Alternatives: import the session from a browser you're signed into (needs yt-dlp),
  or paste a Cookie header. Guest mode works too.
- **Home**: your personalized suggestions (Quick picks, mixes, recommendations) when signed in.
- **Explore**: new releases, charts, moods & genres, trending.
- **Library**: your playlists and Liked music.
- **Search**: top result, songs, artists, albums & playlists.
- Album, playlist and artist pages with Play / Shuffle.
- Clicking a song starts a radio of suggestions; the queue keeps extending itself when it runs out.
- Up-next panel, seek and volume sliders, album art, dark theme.

Audio is played by toyou's own player: `yt-dlp` resolves each song to YouTube's AAC stream,
which is downloaded in chunks, decoded by [symphonia](https://github.com/pdeljanov/Symphonia)
and played through [rodio](https://github.com/RustAudio/rodio)/cpal. No mpv needed.

## Requirements

Runtime: a recent `yt-dlp` on `PATH` (distro packages are often too old to play).

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

Live API tests: `cargo test -- --ignored --nocapture`.
