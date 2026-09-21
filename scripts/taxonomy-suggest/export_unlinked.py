#!/usr/bin/env python3
"""Export active catalog procedures not yet linked to any taxonomy event.

Reads the dev Postgres (docker-mapped localhost:5432) and writes JSONL rows
{external_id, name, description, organization}. Read-only; no writes.
"""
import json
import os
import sys

import psycopg

DB_URL = os.environ.get("DATABASE_URL", "postgresql://postgres:postgres@localhost:5432/tramitesuy")
OUT = sys.argv[1] if len(sys.argv) > 1 else "unlinked.jsonl"

QUERY = """
SELECT p.external_id, p.name, COALESCE(p.description, ''), COALESCE(o.name, '')
FROM procedures p
LEFT JOIN organizations o ON o.id = p.organization_id
WHERE p.status = 'active'
  AND NOT EXISTS (
    SELECT 1 FROM life_event_procedures r
    JOIN life_events e ON e.id = r.life_event_id
    WHERE r.procedure_id = p.id
  )
ORDER BY p.external_id
"""

def main() -> None:
    with psycopg.connect(DB_URL) as conn, conn.cursor() as cur:
        cur.execute(QUERY)
        rows = cur.fetchall()
    with open(OUT, "w", encoding="utf-8") as f:
        for external_id, name, description, organization in rows:
            f.write(json.dumps({
                "external_id": external_id,
                "name": name,
                "description": description,
                "organization": organization,
            }, ensure_ascii=False) + "\n")
    print(f"exported {len(rows)} unlinked procedures -> {OUT}")

if __name__ == "__main__":
    main()
