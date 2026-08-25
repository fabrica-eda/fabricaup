# fabricaup

Fabrica EDA のツールチェーンマネージャーです。Fabrica の各 GitHub Releases から
OS/CPU に合うツールを取得し、SHA-256 を検証して、ツールごとに複数バージョンを
切り替えます。既定ツールは Texo です。

## インストール

Linux / macOS:

```sh
curl --proto '=https' --tlsv1.2 -sSf \
  https://raw.githubusercontent.com/fabrica-eda/fabricaup/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/fabrica-eda/fabricaup/main/install.ps1 | iex
```

インストーラーは `fabricaup` を `~/.fabrica/bin` に置き、PATH を設定し、最新の
`texo` も導入します。マネージャーだけ導入したい場合は
`FABRICAUP_INIT_SKIP=1`、PATH を変更したくない場合は
`FABRICAUP_NO_MODIFY_PATH=1` を設定してください。

## 使い方

```text
fabricaup install              # 最新の texo を導入してデフォルトにする
fabricaup install v0.1.0       # 指定した texo リリースを導入する
fabricaup update               # 最新版へ更新する
fabricaup list                 # 導入済みバージョンを表示する
fabricaup default v0.1.0       # アクティブ版を切り替える
fabricaup which                # アクティブな texo の場所を表示する
fabricaup uninstall v0.1.0     # 非アクティブ版を削除する

fabricaup install --tool struo             # 別ツールの最新版を導入する
fabricaup install v0.2.0 --tool struo      # 別ツールの指定版を導入する
fabricaup list --tool struo                # Struo だけを表示する
fabricaup which --tool struo               # Struo の実行ファイルを表示する
```

`--tool <name>` を指定すると、既定では `fabrica-eda/<name>` の GitHub Releases、
`<name>-<target>.tar.gz`、`<name>-<target>.sha256`、アーカイブ内の `<name>` 実行
ファイルを参照します。フォークや異なる配布先は `--repo owner/name` または
`FABRICA_DIST_REPO=owner/name` で変更できます。既定ツールは `FABRICAUP_TOOL`、
保存先は `FABRICAUP_HOME` でも変更できます。

各ツールの導入済みバージョンとデフォルトは独立しています。たとえば Texo の
バージョンを切り替えても、導入済みの Struo は `~/.fabrica/bin` に残ります。

## 開発

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

CI は通常のテストに加えて、Linux、macOS、Windows で公開中の最新 Texo release を
実際に取得し、checksum 検証、`texo --version`、`which`、`list` まで実行します。

`v*` タグを push すると、Release workflow が Linux x86_64/aarch64、macOS
x86_64/Apple Silicon、Windows x86_64 向けの `fabricaup` と checksum を GitHub
Release に公開します。Texo 側が用意する配布物の契約は
[docs/distribution.md](docs/distribution.md) を参照してください。
