#!/usr/bin/env bash
# Restore a backup produced by scripts/backup.sh into a target database
# (task 42, S13, design §8).
#
# The target database MUST already exist and be disposable for rehearsals
# (`DATABASE=...` selects it inside the database container). The script
# never drops or creates databases — a destructive restore is an explicit
# operator action outside this script. The restored catalog is recovered
# from the backup (schema + data + durable generations travel in the
# dump); no recovery path relies on the search cache, which is derived,
# in-process and rebuilt from the restored manifest by the API's
# reconciliation (see docs/deploy-raspi.md for the executed rehearsal).
#
#   PROFILE=dev DATABASE=backup_rehearsal_b scripts/restore.sh /mnt/ssd/backups/tramitesuy-STAMP.sql.gz

set -euo pipefail

PROFILE="${PROFILE:-prod}"
case "$PROFILE" in
  prod) SERVICE=db-prod ;;
  dev) SERVICE=db ;;
  *) echo "restore: PROFILE must be prod or dev (got $PROFILE)" >&2; exit 1 ;;
esac

FILE="${1:?usage: restore.sh <backup.sql.gz> (with PROFILE/DATABASE selected)}"
TARGET_DB="${DATABASE:?select the target database: DATABASE=<disposable db> scripts/restore.sh ...}"

[ -f "$FILE" ] || { echo "restore: no such backup file: $FILE" >&2; exit 1; }

# Verify the checksum when it travels with the dump.
if [ -f "$FILE.sha256" ]; then
  (
    cd "$(dirname "$FILE")" &&
      { shasum -a 256 -c "$(basename "$FILE").sha256" 2>/dev/null || sha256sum -c "$(basename "$FILE").sha256"; }
  ) || { echo "restore: FAIL: checksum mismatch for $FILE" >&2; exit 1; }
fi

docker compose --profile "$PROFILE" exec -T \
  -e RESTORE_DB="$TARGET_DB" \
  "$SERVICE" sh -lc 'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$RESTORE_DB"' \
  < <(gunzip -c "$FILE")

echo "restore: OK $FILE -> $TARGET_DB"
