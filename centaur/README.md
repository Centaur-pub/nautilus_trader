# centaur patches on NautilusTrader 1.231.0

This branch holds the source of the NautilusTrader wheel that centaur runs for
Bybit live trading, so the patched build can be rebuilt and extended.

| Item | Value |
| --- | --- |
| Upstream base | `v1.231.0` = `27a8e54e7ac3c57d6cbf8891f0283dfbaee97317` |
| Patch commit | the commit directly above `27a8e54` on this branch (tag `v1.231.0-centaur-bybitstr1`) |
| Patch file | centaur `patches/nautilus_trader-1.231.0-bybit-string-integers.patch`, applied unchanged |
| Production wheel | `nautilus_trader-1.231.0-1centaurbybitstr1-cp313-cp313-manylinux_2_35_x86_64.whl` |
| Wheel SHA256 | `34edf0447a9363d24a6f26612fb1b0e55ce7db98b54d14e2495b1c27e6f911ce` |
| `nautilus_pyo3` .so SHA256 | `d047de3a52b95363aa6d03f8174520645b7b55f4cc75218b393ef09f72b0f59e` |

## What the patch changes

Bybit occasionally sends `positionIdx`, `smpGroup` and `triggerDirection` as
strings. The WebSocket and HTTP order models accept both integer and string
encodings. The Docker builder pins its uv image and Python install path.

## Build

`build_patched_nautilus_bybit.sh` is the centaur build script (copied here for
reference; it expects to live in centaur `scripts/` next to `patches/`):

- `test`: `cargo test -p nautilus-bybit --lib string_integer_fields` in `rust:1.97.1-bookworm`;
- `wheel`: `docker build -f .docker/DockerfileUbuntu` then `make build-wheel`.

The production wheel file carries the build tag `1centaurbybitstr1`, which the
script does not add. `scripts/verify_patched_nautilus_bybit.py` in centaur
checks for the marker string `invalid Bybit trigger direction`.

## Provenance and open items

- 2026-10-11: the production `.so` SHA above was read from the installed wheel
  on the Bybit hosts; it contains the patch marker. The patch file and build
  script were taken from the centaur maintainer's working tree (not yet
  committed to centaur at that time).
- Byte-for-byte equivalence of a rebuild from this branch with the production
  `.so` has not been verified yet. Rebuild with the script and compare SHA256
  before using this branch as the base for further native changes.
