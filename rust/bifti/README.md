# TODO: write

## Examples

### `download_random_phantom`

Downloads the public phantom registry, picks a random phantom from a random
collection, downloads it (and the NIfTI files it references) into a local
`cache/` folder, then loads it with `Phantom::load`.

```sh
cargo run --example download_random_phantom
```
