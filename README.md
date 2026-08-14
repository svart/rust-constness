# rust-constness

![Rust standard-library method constness chart](assets/rust-standard-library-method-constness.png)

This project collects the number of stable public methods and associated
functions in Rust's standard-library rustdoc artifacts, then draws the totals
and the const-capable subset as two Plotly lines in an HTML file.

The collector downloads only the version-matched `rust-docs` archive from the
official Rust distribution server. It uses release manifests where available
and the archive's published checksum sidecar for Rust 1.0 through 1.7. It does
not install Rust toolchains. The extracted HTML tree is cached under
`target/rust-docs-cache/` and read at:

```text
<cache>/<release>/share/doc/rust/html/std/
```

The collector streams HTTPS downloads, verifies the published SHA-256 hash, and
extracts the archive only after verification. Version retrievals run in
parallel (5 by default), with one progress bar per release.

The parser counts methods declared by inherent implementations and public
traits. Methods from trait implementations, blanket implementations, and
synthetic implementations are excluded so the same API method is not counted
multiple times. A method contributes to the const-capable line only when it is
stable and has stable const support in that release.

Rustdoc's HTML has changed several times since Rust 1.0. The parser handles the
historical method heading tags, stability markers, item metadata, sidebar
layouts, and const-signature formats. For releases before `std/all.html` was
introduced, it discovers item pages directly from the extracted documentation
tree.

## Usage

The Rust release catalogue is checked into `src/versions.rs`. Pass one exact
release or an inclusive release range with `--version`:

```text
--version 1.97.1
--version 1.50-1.97
```

Collect data and generate the chart in one command:

```bash
cargo run --release -- generate \
  --version 1.50-1.97 \
  --parallel 5 \
  --output target/rust-constness.html
```

Or run collection and rendering separately:

```bash
cargo run --release -- collect --version 1.50-1.97
cargo run --release -- chart \
  --output target/rust-constness.html
```

Use `--parallel N` to change the number of concurrent downloads. It must be a
positive integer.

Collected data is always written to `data/methods.json`.

The generated HTML embeds Plotly.js, so it can be opened directly without a
separate web server.

## License

Licensed under the [MIT License](LICENSE).
