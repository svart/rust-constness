# rust-constness

Tracks the growth of const-capable APIs in Rust's standard library across
stable Rust releases.

![Rust standard-library method constness chart](assets/rust-standard-library-method-constness.png)

The chart compares all stable public methods and associated functions with the
subset that can be called in stable const contexts. The underlying observations
are available in [`data/methods.json`](data/methods.json).

## Usage

Generate data and an interactive chart for a Rust release:

```bash
cargo run --release -- generate --version 1.98.0
```

An inclusive range can be used to compare releases over time:

```bash
cargo run --release -- generate --version 1.50-1.98
```

The command writes the dataset to `data/methods.json` and the chart to
`target/rust-constness.html`. To render the checked-in dataset without
downloading documentation again, run:

```bash
cargo run --release -- chart
```

Downloaded documentation is cached under `target/rust-docs-cache`. To list the
cached Rust versions, the cache location, and its total size, run:

```bash
cargo run --release -- cache --status
```

## Adding a Rust release

Allowed release numbers are listed in
[`AVAILABLE_RELEASES`](src/versions.rs). To add the next stable release:

1. Append its full version number to `AVAILABLE_RELEASES`.
2. Regenerate the complete dataset and chart, where `NEW_RELEASE` is the
   version number appended in step 1:

   ```bash
   cargo run --release -- generate --version 1.0-NEW_RELEASE
   ```

3. Refresh `assets/rust-standard-library-method-constness.png` from the generated chart.

## Methodology

Data is collected from the official `rust-docs` archives published for each
release. The count includes stable methods declared by inherent implementations
and public traits. Trait implementation copies, blanket implementations, and
synthetic implementations are excluded to avoid counting the same API more than
once. An item that the standard library re-exports under several paths, such as
`collections::BTreeMap` and `collections::btree_map::BTreeMap`, is counted once.
A method declared in several implementation blocks, such as one block for each
integer type, is counted once for each block. A method is considered
const-capable only when its const support is stable in that release.

### Known limitations

- Const methods are counted from Rust 1.32. Earlier rustdoc shows `const fn`
  for const-unstable functions too, so it cannot separate the few methods that
  were const-stable before 1.32, such as `Cell::new` (const since 1.24).
- Rust 1.62.0 to 1.63.0 document `std::ffi::CStr`, `CString`, and their error
  types only as aliases of `core` and `alloc` pages, which are not part of the
  standard-library documentation that is read. About 28 methods are missing.
- In Rust 1.78, `NonZero::new`, `new_unchecked`, and `get` moved from one
  implementation block per integer type into one generic block. The count falls
  by 33 although the API did not change.
- Rust 1.0 to 1.3 count the methods of the unstable `BitSet`, `BitVec`, and
  `VecMap` types (about 51), because rustdoc marks only the types as unstable.
  An unstable type does not exclude its methods, because stable aliases, such as
  `NonZeroU8` for `NonZero<T>`, can still reach them. Unstable primitive types,
  such as `i128` before Rust 1.26, are excluded.
- `CStr::from_bytes_with_nul_unchecked` is counted as const in Rust 1.32 to
  1.46, and `NonZero*::get` in Rust 1.33. Their const support became stable in
  Rust 1.59 and 1.34.

## License

Licensed under the [MIT License](LICENSE).
