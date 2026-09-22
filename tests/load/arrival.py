#!/usr/bin/env python3
"""Arrival-rate load generator — optimize-raspi-serving, S14 task 47.

Spec §7 "Plan de carga en hardware objetivo" + tasks.md task 47:

* **Controlled arrival rate, no client think-time masking.** Slot `k` is
  scheduled at `start + k / rps` and dispatched in its own worker without
  waiting for any previous response, so a saturating server cannot hide
  behind a closed client loop that only sends when the last reply landed.
* **Warm-up.** Slots scheduled before `--warmup` seconds carry real
  traffic but are excluded from the measured statistics.
* **Honest overload accounting.** A slot the generator cannot dispatch in
  time (dispatcher backlog beyond `--late-grace`, or the `--max-inflight`
  worker cap) is recorded as *never sent* with its reason and reported
  separately — generator-side, never counted as a server response.
* **Arrival tolerance.** `observed_rps = measured_sent / duration` must
  land within ±`--tolerance` (default 5 %) of `--expect-rps`; the verdict
  gates the exit code (0 = OK, 2 = arrival mismatch) — the TRIANGULATE
  leg of task 47 at every level.
* **Overload reported separately.** 503 + `Retry-After` responses are
  `controlled_rejection` (the S12 contract), distinct from unexpected
  errors (5xx without the contract, 504, transport failures, 4xx).

Scenarios (one process per run; results are JSON + a stdout summary):
`catalog`, `warm`, `unique`, `mixed`, `burst` — see `mixed_dispatch`.

Privacy: the generator never persists query text (R2/R14) — only
counters, latency distributions and mode labels are recorded.
"""

from __future__ import annotations

import argparse
import json
import random
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# Query pools (committed, non-sensitive; mirrors the fixture's scenario set)
# ---------------------------------------------------------------------------

# Mirror of `crates/db/tests/support/catalog_fixture.rs::sample_queries()`
# (task 3): accented variants, a zero-match fallback and redaction-requiring
# INPUT shapes (synthetic by construction).
SAMPLE_QUERIES = [
    "compre un auto usado",
    "compré un auto usado",
    "vender vehículo usado",
    "vendér un vehículo",
    "pagar la patente",
    "pagár paténte",
    "consultar deuda vehicular",
    "consulta déuda vehicular",
    "transferir un vehiculo",
    "quiero abrir una cuenta bancaria",
    "cambiar matrícula de la cédula 1.111.111-1",
    "consulta al teléfono 0900 111 222",
    "escribir a ejemplo@ejemplo.uy",
]

_FALLBACK_WARMING = [
    "carnet de salud",
    "cédula de identidad",
    "comprar un vehículo",
    "licencia de conducir",
    "partida de nacimiento",
    "pasaporte",
    "turno de trámites",
    "certificado de residencia",
]


def _load_warming_pool() -> list[str]:
    """Reads `apps/api/warming_queries.txt` (the committed warming list);
    falls back to a mirror of its content when the file is unavailable."""
    from pathlib import Path

    path = Path(__file__).resolve().parents[2] / "apps" / "api" / "warming_queries.txt"
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError:
        return list(_FALLBACK_WARMING)
    queries = [
        line.strip()
        for line in lines
        if line.strip() and not line.strip().startswith("#")
    ]
    return queries or list(_FALLBACK_WARMING)


# Warm-cache pool: the committed warming list + the fixture scenario set.
WARM_POOL = _load_warming_pool() + SAMPLE_QUERIES
# Bases for unique (always-miss) queries: non-redaction scenario phrases.
UNIQUE_BASES = SAMPLE_QUERIES[:10]

SCENARIOS = ("catalog", "warm", "unique", "mixed", "burst")

# Placeholder catalog targets for pure unit tests; live runs discover the
# real ones from the API (`discover`).
DEFAULT_TARGETS = {
    "categories": ["vehiculos"],
    "events": ["comprar-vehiculo"],
    "procedures": ["4551"],
}


# ---------------------------------------------------------------------------
# Pure helpers (unit-tested)
# ---------------------------------------------------------------------------

def percentile(values, q):
    """Nearest-rank percentile (q in 1..100); None for an empty sample."""
    if not values:
        return None
    ordered = sorted(values)
    rank = max(1, min(len(ordered), (q * len(ordered) + 99) // 100))
    return ordered[rank - 1]


def rate_within_tolerance(configured, observed, tolerance):
    """|observed − configured| / configured ≤ tolerance."""
    if configured <= 0:
        return False
    return abs(observed - configured) / configured <= tolerance


def classify(record):
    """Maps one response to a harness class (overload reported apart)."""
    if record.get("err"):
        return "transport_error"
    status = record.get("status", 0)
    if 200 <= status < 300:
        return "ok"
    if status == 503 and record.get("retry_after") is not None:
        return "controlled_rejection"
    if status == 504:
        return "deadline"
    return "unexpected_http"


def warm_query(seq):
    return WARM_POOL[seq % len(WARM_POOL)]


def unique_query(base, run_token, seq):
    """Every unique query carries a distinct token ⇒ no cache hit ever."""
    return f"{base} {run_token}-{seq}"


def mixed_dispatch(seq, seed=0, targets=None):
    """Realistic mixed traffic (documented in tests/load/README.md):
    50 % search (70 % warm-pool / 30 % unique inside the searches),
    15 % category reads, 20 % event reads, 15 % procedure reads.
    Returns `(kind, payload)`; payload is the query for `search`, the
    slug/id for catalog kinds."""
    targets = targets or DEFAULT_TARGETS
    rng = random.Random(seed * 1_000_003 + seq)
    draw = rng.random()
    if draw < 0.50:
        if rng.random() < 0.70:
            return "search", warm_query(seq)
        return "search", unique_query(
            UNIQUE_BASES[seq % len(UNIQUE_BASES)], "mezcla", seq
        )
    if draw < 0.65:
        return "categories", None
    if draw < 0.85:
        return "event", targets["events"][seq % len(targets["events"])]
    return "procedure", targets["procedures"][seq % len(targets["procedures"])]


# ---------------------------------------------------------------------------
# Transport
# ---------------------------------------------------------------------------

def http_get(url, timeout):
    """Blocking GET → {status, retry_after, mode, err, body} (latency is
    measured around the full round trip including the body read)."""
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = response.read()
            return {
                "status": response.status,
                "retry_after": response.headers.get("Retry-After"),
                "mode": _extract_mode(url, body, response.status),
                "err": None,
                "body": body,
            }
    except urllib.error.HTTPError as error:
        body = error.read() if error.fp else b""
        return {
            "status": error.code,
            "retry_after": error.headers.get("Retry-After") if error.headers else None,
            "mode": None,
            "err": None,
            "body": body,
        }
    except (urllib.error.URLError, TimeoutError, ConnectionError, OSError) as error:
        return {"status": 0, "retry_after": None, "mode": None,
                "err": type(error).__name__, "body": None}


def _extract_mode(url, body, status):
    if "/api/v1/search" not in url or status != 200 or not body:
        return None
    try:
        return json.loads(body).get("mode")
    except (ValueError, AttributeError):
        return None


# ---------------------------------------------------------------------------
# Discovery (catalog targets for the catalog/mixed/burst scenarios)
# ---------------------------------------------------------------------------

def _get_json(url, timeout=10.0):
    record = http_get(url, timeout)
    if record.get("err") or record["status"] != 200:
        raise RuntimeError(f"discovery GET {url} failed: {record}")
    return record


def discover(base_url, transport=None):
    """Catalog targets from the LIVE API: category slugs, event slugs and
    procedure external ids (the `/api/v1` surface only)."""
    transport = transport or http_get

    def fetch(url):
        record = transport(url, 10.0)
        if record.get("err") or record["status"] != 200 or not record.get("body"):
            raise RuntimeError(f"discovery GET {url} failed: {record}")
        return json.loads(record["body"])

    categories = fetch(f"{base_url}/api/v1/categories")["categories"]
    cat_slugs = [c["slug"] for c in categories]
    events, procedures = [], []
    for slug in cat_slugs[:4]:
        page = fetch(f"{base_url}/api/v1/categories/{slug}/events")
        events.extend(e["slug"] for e in page.get("events", []))
    for slug in events[:4]:
        page = fetch(f"{base_url}/api/v1/events/{slug}")
        procedures.extend(p["external_id"] for p in page.get("procedures", []))
    if not (cat_slugs and events and procedures):
        raise RuntimeError("discovery produced an empty catalog target set")
    return {"categories": cat_slugs, "events": events, "procedures": procedures}


def build_url(base_url, kind, payload, seq):
    if kind == "search":
        return f"{base_url}/api/v1/search?{urllib.parse.urlencode({'q': payload})}"
    if kind == "categories":
        return f"{base_url}/api/v1/categories"
    if kind == "event":
        return f"{base_url}/api/v1/events/{urllib.parse.quote(payload)}"
    if kind == "procedure":
        return f"{base_url}/api/v1/procedures/{urllib.parse.quote(payload)}"
    raise ValueError(f"unknown dispatch kind {kind!r}")


# ---------------------------------------------------------------------------
# Run configuration + generator
# ---------------------------------------------------------------------------

@dataclass
class RunConfig:
    base_url: str
    scenario: str
    rps: float
    warmup: float
    duration: float
    expect_rps: float
    tolerance: float = 0.05
    max_inflight: int = 512
    late_grace: float = 1.0
    request_timeout: float = 10.0
    label: str = "run"
    count: int = 200          # burst only
    run_token: str = "carga"  # unique-query token (no personal data)
    seed: int = 0
    extra: dict = field(default_factory=dict)


def never_sent_total(report):
    """Measured-window never-sent slots (generator-side, not responses)."""
    return sum(report["arrival"]["never_sent"].values())


def unexpected_error_count(report):
    classes = report["measure"]["classes"]
    return (
        classes.get("transport_error", 0)
        + classes.get("unexpected_http", 0)
        + classes.get("deadline", 0)
    )


def run_generator(cfg, transport=None):
    transport = transport or http_get
    if cfg.scenario not in SCENARIOS:
        raise ValueError(f"scenario must be one of {SCENARIOS}")
    targets = None
    if cfg.scenario in ("catalog", "mixed", "burst"):
        targets = discover(cfg.base_url, transport)

    if cfg.scenario == "burst":
        warmup_slots, measure_slots = 0, cfg.count
        interval = 0.0
    else:
        warmup_slots = round(cfg.warmup * cfg.rps)
        measure_slots = round(cfg.duration * cfg.rps)
        interval = 1.0 / cfg.rps
    total_slots = warmup_slots + measure_slots

    lock = threading.Lock()
    in_flight = 0
    records, threads = [], []
    spawned_indices = set()
    never = {"warmup": {}, "measure": {}}
    max_send_late_ms = 0.0
    spawned = 0

    def skip(phase, reason):
        never[phase][reason] = never[phase].get(reason, 0) + 1

    def slot_url(seq):
        if cfg.scenario == "catalog":
            kinds = ("categories", "event", "procedure")
            kind = kinds[seq % len(kinds)]
            if kind == "event":
                payload = targets["events"][seq % len(targets["events"])]
            elif kind == "procedure":
                payload = targets["procedures"][seq % len(targets["procedures"])]
            else:
                payload = None
            return kind, build_url(cfg.base_url, kind, payload, seq)
        if cfg.scenario == "warm":
            return "search", build_url(cfg.base_url, "search", warm_query(seq), seq)
        if cfg.scenario == "unique":
            q = unique_query(UNIQUE_BASES[seq % len(UNIQUE_BASES)],
                             cfg.run_token, seq)
            return "search", build_url(cfg.base_url, "search", q, seq)
        # mixed + burst share the documented mixed distribution
        kind, payload = mixed_dispatch(seq, cfg.seed, targets)
        return kind, build_url(cfg.base_url, kind, payload, seq)

    def fire(index, phase, scheduled, url, started):
        nonlocal in_flight
        result = transport(url, cfg.request_timeout)
        result.pop("body", None)  # aggregates only; never persist payloads
        finished = time.monotonic()
        record = {
            "index": index,
            "phase": phase,
            "scheduled": scheduled,
            "sent": started,
            "latency": max(0.0, finished - started),
            "send_late": max(0.0, started - scheduled),
            **result,
        }
        record["class"] = classify(record)
        with lock:
            records.append(record)
            in_flight -= 1

    start = time.monotonic()
    for k in range(total_slots):
        phase = "warmup" if k < warmup_slots else "measure"
        seq = k
        if cfg.scenario == "burst":
            target = start
        else:
            target = start + k * interval
            remaining = target - time.monotonic()
            if remaining > 0:
                time.sleep(remaining)
        now = time.monotonic()
        lateness = max(0.0, now - target)
        if lateness > cfg.late_grace:
            # Dispatcher saturated: this slot is NEVER SENT (generator-side),
            # reported apart — never disguised as a server response.
            skip(phase, "dispatcher_late")
            continue
        with lock:
            if in_flight >= cfg.max_inflight:
                skip(phase, "inflight_cap")
                continue
            in_flight += 1
            spawned += 1
            spawned_indices.add(k)
        _, url = slot_url(seq)
        started = time.monotonic()
        thread = threading.Thread(
            target=fire, args=(k, phase, target, url, started), daemon=True
        )
        thread.start()
        threads.append(thread)

    # Per-thread window: finished threads join instantly; the last-spawned
    # ones get the full response window (never anchored at run start).
    for thread in threads:
        thread.join(timeout=cfg.request_timeout + 2.0)
    answered = {r["index"] for r in records}
    for k in sorted(spawned_indices - answered):
        phase = "warmup" if k < warmup_slots else "measure"
        # The worker was spawned (the request went out) but never
        # answered within the harness join window.
        records.append({
            "index": k, "phase": phase, "latency": cfg.request_timeout,
            "send_late": 0.0, "status": 0, "retry_after": None,
            "mode": None, "err": "harness_join_timeout",
            "class": "transport_error",
        })

    measured = [r for r in records if r["phase"] == "measure"]
    warm = [r for r in records if r["phase"] == "warmup"]
    classes, statuses, modes = {}, {}, {}
    latencies = []
    for record in measured:
        classes[record["class"]] = classes.get(record["class"], 0) + 1
        key = str(record.get("status", 0))
        statuses[key] = statuses.get(key, 0) + 1
        if record.get("mode"):
            modes[record["mode"]] = modes.get(record["mode"], 0) + 1
        if record["class"] == "ok":
            latencies.append(record["latency"] * 1000.0)

    sent = len(measured)
    observed = sent / cfg.duration if cfg.duration > 0 else None
    if cfg.scenario == "burst" or observed is None:
        within = True  # a burst makes no sustained-rate claim
    else:
        within = rate_within_tolerance(cfg.expect_rps, observed, cfg.tolerance)

    series = []
    if cfg.duration > 0:
        buckets = {}
        for record in measured:
            position = (record["index"] - warmup_slots) / max(cfg.rps, 1e-9)
            slot = min(int(position // 10), int(cfg.duration // 10))
            bucket = buckets.setdefault(slot, {"t": slot * 10, "sent": 0,
                                               "ok": 0, "err": 0, "lat": []})
            bucket["sent"] += 1
            if record["class"] == "ok":
                bucket["ok"] += 1
                bucket["lat"].append(record["latency"] * 1000.0)
            elif record["class"] in ("transport_error", "unexpected_http",
                                     "deadline"):
                bucket["err"] += 1
        for slot in sorted(buckets):
            bucket = buckets[slot]
            series.append({"t": bucket["t"], "sent": bucket["sent"],
                           "ok": bucket["ok"], "err": bucket["err"],
                           "p95_ms": percentile(bucket["lat"], 95)})

    report = {
        "label": cfg.label,
        "scenario": cfg.scenario,
        "configured_rps": cfg.rps if cfg.scenario != "burst" else None,
        "expect_rps": cfg.expect_rps,
        "tolerance": cfg.tolerance,
        "warmup_s": cfg.warmup,
        "duration_s": cfg.duration,
        "arrival": {
            "scheduled_measure": measure_slots,
            "sent": sent,
            "never_sent": never["measure"],
            "observed_rps": observed,
            "within_tolerance": within,
        },
        "warmup": {
            "scheduled": warmup_slots,
            "sent": len(warm),
            "never_sent": never["warmup"],
        },
        "measure": {
            "sent": sent,
            "ok": classes.get("ok", 0),
            "classes": classes,
            "status_histogram": statuses,
            "modes": modes,
            "latency_ms": {
                "count": len(latencies),
                "p50": percentile(latencies, 50),
                "p95": percentile(latencies, 95),
                "p99": percentile(latencies, 99),
                "max": max(latencies) if latencies else None,
                "mean": (sum(latencies) / len(latencies)) if latencies else None,
            },
        },
        "series": series,
        "generator": {
            "max_inflight": cfg.max_inflight,
            "late_grace_s": cfg.late_grace,
            "threads_spawned": spawned,
            "request_timeout_s": cfg.request_timeout,
        },
    }
    report["unexpected_errors"] = unexpected_error_count(report)
    report["unexpected_error_rate"] = (
        report["unexpected_errors"] / sent if sent else 0.0
    )
    return report


def summary_lines(report):
    arrival = report["arrival"]
    latency = report["measure"]["latency_ms"]
    verdict = "OK" if arrival["within_tolerance"] else "FAIL"
    observed = arrival["observed_rps"]
    observed_text = f"{observed:.2f} rps" if observed is not None else "n/a (burst)"
    p50 = f"{latency['p50']:.1f}" if latency["p50"] is not None else "-"
    p95 = f"{latency['p95']:.1f}" if latency["p95"] is not None else "-"
    p99 = f"{latency['p99']:.1f}" if latency["p99"] is not None else "-"
    ok_rate = (
        100.0 * report["measure"]["ok"] / report["measure"]["sent"]
        if report["measure"]["sent"] else 0.0
    )
    if report["scenario"] == "burst":
        expected_text = "n/a (burst)"
    else:
        expected_text = f"{report['expect_rps']} rps"
    return [
        f"[{report['label']}] scenario={report['scenario']} "
        f"configured={report['configured_rps']} rps "
        f"warmup={report['warmup_s']}s duration={report['duration_s']}s",
        f"arrival: observed {observed_text} vs expected {expected_text} "
        f"(±{report['tolerance'] * 100:.0f}%) → {verdict}; "
        f"sent {arrival['sent']}/{arrival['scheduled_measure']} "
        f"never-sent {arrival['never_sent'] or '{}'}",
        f"measure: ok {ok_rate:.2f}% | p50 {p50} ms  p95 {p95} ms  p99 {p99} ms "
        f"| classes {report['measure']['classes']} "
        f"| unexpected errors {report['unexpected_errors']} "
        f"({report['unexpected_error_rate'] * 100:.3f}%)",
        f"modes: {report['measure']['modes'] or '-'}",
    ]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--scenario", required=True, choices=SCENARIOS)
    parser.add_argument("--rps", type=float, default=10.0)
    parser.add_argument("--warmup", type=float, default=60.0)
    parser.add_argument("--duration", type=float, default=600.0)
    parser.add_argument("--expect-rps", type=float, default=None,
                        help="rate the arrival verdict compares against "
                             "(defaults to --rps; used by the falsifiability probe)")
    parser.add_argument("--tolerance", type=float, default=0.05,
                        help="arrival tolerance as a fraction (default 0.05 = ±5%%)")
    parser.add_argument("--count", type=int, default=200,
                        help="burst: number of simultaneous requests")
    parser.add_argument("--label", required=True)
    parser.add_argument("--out", default=None, help="JSON results path")
    parser.add_argument("--max-inflight", type=int, default=512)
    parser.add_argument("--late-grace", type=float, default=1.0)
    parser.add_argument("--request-timeout", type=float, default=10.0)
    parser.add_argument("--run-token", default="carga")
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args(argv)

    cfg = RunConfig(
        base_url=args.base_url.rstrip("/"),
        scenario=args.scenario,
        rps=args.rps,
        warmup=args.warmup,
        duration=args.duration,
        expect_rps=args.expect_rps if args.expect_rps is not None else args.rps,
        tolerance=args.tolerance,
        max_inflight=args.max_inflight,
        late_grace=args.late_grace,
        request_timeout=args.request_timeout,
        label=args.label,
        count=args.count,
        run_token=args.run_token,
        seed=args.seed,
    )
    report = run_generator(cfg)
    for line in summary_lines(report):
        print(line, flush=True)
    if args.out:
        from pathlib import Path

        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(f"results → {args.out}", flush=True)
    return 0 if report["arrival"]["within_tolerance"] else 2


if __name__ == "__main__":
    sys.exit(main())
