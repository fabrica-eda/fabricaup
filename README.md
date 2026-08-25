# fabricaup

The toolchain manager for Fabrica EDA. It downloads platform-specific tools from
Fabrica GitHub Releases, verifies their SHA-256 checksums, and manages multiple
versions independently for each tool. Texo is the default tool.

## Installation

Linux and macOS:

```sh
curl --proto '=https' --tlsv1.2 -sSf \
  https://raw.githubusercontent.com/fabrica-eda/fabricaup/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/fabrica-eda/fabricaup/main/install.ps1 | iex
```

The installer places `fabricaup` in `~/.fabrica/bin`, adds that directory to
`PATH`, and installs the latest Texo release. Set `FABRICAUP_INIT_SKIP=1` to
install only the manager, or `FABRICAUP_NO_MODIFY_PATH=1` to leave `PATH`
unchanged.

## Usage

```text
fabricaup install                         # Install the latest Texo release
fabricaup install v0.1.0                  # Install a specific Texo release
fabricaup update                          # Update Texo to the latest release
fabricaup list                            # List installed Texo versions
fabricaup default v0.1.0                  # Select the active Texo version
fabricaup which                           # Print the active Texo path
fabricaup uninstall v0.1.0                # Remove an inactive Texo version
fabricaup self update                     # Update fabricaup itself

fabricaup install --tool struo            # Install another Fabrica tool
fabricaup install v0.2.0 --tool struo     # Install a specific tool release
fabricaup list --tool struo               # List only Struo versions
fabricaup which --tool struo              # Print the active Struo path
```

For `--tool <name>`, fabricaup defaults to the `fabrica-eda/<name>` GitHub
repository, `<name>-<target>.tar.gz`, `<name>-<target>.sha256`, and a `<name>`
executable inside the archive. Override the repository with `--repo owner/name`
or `FABRICA_DIST_REPO=owner/name`. The default tool can be changed with
`FABRICAUP_TOOL`, and the installation root with `FABRICAUP_HOME`.

For `latest`, fabricaup selects the highest stable `vMAJOR.MINOR.PATCH` release
that contains both required assets for the current platform. Auxiliary releases,
drafts, prereleases, and incomplete platform builds are ignored.

Installed versions and defaults are independent for every tool. Switching the
active Texo version, for example, does not remove an installed Struo executable
from `~/.fabrica/bin`.

`fabricaup self update` downloads the newest compatible fabricaup release,
verifies its SHA-256 checksum, and replaces the current executable in place. It
works for installations created by either installer script on every supported
platform. Updating a protected system-wide executable may require elevated
permissions.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

CI runs the regular checks and also downloads the latest published Texo release
on Linux, macOS, and Windows. It verifies the checksum and exercises
`texo --version`, `fabricaup which`, and `fabricaup list`.

Pushing a `v*` tag that matches the workspace package version runs the release
workflow. For example, workspace version `0.2.0` must be released as `v0.2.0`.
The workflow publishes `fabricaup` binaries and checksums for Linux
x86_64/aarch64, macOS x86_64/Apple Silicon, and Windows x86_64. See
[docs/distribution.md](docs/distribution.md) for the release contract used by
managed Fabrica tools.
