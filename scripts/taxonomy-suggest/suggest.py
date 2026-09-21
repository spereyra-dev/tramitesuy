#!/usr/bin/env python3
"""Offline taxonomy suggester: Laya coarse-to-fine classification.

Stage 1: pick the taxonomy category for each unlinked procedure.
Stage 2: pick the citizen event within that category (or "none").
Writes suggestions.jsonl with per-question probabilities. Offline tool only:
nothing here runs in the product pipeline; every suggestion requires human
verification against the local catalog before entering YAML.
"""
import argparse
import glob
import json
import os

import yaml
import laya

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DATA = os.path.join(ROOT, "data")


def load_taxonomy():
    categories = {}
    for path in glob.glob(os.path.join(DATA, "categories", "*.yaml")):
        doc = yaml.safe_load(open(path, encoding="utf-8"))
        categories[doc["slug"]] = doc["name"]
    events = {}
    for path in glob.glob(os.path.join(DATA, "events", "*.yaml")):
        doc = yaml.safe_load(open(path, encoding="utf-8"))
        events[doc["slug"]] = {
            "name": doc["name"],
            "description": doc.get("description", ""),
            "category": doc["category"],
        }
    return categories, events


def state_text(row):
    parts = [row["name"], row.get("description") or "", row.get("organization") or ""]
    return " \n ".join(p for p in parts if p)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", default="unlinked.jsonl")
    parser.add_argument("--output", default="suggestions.jsonl")
    parser.add_argument("--limit", type=int, default=0, help="only first N rows (0=all)")
    parser.add_argument("--threshold", type=float, default=0.35, help="min event probability to keep")
    args = parser.parse_args()

    categories, events = load_taxonomy()
    cat_slugs = sorted(categories)
    ev_by_cat = {}
    for slug in sorted(events):
        ev_by_cat.setdefault(events[slug]["category"], []).append(slug)

    rows = [json.loads(line) for line in open(args.input, encoding="utf-8")]
    if args.limit:
        rows = rows[: args.limit]

    agent = laya.load("convaiinnovations/laya", subfolder="multilingual")

    def answer(result, name):
        entry = result["answers"][name]
        return entry["choice"], entry.get("probability", entry.get("confidence", 0.0))

    kept = 0
    with open(args.output, "w", encoding="utf-8") as out:
        for row in rows:
            state = {"body": " \n ".join([row["name"], row.get("description") or "", row.get("organization") or ""])}
            res = agent.predict(state, {
                "category": {
                    "type": "choice",
                    "instructions": "¿A qué categoría de trámites del Estado uruguayo corresponde este trámite?",
                    "criteria": {s: categories[s] for s in cat_slugs},
                }
            })
            cat, cat_p = answer(res, "category")
            candidates = ev_by_cat.get(cat, [])
            if not candidates:
                continue
            res2 = agent.predict(state, {
                "event": {
                    "type": "choice",
                    "instructions": "¿Cuál de estos eventos ciudadanos cubre mejor el trámite? Si ninguno encaja, responde none.",
                    "criteria": {**{s: events[s]["name"] for s in candidates}, "none": "ninguno de los anteriores"},
                }
            })
            ev, ev_p = answer(res2, "event")
            if ev == "none" or ev_p < args.threshold:
                continue
            out.write(json.dumps({
                "external_id": row["external_id"],
                "name": row["name"],
                "suggested_category": cat,
                "category_probability": round(cat_p, 3),
                "suggested_event": ev,
                "event_name": events[ev]["name"],
                "event_probability": round(ev_p, 3),
                "state_text": " \n ".join([row["name"], row.get("description") or ""])[:400],
            }, ensure_ascii=False) + "\n")
            kept += 1
    print(f"classified {len(rows)} procedures, kept {kept} suggestions -> {args.output}")


if __name__ == "__main__":
    main()
