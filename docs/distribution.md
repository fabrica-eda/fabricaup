# Fabrica ツール配布物の契約

`fabricaup --tool <tool>` は、既定で `fabrica-eda/<tool>` の GitHub Release API を
参照します。公開済み Texo release を基準に、すべての Fabrica ツールで次の asset
命名を使用します。

```text
<tool>-<target>.tar.gz
<tool>-<target>.zip
<tool>-<target>.sha256
```

たとえば Linux x86_64 では次の2ファイルです。Release tag は asset 名には含めず、
GitHub Release 自体の tag（例: `v0.1.0`）でバージョンを選択します。

```text
texo-x86_64-unknown-linux-gnu.tar.gz
texo-x86_64-unknown-linux-gnu.sha256
```

`fabricaup` が利用する target:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

アーカイブは、現在の Texo release のようにトップレベルディレクトリ直下へ
`<tool>` 実行ファイルを置けます。

```text
texo-x86_64-unknown-linux-gnu/
├── texo
└── README.md
```

`bin/<tool>` の形式も利用できます。`fabricaup` はアーカイブから `<tool>`
（Windows は `<tool>.exe`）だけを取り出し、選択した release の
`~/.fabrica/toolchains/<tool>/<tag>/bin/` に保存します。

checksum は Texo release が現在生成している sha256sum 形式です。

```text
<64文字のSHA-256>  texo-x86_64-unknown-linux-gnu.tar.gz
```

新しいツールはコード変更なしで追加できます。たとえば `fabrica-eda/struo` がこの
契約の release assets を公開すれば、次のコマンドで導入できます。

```sh
fabricaup install --tool struo
```

フォークした release のスモークテスト:

```sh
FABRICAUP_HOME="$(mktemp -d)" \
FABRICA_DIST_REPO="your-org/texo" \
fabricaup install v0.1.0 --tool texo
texo --version
```
