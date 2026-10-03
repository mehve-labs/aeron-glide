# Contributing to aeron-glide

Thanks for your interest in improving aeron-glide! Please read this document
before opening a pull request.

## Licensing of contributions

aeron-glide is licensed under the [Apache License 2.0](LICENSE). Contributions
follow the standard "inbound = outbound" model:

> Unless you explicitly state otherwise, any contribution you intentionally
> submit for inclusion in aeron-glide — as defined in Section 5 of the Apache
> License 2.0 — shall be licensed under the Apache License 2.0, without any
> additional terms or conditions.

You keep full ownership and copyright of your work; you are simply licensing it
to the project (and its users) under the same terms the project ships under.
Please only submit contributions that are your original work, or that you
otherwise have the right to submit under these terms. If your employer owns the
copyright, make sure you have permission before contributing.

## How to signal agreement

Add a `Signed-off-by` line to each commit (using `git commit -s`) to certify you
have the right to submit the contribution under the Apache License 2.0
(see the [Developer Certificate of Origin](https://developercertificate.org/)):

```
Signed-off-by: Your Name <your.email@example.com>
```

## Development

See [README.md](README.md) for build and test instructions.

Integration tests live in `tests/`. Each test starts its own media driver in a
unique directory through `tests/common::TestDriver`, so tests run in parallel;
wait with `common::wait_until` (it has a deadline) rather than open-ended loops.

### Binding conventions

- **Exceptions.** cxx aborts the process if a C++ exception escapes a bridged
  function that is not declared `-> Result<..>`. Declare every bridged function
  that can throw (directly, via the Aeron C++ wrapper, or via the checked
  `rust::String` constructor) as `-> Result<..>` and map it to `aeron_glide::Result` on the
  Rust side. Only functions that cannot throw may be bridged without `Result`;
  when a lookup's only failure means "not found", catch in `shim.cc` and return
  `nullptr` (surfaced as `Option`).
- **Strings from C++.** Use `rust::String::lossy`, never `rust::String(...)`,
  for data Aeron hands us (labels, channels, source identities): the checked
  constructor throws on invalid UTF-8.
- **Error kinds.** `rust::behavior::trycatch` in `shim.h` classifies Aeron
  exceptions into `ErrorKind`. Errors from the C API (e.g. the media driver)
  should go through `AERON_MAP_TO_SOURCED_EXCEPTION_AND_THROW` so they get a
  kind and an error code too.
- **C++ API only.** Bind what the official Aeron C++ wrapper exposes. Functions
  only the C API exposes are not bound unless a concrete use case needs them
  (see the C extras list in the roadmap); the media driver, which has no C++
  API, is the exception.
- **Generated code.** `src/driver_gen.rs` and `src/driver_gen.h` (media driver
  settings) are generated from Aeron's `aeronmd.h` by
  `scripts/gen_driver_context.py` and checked in. Don't edit them; rerun the
  script after building with a new Aeron version and review the diff. They
  match the default Aeron version in `build.rs`; building with a different
  `AERON_VERSION` may need them regenerated (and the archive header patch
  re-checked).
