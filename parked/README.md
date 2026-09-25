# parked/ — code that isn't built, but isn't deleted

The code in this directory is **outside the workspace**: it isn't built, isn't
tested and isn't in CI (`Cargo.toml` → `exclude`). The difference from
`spike/`: the code here isn't a throwaway prototype but a piece that worked and
is expected to come back.

## `crates/headshell-plugin-torrent/` + `plugins/torrent/`

The torrent provider (D-047): it searches Torznab, downloads sequentially with
`librqbit` and plays from a local address. **Parked with D-069.**

The reason: the plugin system moved from a subprocess + JSON-RPC (api 1) to an
embedded QuickJS (api 2). The torrent plugin was a Rust binary written for
api 1 and used the core's `plugin::protocol` types; when those types went away,
it stopped compiling. The user's decision: *"let's leave torrent aside for now;
it doesn't come with us. We'll look at it much later."*

The code stays as it was — 2,335 lines of source, 647 lines of tests (D-056's
measurement). Open questions for its return:

1. **Where will it run?** `librqbit` is a BitTorrent client: it opens sockets
   to arbitrary peers. The QuickJS engine's `host.http` can't express that.
   Three ways are in sight: as a **tool** the engine installs
   (`host.tools.run`, a prebuilt binary per platform — yt-dlp's way), as a
   provider behind a feature gate in the core (D-050 Q3's cancelled idea, +179
   crates), or as a separate class of provider.
2. **Distribution.** Whichever way is chosen, making the user run `cargo build`
   breaks D-049 — this debt existed before it was parked too.
3. **Permissions.** In api 2 the network permission is enforced; the addresses
   a torrent client will connect to can't be known in advance. The engine's
   permission vocabulary can't express that today, and it shouldn't — that is
   the reason to be a tool or a core provider.

The code isn't taken back into the workspace before a decision is made.
