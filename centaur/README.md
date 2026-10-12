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

### Python Bybit fill send telemetry (issue #17)

The Python execution binding captures the execution / execution.fast envelope's
root `creationTime` after the existing fill parser succeeds, before scheduling the
existing callback. Reports, decoder acceptance, callback count/order and trading
deduplication are unchanged. `execTime` is never used as a fallback.

Each WebSocket client owns an optional sidecar shared with its clones, keyed by
`(instrument_id, venue_order_id, trade_id)` using the parsed report's full identity.
It is disabled by default. First writer wins, including zero; reads do not consume
or renew entries. Capacity is 16,384, monotonic TTL is 60 seconds, oldest entries
are evicted. Negative/zero/UInt64 ns multiplication overflow maps to zero; missing
`creationTime` still fails the original decoder. Lock contention waits for the
short, I/O-free table operation; poison increments `failures`, returns zero on
lookup and skips recording without interrupting fill delivery. The disabled path
performs only the enable check: no clock read, key allocation or table lock.

Python capability on `BybitWebSocketClient` (also declared in the core stub):

```python
def set_fill_send_telemetry(self, enabled: bool) -> None: ...
def fill_source_send_ns(self, instrument_id: str, venue_order_id: str, trade_id: str) -> int: ...
def fill_send_stats(self) -> dict[str, int]: ...
```

Stats contain `entries`, `overflow`, `expiry`, `failures`; expiry is counted on
record/lookup/stats cleanup. Disabling clears entries/eviction/expiry counters;
lock failures remain cumulative. Enabling again is idempotent. Separate clients
never share timestamps. REST-only fills return zero; with the accepted identity
lookup semantics a REST fill followed by WS can acquire the WS send at trace lookup.

Deploy this increment together with centaur's Python lookup and a newly built
Nautilus wheel. Rebuild/verify/reissue matching MDD artifacts for the new binary
SHA; MDD source and CTN6 schema do not change. The production hashes above describe
the original wheel, not this unbuilt increment. Maintainers own packaging,
manifest updates and real-wheel integration validation.

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
