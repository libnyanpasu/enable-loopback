# enable-loopback

A Windows AppContainer loopback exemption tool for clash-nyanpasu, built with
Rust and WinUI 3. It provides a localized table, multi-selection, sorting, copy, and exemption editing.

On Windows x64 or arm64, install the nightly Rust MSVC toolchain and Visual
Studio C++ build tools (including the Windows SDK). The GUI requires Windows
App Runtime **2.5.1.0 or later**, matching the process architecture:

Download the official **2.5.1** installer for [x64](https://aka.ms/windowsappsdk/2.5/2.5.1/windowsappruntimeinstall-x64.exe)
or [arm64](https://aka.ms/windowsappsdk/2.5/2.5.1/windowsappruntimeinstall-arm64.exe).
See also the [Windows App SDK downloads](https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads).

Build and run in Git Bash:

```sh
git clone --recursive https://github.com/libnyanpasu/enable-loopback.git
cd enable-loopback
# For an existing checkout:
git submodule update --init
export CARGO_INCREMENTAL=0
cargo build
cargo run -- --lang zh-cn
```

If a global Cargo configuration uses a compiler wrapper, set `RUSTC_WRAPPER=`
to bypass it for these commands. Do not change the global configuration.

`enableLoopback.exe [--lang CODE]` opens the GUI. Supported languages:
`en`, `zh-cn`, `zh-tw`, `ja`, `ko`, `ru`; otherwise the system language is used,
with English as the fallback. `manual` skips automatic refresh and `-registry`
selects registry enumeration.

CLI commands: `-all`, `-none`, `-allow NAME...`, `-deny NAME...`, and `?` for
localized syntax. Commands ignore case; the leading `-` or `/` is optional.
Names may contain spaces; `*` matches all. CLI dispatch does not initialize
WinAppSDK or check its runtime. Exemption commands apply the requested configuration; syntax errors exit `-10`.

The executable requests the highest available privilege level. Read the
[design specification](docs/design.md) for the planned behavior and architecture.

Run tests with `cargo test`; only the GUI executable embeds the elevation manifest.

The vendored windows-rs fork is a separate workspace. Check application formatting
with `cargo fmt -p enable-loopback -- --check`; `cargo fmt --all` recursively visits
the fork too and can exceed Windows' command-line limit.
