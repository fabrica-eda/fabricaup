# Fabrica tool distribution contract

`fabricaup --tool <tool>` uses the `fabrica-eda/<tool>` GitHub Release API by
default. All managed Fabrica tools follow the asset naming convention already
used by published Texo releases:

```text
<tool>-<target>.tar.gz
<tool>-<target>.zip
<tool>-<target>.sha256
```

For example, a Linux x86_64 Texo release provides these two files. The release
tag is not included in the asset name; the GitHub Release tag itself, such as
`v0.1.0`, selects the version.

```text
texo-x86_64-unknown-linux-gnu.tar.gz
texo-x86_64-unknown-linux-gnu.sha256
```

Targets recognized by fabricaup:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

As in current Texo releases, an archive may place the executable directly in a
top-level package directory:

```text
texo-x86_64-unknown-linux-gnu/
|-- texo
`-- README.md
```

A `bin/<tool>` layout is also accepted. Fabricaup extracts only `<tool>` or
`<tool>.exe` on Windows and stores it under
`~/.fabrica/toolchains/<tool>/<tag>/bin/`.

The checksum file uses the standard `sha256sum` format:

```text
<64-character SHA-256>  texo-x86_64-unknown-linux-gnu.tar.gz
```

`fabricaup install` discovers the highest stable `vMAJOR.MINOR.PATCH` release
that contains both the archive and checksum for the current platform. A tool
repository may also publish database bundles or other auxiliary GitHub Releases;
those releases are ignored. Drafts, prereleases, and releases missing either
required platform asset are also excluded from `latest` discovery. Users may
still request an exact release tag explicitly.

New tools require no fabricaup code changes. If `fabrica-eda/struo` publishes
assets that follow this contract, users can install it with:

```sh
fabricaup install --tool struo
```

Smoke-test a release from a fork with an isolated installation root:

```sh
FABRICAUP_HOME="$(mktemp -d)" \
FABRICA_DIST_REPO="your-org/texo" \
fabricaup install v0.1.0 --tool texo
texo --version
```
