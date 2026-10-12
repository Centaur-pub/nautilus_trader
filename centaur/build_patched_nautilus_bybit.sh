#!/usr/bin/env bash
set -euo pipefail

readonly NAUTILUS_TAG="v1.231.0"
readonly NAUTILUS_COMMIT="27a8e54e7ac3c57d6cbf8891f0283dfbaee97317"
readonly BUILDER_IMAGE="centaur-nautilus-1.231.0-builder"
readonly RUST_IMAGE="rust:1.97.1-bookworm"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Applied in order on top of NAUTILUS_COMMIT.
patch_files=(
    "${repo_root}/patches/nautilus_trader-1.231.0-bybit-string-integers.patch"
    "${repo_root}/patches/nautilus_trader-1.231.0-bybit-fill-send.patch"
)
mode="${1:-test}"
output_dir="${2:-${repo_root}/dist}"
source_dir="$(mktemp -d -t centaur-nautilus-bybit.XXXXXXXX)"

cleanup() {
    if [[ "$(basename -- "${source_dir}")" == centaur-nautilus-bybit.* ]]; then
        rm -rf -- "${source_dir}"
    else
        echo "Refusing to remove unexpected temporary path: ${source_dir}" >&2
    fi
}
trap cleanup EXIT

git clone --quiet --branch "${NAUTILUS_TAG}" --depth 1 \
    https://github.com/nautechsystems/nautilus_trader.git "${source_dir}"

actual_commit="$(git -C "${source_dir}" rev-parse HEAD)"
if [[ "${actual_commit}" != "${NAUTILUS_COMMIT}" ]]; then
    echo "Unexpected ${NAUTILUS_TAG} commit: ${actual_commit}" >&2
    exit 1
fi

for patch_file in "${patch_files[@]}"; do
    git -C "${source_dir}" apply --check "${patch_file}"
    git -C "${source_dir}" apply "${patch_file}"
done

case "${mode}" in
    test)
        docker run --rm \
            -e CARGO_TARGET_DIR=/tmp/centaur-nautilus-target \
            -e RUSTUP_TOOLCHAIN=1.97.1 \
            -v "${source_dir}:/src:ro" \
            -w /src \
            "${RUST_IMAGE}" \
            sh -c "cargo test -p nautilus-bybit --lib string_integer_fields && cargo test -p nautilus-bybit --lib fill_send"
        ;;
    wheel)
        mkdir -p "${output_dir}"
        docker build -f "${source_dir}/.docker/DockerfileUbuntu" \
            -t "${BUILDER_IMAGE}" "${source_dir}"
        docker run --rm \
            --user "$(id -u):$(id -g)" \
            -e CARGO_HOME=/tmp/centaur-cargo-home \
            -e HOME=/tmp \
            -e CARGO_TARGET_DIR=/tmp/centaur-nautilus-target \
            -e UV_CACHE_DIR=/tmp/centaur-uv-cache \
            -v "${source_dir}:/workspace" \
            "${BUILDER_IMAGE}" \
            make build-wheel
        cp "${source_dir}"/dist/nautilus_trader-*.whl "${output_dir}/"
        ;;
    *)
        echo "Usage: $0 [test|wheel] [wheel-output-directory]" >&2
        exit 2
        ;;
esac
