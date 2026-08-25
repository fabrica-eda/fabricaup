#!/bin/sh
set -eu

repo="${FABRICAUP_REPO:-fabrica-eda/fabricaup}"
fabrica_home="${FABRICAUP_HOME:-${HOME}/.fabrica}"
bin_dir="${fabrica_home}/bin"

need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "error: required command not found: $1" >&2
        exit 1
    }
}

need curl
need tar

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
    Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-gnu" ;;
    Darwin-x86_64) target="x86_64-apple-darwin" ;;
    Darwin-arm64|Darwin-aarch64) target="aarch64-apple-darwin" ;;
    *) echo "error: unsupported platform: $(uname -s)/$(uname -m)" >&2; exit 1 ;;
esac

asset="fabricaup-${target}.tar.gz"
base_url="https://github.com/${repo}/releases/latest/download"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT HUP INT TERM

echo "downloading fabricaup for ${target}"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
    "${base_url}/${asset}" --output "${temp_dir}/${asset}"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
    "${base_url}/${asset}.sha256" --output "${temp_dir}/${asset}.sha256"

expected="$(awk '{print $1}' "${temp_dir}/${asset}.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "${temp_dir}/${asset}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "${temp_dir}/${asset}" | awk '{print $1}')"
else
    echo "error: sha256sum or shasum is required" >&2
    exit 1
fi
[ "$actual" = "$expected" ] || { echo "error: checksum mismatch" >&2; exit 1; }

tar -xzf "${temp_dir}/${asset}" -C "$temp_dir"
mkdir -p "$bin_dir"
install -m 755 "${temp_dir}/fabricaup" "${bin_dir}/fabricaup"

path_line="export PATH=\"${bin_dir}:\$PATH\""
if [ "${FABRICAUP_NO_MODIFY_PATH:-0}" != "1" ]; then
    case "${SHELL:-}" in
        */zsh) profile="${HOME}/.zshenv" ;;
        *) profile="${HOME}/.profile" ;;
    esac
    if ! grep -F "$path_line" "$profile" >/dev/null 2>&1; then
        printf '\n# Fabrica EDA\n%s\n' "$path_line" >> "$profile"
        echo "updated ${profile}"
    fi
fi

echo "installed fabricaup to ${bin_dir}/fabricaup"
if [ "${FABRICAUP_INIT_SKIP:-0}" != "1" ]; then
    "${bin_dir}/fabricaup" install
fi
printf 'restart your shell or run: export PATH="%s:$PATH"\n' "$bin_dir"
