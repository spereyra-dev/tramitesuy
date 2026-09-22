#!/usr/bin/env bash
# Scheduled pg_dump backup to an operator-configured external/SSD target
# (task 42, S13, design §8, OPT-11, R12).
#
# The search cache is a derived, in-process structure — this backup (and the
# restore it feeds) is the ONLY recovery path for the catalog; the cache is
# never a backup substitute. Recovery after a restore goes through the
# durable generations in the restored database: the API loads a fresh
# snapshot from the restored manifest (docs/deploy-raspi.md records the
# executed rehearsal).
#
# Usage (operator .env exports PROFILE/POSTGRES_* as the compose stack):
#   BACKUP_DIR=/mnt/ssd/backups scripts/backup.sh
#   PROFILE=dev BACKUP_DIR=/tmp/backups scripts/backup.sh          (dev db)
#   DATABASE=other_db PROFILE=dev BACKUP_DIR=/tmp scripts/backup.sh
#
# Credentials are NOT handled here: the dump runs inside the database
# container with the container's own POSTGRES_USER. Never commit a real
# target or credential; make check-deploy guards the committed files.

set -euo pipefail

PROFILE="${PROFILE:-prod}"
case "$PROFILE" in
  prod) SERVICE=db-prod ;;
  dev) SERVICE=db ;;
  *) echo "backup: PROFILE must be prod or dev (got $PROFILE)" >&2; exit 1 ;;
esac

BACKUP_DIR="${BACKUP_DIR:?operator-managed external/SSD target, e.g. BACKUP_DIR=/mnt/ssd/backups}"
KEEP="${KEEP:-7}"
# Optional: dump a different database inside the same container (rehearsals
# against a disposable database use this; defaults to the service's own).
DATABASE="${DATABASE:-}"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$BACKUP_DIR"
OUT="$BACKUP_DIR/tramitesuy-$STAMP.sql.gz"

docker compose --profile "$PROFILE" exec -T \
  ${DATABASE:+-e BACKUP_DB="$DATABASE"} \
  "$SERVICE" sh -lc 'pg_dump -U "$POSTGRES_USER" -d "${BACKUP_DB:-$POSTGRES_DB}"' \
  | gzip > "$OUT"

# The dump must be a complete, loadable artifact — never a partial file that
# a later restore would silently treat as the catalog.
gzip -t "$OUT"
SIZE=$(wc -c <"$OUT" | tr -d ' ')
[ "$SIZE" -gt 1024 ] || {
  echo "backup: FAIL: $OUT is only $SIZE bytes — refusing to keep a near-empty dump" >&2
  rm -f "$OUT"
  exit 1
}

# Checksum travels next to the dump; the restore rehearsal verifies it.
(
  cd "$BACKUP_DIR" &&
    { shasum -a 256 "$(basename "$OUT")" 2>/dev/null || sha256sum "$(basename "$OUT")"; } \
      > "$(basename "$OUT").sha256"
)

# Retention: prune older dumps beyond KEEP.
ls -1t "$BACKUP_DIR"/tramitesuy-*.sql.gz 2>/dev/null | tail -n +$((KEEP + 1)) | xargs rm -f 2>/dev/null || true

echo "backup: OK $OUT ($SIZE bytes)"
