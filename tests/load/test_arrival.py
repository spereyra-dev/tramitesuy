#!/usr/bin/env python3
"""Unit tests for the arrival-rate load generator (S14 task 47, strict TDD RED).

Run: `python3 -m unittest discover -s tests/load -p 'test_*.py'`

These pin the harness *mechanics* — slot scheduling, warm-up exclusion,
arrival-rate tolerance, never-sent accounting and response classification —
with an injected fake transport (no HTTP, no database, milliseconds total).
The executed sustained runs are recorded in `tests/load/RESULTS.md`.
"""

import time
import unittest

import arrival


def fake_transport(records=None, delay=0.0, status=200, retry_after=None):
    """Transport stub: sleeps `delay`, returns `status`, optionally records."""
    calls = {"count": 0}

    def transport(url, timeout):
        calls["count"] += 1
        if records is not None:
            records.append(url)
        if delay:
            time.sleep(delay)
        return {
            "status": status,
            "retry_after": retry_after,
            "mode": "open" if "/search" in url else None,
            "err": None,
        }

    transport.calls = calls
    return transport


class PercentileTest(unittest.TestCase):
    def test_nearest_rank_percentiles(self):
        values = list(range(1, 101))  # 1..100
        self.assertEqual(arrival.percentile(values, 50), 50)
        self.assertEqual(arrival.percentile(values, 95), 95)
        self.assertEqual(arrival.percentile(values, 99), 99)
        self.assertEqual(arrival.percentile(values, 100), 100)

    def test_empty_and_single_value(self):
        self.assertIsNone(arrival.percentile([], 95))
        self.assertEqual(arrival.percentile([7], 50), 7)


class ToleranceTest(unittest.TestCase):
    """TRIANGULATE machinery: observed arrival vs configured, ±tolerance."""

    def test_within_tolerance_passes(self):
        self.assertTrue(arrival.rate_within_tolerance(20.0, 19.4, 0.05))
        self.assertTrue(arrival.rate_within_tolerance(20.0, 20.6, 0.05))

    def test_outside_tolerance_fails(self):
        # Falsifiability: the verdict must reject an observed rate 10 % off.
        self.assertFalse(arrival.rate_within_tolerance(20.0, 18.0, 0.05))
        self.assertFalse(arrival.rate_within_tolerance(20.0, 22.0, 0.05))


class ClassificationTest(unittest.TestCase):
    def test_classes(self):
        self.assertEqual(arrival.classify({"status": 200, "retry_after": None, "err": None}), "ok")
        self.assertEqual(
            arrival.classify({"status": 503, "retry_after": "1", "err": None}),
            "controlled_rejection",
        )
        self.assertEqual(
            arrival.classify({"status": 503, "retry_after": None, "err": None}),
            "unexpected_http",
        )
        self.assertEqual(arrival.classify({"status": 504, "retry_after": None, "err": None}), "deadline")
        self.assertEqual(arrival.classify({"status": 500, "retry_after": None, "err": None}), "unexpected_http")
        self.assertEqual(arrival.classify({"status": 400, "retry_after": None, "err": None}), "unexpected_http")
        self.assertEqual(
            arrival.classify({"status": 0, "retry_after": None, "err": "URLError"}),
            "transport_error",
        )


class ScenarioTest(unittest.TestCase):
    def test_unique_queries_are_unique(self):
        seen = set()
        for seq in range(500):
            q = arrival.unique_query("base consulta", "run-1", seq)
            self.assertNotIn(q, seen)
            seen.add(q)
            self.assertLessEqual(len(q), 512)

    def test_warm_pool_cycles_the_committed_queries(self):
        first = arrival.warm_query(0)
        again = arrival.warm_query(len(arrival.WARM_POOL))
        self.assertEqual(first, arrival.warm_query(0))
        self.assertIsInstance(again, str)

    def test_mixed_dispatch_stays_within_the_documented_paths(self):
        urls = set()
        for seq in range(400):
            path, q = arrival.mixed_dispatch(seq, seed=7)
            self.assertIn(path, ("search", "categories", "event", "procedure"))
            if path == "search":
                self.assertTrue(q)
            urls.add((path, q))
        # The mixed mix must actually reach every documented path.
        paths = {p for p, _ in urls}
        self.assertEqual(paths, {"search", "categories", "event", "procedure"})


class RunGeneratorTest(unittest.TestCase):
    """End-to-end generator runs against the fake transport."""

    def test_warmup_is_excluded_and_arrival_counts_match_slots(self):
        cfg = arrival.RunConfig(
            base_url="http://load.test",
            scenario="warm",
            rps=20.0,
            warmup=0.4,
            duration=1.0,
            expect_rps=20.0,
            tolerance=0.30,  # fake-transport slack; real runs use the default
            max_inflight=64,
            late_grace=1.0,
            request_timeout=5.0,
            label="unit-warm",
        )
        report = arrival.run_generator(cfg, transport=fake_transport())
        self.assertEqual(report["arrival"]["scheduled_measure"], 20)
        self.assertEqual(report["warmup"]["scheduled"], 8)
        self.assertEqual(
            report["arrival"]["sent"] + arrival.never_sent_total(report), 20,
            "every measured slot is either sent or reported never-sent",
        )
        self.assertEqual(report["measure"]["sent"], report["measure"]["ok"] +
                         report["measure"]["classes"].get("transport_error", 0) +
                         report["measure"]["classes"].get("unexpected_http", 0) +
                         report["measure"]["classes"].get("deadline", 0) +
                         report["measure"]["classes"].get("controlled_rejection", 0))
        self.assertGreater(report["measure"]["latency_ms"]["count"], 0)

    def test_never_sent_is_accounted_when_the_inflight_cap_governs(self):
        cfg = arrival.RunConfig(
            base_url="http://load.test",
            scenario="warm",
            rps=100.0,
            warmup=0.0,
            duration=1.0,
            expect_rps=100.0,
            tolerance=0.05,
            max_inflight=1,  # cap governs: slow transport blocks the pipe
            late_grace=0.05,  # dispatcher late beyond grace → never sent
            request_timeout=5.0,
            label="unit-overload",
        )
        report = arrival.run_generator(cfg, transport=fake_transport(delay=0.05))
        never = arrival.never_sent_total(report)
        self.assertGreater(never, 0, "the cap/late-grace path must record never-sent slots")
        self.assertGreater(report["arrival"]["sent"], 0)
        # Never-sent slots are reported with their reasons, never as responses.
        reasons = report["arrival"]["never_sent"]
        self.assertTrue(
            reasons.get("inflight_cap", 0) > 0 or reasons.get("dispatcher_late", 0) > 0,
            f"expected a reason breakdown, got {reasons}",
        )
        self.assertFalse(report["arrival"]["within_tolerance"],
                         "a starved generator must fail the arrival verdict")

    def test_controlled_rejections_are_not_unexpected_errors(self):
        cfg = arrival.RunConfig(
            base_url="http://load.test",
            scenario="warm",
            rps=20.0,
            warmup=0.0,
            duration=0.5,
            expect_rps=20.0,
            tolerance=0.30,
            max_inflight=64,
            late_grace=1.0,
            request_timeout=5.0,
            label="unit-503",
        )
        report = arrival.run_generator(
            cfg, transport=fake_transport(status=503, retry_after="1")
        )
        self.assertEqual(report["measure"]["classes"]["controlled_rejection"],
                         report["measure"]["sent"])
        self.assertEqual(arrival.unexpected_error_count(report), 0)


if __name__ == "__main__":
    unittest.main()
