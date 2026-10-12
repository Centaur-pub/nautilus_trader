// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Optional, per-client execution envelope timestamps. Never participates in trading.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use nautilus_model::reports::FillReport;

const CAPACITY: usize = 16_384;
const TTL: Duration = Duration::from_secs(60);
type FillKey = (String, String, String);

fn send_ms_to_ns(value: Option<i64>) -> u64 {
    value
        .and_then(|ms| u64::try_from(ms).ok())
        .and_then(|ms| ms.checked_mul(1_000_000))
        .unwrap_or(0)
}

#[derive(Debug, Default)]
struct Table {
    items: HashMap<FillKey, u64>,
    arrivals: VecDeque<(Instant, FillKey)>,
    overflow: u64,
    expiry: u64,
}

impl Table {
    fn expire(&mut self, now: Instant) {
        while self
            .arrivals
            .front()
            .is_some_and(|(seen, _)| now.duration_since(*seen) >= TTL)
        {
            if let Some((_, key)) = self.arrivals.pop_front() {
                self.items.remove(&key);
                self.expiry = self.expiry.saturating_add(1);
            }
        }
    }

    fn remember(&mut self, key: FillKey, send_ns: u64, now: Instant) {
        self.expire(now);
        if self.items.contains_key(&key) {
            return; // Includes zero; duplicates never renew TTL.
        }
        if self.items.len() >= CAPACITY
            && let Some((_, oldest)) = self.arrivals.pop_front()
        {
            self.items.remove(&oldest);
            self.overflow = self.overflow.saturating_add(1);
        }
        self.items.insert(key.clone(), send_ns);
        self.arrivals.push_back((now, key));
    }
}

/// Shared by a client's clones, isolated from separately constructed clients.
#[derive(Debug, Default)]
pub(crate) struct FillSendTimes {
    enabled: AtomicBool,
    table: Mutex<Table>,
    failures: AtomicU64,
}

impl FillSendTimes {
    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            match self.table.lock() {
                Ok(mut table) => *table = Table::default(),
                Err(_) => {
                    self.failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    pub(crate) fn remember(&self, report: &FillReport, creation_time: i64) {
        // Disabled: no clock read, key allocation or table lock.
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        match self.table.lock() {
            Ok(mut table) => {
                if self.enabled.load(Ordering::Relaxed) {
                    let key = (
                        report.instrument_id.to_string(),
                        report.venue_order_id.to_string(),
                        report.trade_id.to_string(),
                    );
                    table.remember(key, send_ms_to_ns(Some(creation_time)), Instant::now());
                }
            }
            Err(_) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn get(&self, instrument_id: &str, venue_order_id: &str, trade_id: &str) -> u64 {
        if !self.enabled.load(Ordering::Relaxed) {
            return 0;
        }
        if instrument_id.is_empty() || venue_order_id.is_empty() || trade_id.is_empty() {
            return 0;
        }
        match self.table.lock() {
            Ok(mut table) => {
                table.expire(Instant::now());
                let key = (
                    instrument_id.to_owned(),
                    venue_order_id.to_owned(),
                    trade_id.to_owned(),
                );
                table.items.get(&key).copied().unwrap_or(0)
            }
            Err(_) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
                0
            }
        }
    }

    pub(crate) fn stats(&self) -> HashMap<String, u64> {
        let mut stats = HashMap::from([
            ("entries".to_owned(), 0),
            ("overflow".to_owned(), 0),
            ("expiry".to_owned(), 0),
        ]);
        match self.table.lock() {
            Ok(mut table) => {
                table.expire(Instant::now());
                stats.insert("entries".to_owned(), table.items.len() as u64);
                stats.insert("overflow".to_owned(), table.overflow);
                stats.insert("expiry".to_owned(), table.expiry);
            }
            Err(_) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
            }
        }
        stats.insert("failures".to_owned(), self.failures.load(Ordering::Relaxed));
        stats
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(None, 0)]
    #[case(Some(-1), 0)]
    #[case(Some(0), 0)]
    #[case(Some(2), 2_000_000)]
    #[case(Some((u64::MAX / 1_000_000) as i64), (u64::MAX / 1_000_000) * 1_000_000)]
    #[case(Some((u64::MAX / 1_000_000 + 1) as i64), 0)]
    #[case(Some(i64::MAX), 0)]
    fn checked_conversion(#[case] ms: Option<i64>, #[case] expected: u64) {
        assert_eq!(send_ms_to_ns(ms), expected);
    }

    fn key(id: usize) -> FillKey {
        (
            "BTCUSDT-LINEAR.BYBIT".into(),
            "order-uuid".into(),
            id.to_string(),
        )
    }

    #[rstest]
    fn first_writer_capacity_ttl_and_counters() {
        let mut table = Table::default();
        let now = Instant::now();
        for id in 0..CAPACITY {
            table.remember(key(id), id as u64, now);
        }
        table.remember(key(0), 999, now + Duration::from_secs(1));
        assert_eq!(table.items[&key(0)], 0);
        assert_eq!(table.arrivals.len(), CAPACITY);
        table.remember(key(CAPACITY), 42, now + Duration::from_secs(1));
        assert!(!table.items.contains_key(&key(0)));
        assert_eq!(table.items.len(), CAPACITY);
        assert_eq!(table.arrivals.len(), CAPACITY);
        assert_eq!(table.overflow, 1);
        table.expire((now + TTL).checked_sub(Duration::from_nanos(1)).unwrap());
        assert_eq!(table.expiry, 0);
        table.expire(now + TTL);
        assert_eq!(table.expiry, (CAPACITY - 1) as u64);
        assert_eq!(table.items.len(), 1);
        table.expire(now + TTL + Duration::from_secs(1));
        assert_eq!(table.expiry, CAPACITY as u64);
        assert!(table.items.is_empty());
        assert!(table.arrivals.is_empty());
    }

    #[rstest]
    fn disabled_lookup_does_not_lock() {
        let times = FillSendTimes::default();
        let guard = times.table.lock().unwrap();
        assert_eq!(times.get("i", "o", "t"), 0);
        assert_eq!(times.failures.load(Ordering::Relaxed), 0);
        drop(guard);
        times.set_enabled(false);
        assert_eq!(times.stats()["entries"], 0);
    }

    fn report() -> FillReport {
        use nautilus_core::UnixNanos;
        use nautilus_model::identifiers::AccountId;

        use crate::{
            common::{parse::parse_linear_instrument, testing::load_test_json},
            http::models::{BybitFeeRate, BybitInstrumentLinearResponse},
            websocket::{messages::BybitWsAccountExecutionMsg, parse::parse_ws_fill_report},
        };
        let response: BybitInstrumentLinearResponse =
            serde_json::from_str(&load_test_json("http_get_instruments_linear.json")).unwrap();
        let fee_rate = BybitFeeRate {
            symbol: "BTCUSDT".into(),
            taker_fee_rate: "0.00055".into(),
            maker_fee_rate: "0.0001".into(),
            base_coin: Some("BTC".into()),
        };
        let ts = UnixNanos::new(1_700_000_000_000_000_000);
        let instrument =
            parse_linear_instrument(&response.result.list[0], &fee_rate, ts, ts).unwrap();
        let msg: BybitWsAccountExecutionMsg =
            serde_json::from_str(&load_test_json("ws_account_execution.json")).unwrap();
        parse_ws_fill_report(&msg.data[0], AccountId::new("BYBIT-001"), &instrument, ts).unwrap()
    }

    #[rstest]
    fn recording_first_writer_reads_isolation_and_expiry() {
        let report = report();
        let times = FillSendTimes::default();
        let guard = times.table.lock().unwrap();
        times.remember(&report, 2); // Would fail if the disabled path touched the lock.
        assert_eq!(times.failures.load(Ordering::Relaxed), 0);
        drop(guard);
        assert_eq!(times.stats()["entries"], 0);
        times.set_enabled(true);
        times.remember(&report, 2);
        times.remember(&report, 999);
        let instrument_id = report.instrument_id.to_string();
        let order = report.venue_order_id.to_string();
        let trade = report.trade_id.to_string();
        for _ in 0..2 {
            assert_eq!(times.get(&instrument_id, &order, &trade), 2_000_000);
        }
        let other = FillSendTimes::default();
        other.set_enabled(true);
        assert_eq!(other.get(&instrument_id, &order, &trade), 0);
        assert_eq!(times.get("other.BYBIT", &order, &trade), 0);
        assert_eq!(times.get(&instrument_id, "other-order", &trade), 0);
        assert_eq!(times.get(&instrument_id, &order, "other-trade"), 0);
        times.table.lock().unwrap().arrivals.front_mut().unwrap().0 =
            Instant::now().checked_sub(TTL).unwrap();
        assert_eq!(times.get(&instrument_id, &order, &trade), 0);
        assert_eq!(times.stats()["expiry"], 1);
        times.remember(&report, -1);
        times.remember(&report, 2);
        assert_eq!(times.get(&instrument_id, &order, &trade), 0);
        assert_eq!(times.stats()["entries"], 1);
        times.set_enabled(false);
        assert_eq!(times.stats()["entries"], 0);
    }

    #[rstest]
    #[case("remember", 2_000_000)]
    #[case("get", 2_000_000)]
    #[case("stats", 1)]
    fn contention_waits_without_losing_records(#[case] operation: &str, #[case] expected: u64) {
        use std::sync::mpsc;

        let times = FillSendTimes::default();
        let report = report();
        let instrument = report.instrument_id.to_string();
        let order = report.venue_order_id.to_string();
        let trade = report.trade_id.to_string();
        times.set_enabled(true);
        if operation != "remember" {
            times.remember(&report, 2);
        }
        let guard = times.table.lock().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                started_tx.send(()).unwrap();
                let result = match operation {
                    "remember" => {
                        times.remember(&report, 2);
                        times.get(&instrument, &order, &trade)
                    }
                    "get" => times.get(&instrument, &order, &trade),
                    "stats" => times.stats()["entries"],
                    _ => unreachable!(),
                };
                done_tx.send(result).unwrap();
            });
            let started = started_rx.recv_timeout(Duration::from_secs(5));
            // The worker must wait for the held mutex, rather than discard metadata.
            let waiting = done_rx.recv_timeout(Duration::from_millis(100));
            drop(guard);
            assert!(started.is_ok());
            assert!(matches!(waiting, Err(mpsc::RecvTimeoutError::Timeout)));
            assert_eq!(
                done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                expected
            );
        });
        assert_eq!(times.get(&instrument, &order, &trade), 2_000_000);
        assert_eq!(times.stats()["failures"], 0);
    }

    #[rstest]
    fn poisoned_sidecar_returns_zero_and_counts_failures() {
        let times = FillSendTimes::default();
        times.set_enabled(true);
        let _ = std::panic::catch_unwind(|| {
            let _guard = times.table.lock().unwrap();
            panic!("sidecar poison injection");
        });
        times.remember(&report(), 2);
        assert_eq!(times.get("i", "o", "t"), 0);
        assert_eq!(times.stats()["failures"], 3);
        times.set_enabled(false);
        assert_eq!(times.get("i", "o", "t"), 0);
        assert_eq!(times.failures.load(Ordering::Relaxed), 4);
    }
}
