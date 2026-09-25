# The torrent plugin

The provider of Phase 2 §2.4. The decision record: **D-047**.

This directory is the plugin's *installed* form; its source is
`crates/headshell-plugin-torrent/`.

> **`TODO: AFTER FIRST RELEASE` — installation breaks D-049.**
> The installation below makes you install a **Rust toolchain**. D-049 requires
> that no plugin ask for a system-wide install, and after D-055 this is the
> only plugin that breaks it: the others are scripts that go through the
> engine's Python; this one is a binary, and it can't.
>
> An interim solution was "move it into the core as a provider behind a
> feature gate" (D-050 Q3); **D-056 cancelled it** — what would be torn out is
> 2,335 lines of source + 647 lines of tests, a working plugin. The question
> left open is **distribution**: a prebuilt release output per platform, or
> staying with "build from source". It will be decided after the first release
> (PLAN §2.8 item 5).

The plugin runs not inside the core but **as a subprocess** (K5) — the reason
was measured: `librqbit` added 179 crates to `headshell-core`'s dependency tree
(77 → 256), and that tree would have gone to mobile too, through `uniffi`. This
arrangement became **permanent** with D-056.

## Installation

```bash
cargo build --release -p headshell-plugin-torrent
mkdir -p ~/.local/share/headshell/plugins/torrent
cp plugins/torrent/plugin.json ~/.local/share/headshell/plugins/torrent/
cp target/release/headshell-plugin-torrent ~/.local/share/headshell/plugins/torrent/
headshell plugin approve torrent
```

The directory name is the ID (D-037): the directory must be `torrent`, exactly
the same as the `name` in `plugin.json`.

## Search needs Torznab

Search doesn't go to the indexers directly. We use the **Torznab** API that
Prowlarr or Jackett speak: one standard, one parser, and no site-specific
scraper in the repository. You choose which indexers are queried in your own
Prowlarr/Jackett; when a site breaks, what needs updating is not our code but
their indexer definition.

```bash
headshell secret set plugin:torrent torznab_url      # e.g. http://127.0.0.1:9696/1/api
headshell secret set plugin:torrent torznab_api_key
```

If it isn't configured, `search` returns **not an empty result but an explicit
error**: "I didn't look" and "I couldn't find it" are different diagnoses
(K9). Playing works in this case too — if you have an infohash or a magnet at
hand.

## Two-step search

Torznab returns a **release**, not a track — usually an album. The protocol's
`WireTrack`, however, is a track. Without growing api 1, the solution is two
steps:

**There is no CLI command that makes a provider search directly** — there is
no `search` subcommand under `headshell provider`. The plugin's search works
through `headshell play`: if the catalog has no results, the core asks every
provider that can stream (`Session::queue_from_search`), and torrent is one of
them.

Seeing the IDs needs `--dry-run --json`; the human-readable output only prints
`artist - title`, and a release's infohash **doesn't show** there:

```bash
# step 1 — releases. ID = <infohash>, and it shows only in the JSON.
headshell play "radiohead ok computer" --dry-run --json | jq -r '.queued[].id.id'

# step 2 — the audio files inside that release. ID = <infohash>/<index>
headshell play <infohash> --dry-run --json | jq -r '.queued[].id.id'

# step 3 — play.
headshell play <infohash>/3
```

> That these two steps are stuck with `--json` is not the plugin's gap but
> **the CLI's**: `play --dry-run` doesn't write the provider track ID in the
> human output. It stands as an open debt in PLAN.md §2.6.

You can search for a magnet link directly; the plugin writes it into its
catalog and lists the files inside.

In a release with a single audio file, a bare `<infohash>` plays directly. If
there are several files, the plugin **doesn't guess**: it returns an error
that lists the files and tells you what to type — you can see step 2 through
this error without `--json` too.

## How the audio arrives

The download isn't expected to finish. `librqbit` sets the piece priority
according to the read position; the plugin serves that stream from a small HTTP
server bound only to `127.0.0.1`, and `resolve_source` returns that address.

This isn't what K3 forbids: no data is relayed; the stream is read on the
user's own machine from data they fetched themselves. The address's path
carries a random token that lives as long as the process — so that another
process on the same machine can't read the downloads by trying addresses.

## The permission declaration is incomplete, on purpose

`plugin.json` declares only two DHT entry points. A torrent client by
definition connects to trackers and random peer addresses that **can't be known
in advance**; it also goes to the Torznab address you gave for search. D-040's
permission vocabulary ("a list of host names, no `*`") can't express this.
Leaving the declaration incomplete and saying so in the `description` is more
honest than pretending a restriction exists that doesn't. Widening the
vocabulary is an open topic (D-047).

## Settings

| Variable | What it does |
|---|---|
| `HEADSHELL_TORRENT_LOG` | The `tracing` filter (default `info`). The log goes to stderr, and the core carries it into `headshell diag`. |

Downloads land under `<plugin data directory>/downloads/<infohash>/`, every
torrent in its own directory (PLAN §2.4: two releases carrying the same file
name is common, and overwriting is silent data loss).
