# TODO: write

## Examples

### `download_random_phantom`

Downloads the public phantom registry, picks a random phantom from a random
collection, downloads it (and the NIfTI files it references) into a local
`cache/` folder, then loads it with `Phantom::load`. Prints `tracing` spans
for the slow steps (downloading, NIfTI loading, mapping-function evaluation),
so it requires the `tracing` feature:

```sh
cargo run --example download_random_phantom --features tracing
```

This prints span timings to the console and also writes `trace.json`
(Chrome Trace Event format) — open it at https://ui.perfetto.dev for a
timeline view of the same spans.

## Features

- `tracing`: instruments the slow code paths (downloading, NIfTI loading,
  mapping-function evaluation) with `tracing` spans. Off by default so the
  library doesn't pull in `tracing` unless you want it.
