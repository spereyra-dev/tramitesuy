#!/usr/bin/env bash
# `make check-deploy` (task 40, S13, OPT-11, R12): deployment-profile
# assertions over the committed configuration. Config-only: it renders
# compose models and greps committed files — it never builds, starts,
# stops, or otherwise touches running containers.
set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "check-deploy: FAIL: $*" >&2; exit 1; }

ENV_FILE=.env.example
COMPOSE=docker-compose.yml

# [task 40] The prod profile renders with the operator .env.example template
# (never a developer's local .env — --env-file replaces the default).
cfg=$(docker compose --env-file "$ENV_FILE" --profile prod config --format json --no-interpolate) \
  || fail "docker compose --profile prod config does not render (required variables missing from .env.example?)"

# [task 40] PostgreSQL is reachable only on the internal network: no
# prod-profile service publishes the 5432 port to the host. (The assertion
# walks the full rendered model and selects the services that declare the
# prod profile — the compose model may also carry dev-profile services.)
echo "$cfg" | jq -e '
  [ .services | to_entries[]
    | select(.value.profiles // [] | index("prod"))
    | .value.ports // [] | .[]
    | (.published // "" | tostring) ]
  | map(select(. == "5432"))
  | length == 0' >/dev/null \
  || fail "the prod profile publishes the 5432 port"

# [task 40] The enabled set under `--profile prod` contains no dev service:
# the dev stack never starts under the production profile.
enabled=$(docker compose --env-file "$ENV_FILE" --profile prod config --format json | jq -c '.services | keys')
echo "$enabled" | jq -e '
  map(select(. == "db" or . == "api" or . == "ingest" or . == "web"))
  | length == 0' >/dev/null \
  || fail "a dev service would start under --profile prod (enabled: $enabled)"
for prod_service in db-prod api-prod ingest-prod proxy; do
  echo "$enabled" | jq -e --arg s "$prod_service" 'index($s) != null' >/dev/null \
    || fail "prod service $prod_service missing from the enabled set"
done

# [task 40] Production services run under the restart policy and expose
# healthcheck-based readiness.
echo "$cfg" | jq -e '.services["db-prod"].restart == "unless-stopped"' >/dev/null \
  || fail "db-prod does not use restart: unless-stopped"
echo "$cfg" | jq -e '.services["api-prod"].restart == "unless-stopped"' >/dev/null \
  || fail "api-prod does not use restart: unless-stopped"
echo "$cfg" | jq -e '.services["db-prod"].healthcheck.test | length > 0' >/dev/null \
  || fail "db-prod has no healthcheck"
echo "$cfg" | jq -e '.services["api-prod"].healthcheck.test | length > 0' >/dev/null \
  || fail "api-prod has no healthcheck (healthcheck-based readiness)"

# [task 40] Credentials come from the operator-managed .env interpolation,
# never from literals. (Compose v5 interpolates the whole file even for
# inactive profiles, so the prod services default the variables to EMPTY
# instead of `:?`-required — an empty password makes the postgres image
# itself refuse to start, which is the documented fail-safe; the operator
# contract is --env-file /etc/tramitesuy/.env.)
echo "$cfg" | jq -e '
  .services["db-prod"].environment.POSTGRES_PASSWORD
  | test("^\\$\\{POSTGRES_PASSWORD")' >/dev/null \
  || fail "db-prod POSTGRES_PASSWORD is not operator-managed interpolation"
echo "$cfg" | jq -e '
  .services["api-prod"].environment.DATABASE_URL
  | test("\\$\\{POSTGRES_PASSWORD")' >/dev/null \
  || fail "api-prod DATABASE_URL embeds a literal credential instead of the operator .env"

# [task 40] .env is gitignored and untracked; the .env.example template is
# committed and declares every variable the prod profile interpolates.
git check-ignore -q .env \
  || fail ".env is not gitignored — the operator-managed credential file could be committed"
[ -z "$(git ls-files -- .env | head -1)" ] \
  || fail ".env is tracked in git — the operator-managed credential file must never be committed"
git ls-files --error-unmatch "$ENV_FILE" >/dev/null 2>&1 \
  || fail "$ENV_FILE is not committed"
# Every ${VAR} referenced by the prod profile must be declared in the template.
echo "$cfg" | grep -oE '\$\{[A-Z_]+[}:]' | grep -oE '[A-Z_]+' | sort -u | while read -r var; do
  grep -qE "^${var}=" "$ENV_FILE" || fail "$var is interpolated by the prod profile but not declared in $ENV_FILE"
done

# [task 40] No committed file contains a credential value. The documented
# exceptions are exactly two: the dev compose stack's local development
# credential (postgres) and the change-me* placeholders in .env.example.
violations=$(git grep -nIE '(POSTGRES_PASSWORD|PGPASSWORD)[[:space:]]*[:=][[:space:]]*[^$#]' \
  -- ':!scripts/check-deploy.sh' 2>/dev/null \
  | grep -vE '(postgres|change-me|\$\{)' || true)
[ -z "$violations" ] || fail "a committed file contains a credential value:
$violations"

# [task 40] The Dockerfile builds the release ARM64 (aarch64-unknown-linux-gnu)
# API/ingest image.
grep -q 'aarch64-unknown-linux-gnu' Dockerfile \
  || fail "the Dockerfile does not build the aarch64-unknown-linux-gnu release image"
grep -qE 'CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER' Dockerfile \
  || fail "the Dockerfile does not wire the ARM64 GNU cross linker"
make -n image-arm64 >/dev/null \
  || fail "the make image-arm64 target does not resolve"

# [task 40, TRIANGULATE] The dev profile is unchanged: the named db service
# (what `docker compose up -d db` resolves) still publishes 5432 locally.
devcfg=$(docker compose --env-file "$ENV_FILE" config db --format json) \
  || fail "the dev db service no longer resolves (docker compose up -d db would break)"
echo "$devcfg" | jq -e '
  [ .services["db"].ports[] | (.published // "" | tostring) ]
  | index("5432") != null' >/dev/null \
  || fail "the dev db service no longer publishes 5432 for local development"

echo "check-deploy: OK (prod profile internal-only, credentials operator-managed, ARM64 target present, dev profile unchanged)"
