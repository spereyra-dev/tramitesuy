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

# [task 40, audit F6] Docker ARG scoping: a pre-FROM global `ARG TARGETARCH`
# is NOT in scope inside a stage (Docker's documented scoping), so the build
# stage must re-declare it or the cross-compilation branch is unreachable and
# the native branch ships builder-architecture binaries under
# `--platform linux/arm64`. The stage must also assert the produced binaries'
# machine type against the requested target.
build_stage=$(awk '/^FROM .* AS build/{f=1;next} f && /^FROM /{f=0} f' Dockerfile)
[ -n "$build_stage" ] \
  || fail "the Dockerfile no longer declares a build stage (FROM ... AS build)"
echo "$build_stage" | grep -qE '^ARG[[:space:]]+TARGETARCH([[:space:]]|$)' \
  || fail "the build stage does not re-declare ARG TARGETARCH — a pre-FROM global ARG is out of scope inside a stage, so \$TARGETARCH expands empty and the ARM64 build ships builder-architecture binaries"
awk '/^FROM /{exit} {print}' Dockerfile | grep -qE '^ARG[[:space:]]+TARGETARCH' \
  && fail "a global pre-FROM ARG TARGETARCH remains (redundant: the FROM line does not use it)"
# The architecture assertion must be a real RUN command, not just prose in a
# comment: check the non-comment build-stage commands.
build_cmds=$(echo "$build_stage" | grep -vE '^[[:space:]]*#')
echo "$build_cmds" | grep -q 'readelf -h' \
  || fail "the Dockerfile carries no readelf architecture assertion for the built binaries"
echo "$build_cmds" | grep -qE 'Machine' \
  || fail "the architecture assertion does not compare the readelf Machine field against the expected target"
for binary in api ingest; do
  echo "$build_cmds" | grep -q "/build/bin/$binary" \
    || fail "the architecture assertion does not cover /build/bin/$binary"
done

# [audit F25] Build context: the Rust image must not copy the Next.js
# frontend (apps/web carries hundreds of MB of node_modules/.next in a
# developer checkout), and .dockerignore must keep that context out.
grep -qE '^COPY[[:space:]]+apps[[:space:]]+\./apps[[:space:]]*$' Dockerfile \
  && fail "the broad 'COPY apps ./apps' is back — it copies apps/web/node_modules and apps/web/.next into the Rust image"
grep -qE '^COPY([[:space:]]+--[^ ]+)*[[:space:]]+apps/web' Dockerfile \
  && fail "the Dockerfile copies apps/web into the Rust image"
grep -qE '^COPY[[:space:]]+apps/api[[:space:]]+\./apps/api[[:space:]]*$' Dockerfile \
  || fail "the Dockerfile no longer narrow-copies 'apps/api ./apps/api'"
grep -qE '^COPY[[:space:]]+apps/ingest[[:space:]]+\./apps/ingest[[:space:]]*$' Dockerfile \
  || fail "the Dockerfile no longer narrow-copies 'apps/ingest ./apps/ingest'"
DOCKERIGNORE=.dockerignore
[ -f "$DOCKERIGNORE" ] \
  || fail "$DOCKERIGNORE is missing — the Rust build context would include apps/web/node_modules and apps/web/.next"
grep -qE '^/?apps/web/node_modules/?$' "$DOCKERIGNORE" \
  || fail "$DOCKERIGNORE does not exclude apps/web/node_modules"
grep -qE '^/?apps/web/\.next/?$' "$DOCKERIGNORE" \
  || fail "$DOCKERIGNORE does not exclude apps/web/.next"
# Every workspace member (Cargo.toml) and every manifest the build copies
# must stay inside the build context: no .dockerignore pattern may match a
# required input or one of its parent directories.
members=$(awk '/^members = \[/{f=1} f{print} f&&/\]/{exit}' Cargo.toml | grep -oE '"[^"]+"' | tr -d '"')
for required in Cargo.toml Cargo.lock rust-toolchain.toml migrations .sqlx data $members; do
  while IFS= read -r pattern; do
    case "$pattern" in ''|'#'*|'!'*) continue ;; esac
    pattern=${pattern#/}; pattern=${pattern%/}
    case "$required" in
      "$pattern"|"$pattern"/*) fail "$DOCKERIGNORE pattern '$pattern' excludes the required build input '$required'" ;;
    esac
  done < "$DOCKERIGNORE"
done

# [task 41, audit F10] HTTPS reverse proxy: TLS termination, readiness
# wiring to the internal /ready (read-only), internal-only probes and metrics
# outside the closed /api/v1 inventory, and an access-log format stripped of
# every query-string AND client-identifying field (AGENTS.md privacy policy:
# query, result, feedback, timestamp only — no client address, no remote
# user, no user agent).
PROXY_CONF=docker/proxy/nginx.conf
[ -f "$PROXY_CONF" ] || fail "the HTTPS reverse proxy config ($PROXY_CONF) is missing"
log_format=$(sed -n '/log_format[[:space:]][[:space:]]*privacy/,/;/p' "$PROXY_CONF")
[ -n "$log_format" ] || fail "the proxy does not define the privacy log_format"
# Banned from the access-log FORMAT: query-string fields ($args,
# $query_string, $is_args, $request_uri, $http_referer) and
# client-identifying fields ($remote_addr, $remote_user, $http_user_agent).
for banned_field in '$args' '$query_string' '$is_args' '$request_uri' '$http_referer' '$remote_addr' '$remote_user' '$http_user_agent'; do
  case "$log_format" in
    *"$banned_field"*) fail "the proxy access-log format carries the forbidden field $banned_field" ;;
  esac
done
case "$log_format" in
  *'$uri'*) : ;;
  *) fail "the proxy access-log format must log the path via \$uri (no query string)" ;;
esac
access_log=$(grep -n '^[[:space:]]*access_log' "$PROXY_CONF")
echo "$access_log" | grep -q 'privacy' \
  || fail "the proxy access_log does not use the privacy format:
$access_log"
grep -Eq 'listen[[:space:]]+443 ssl' "$PROXY_CONF" \
  || fail "the proxy does not terminate TLS (no 443 ssl listener)"
grep -qE '^[[:space:]]*ssl_certificate[[:space:]]+/etc/nginx/tls/' "$PROXY_CONF" \
  || fail "the proxy does not terminate TLS with the operator-mounted material"
grep -qE 'server[[:space:]]+api-prod:8080[[:space:]]+max_fails=' "$PROXY_CONF" \
  || fail "the proxy does not wire readiness to api-prod (passive health checks against the 503 cold start)"
grep -q 'proxy_next_upstream.*http_503' "$PROXY_CONF" \
  || fail "the proxy does not treat the cold-start 503 as a failed upstream"
grep -Eq '^[[:space:]]*location[[:space:]]+=[[:space:]]+/ready[[:space:]]*\{[[:space:]]*return[[:space:]]+404' "$PROXY_CONF" \
  || fail "the public proxy must deny /ready (probes stay internal-only, outside the closed /api/v1 inventory)"
grep -Eq '^[[:space:]]*location[^;]*metrics' "$PROXY_CONF" \
  && fail "the proxy exposes a metrics location (metrics are internal only)"
[ "$(grep -c 'proxy_pass' "$PROXY_CONF")" -eq 1 ] \
  || fail "the proxy routes through an unexpected number of upstreams"

# [task 41] The proxy's own healthcheck probes a query-less catalog route:
# the health traffic itself never carries a query string.
echo "$cfg" | jq -e '
  .services["proxy"].healthcheck.test | join(" ")
  | (test("/api/v1/categories") and (test("\\?q=") | not))' >/dev/null \
  || fail "the proxy healthcheck does not use a query-less internal probe"

# [task 40, TRIANGULATE] The dev profile is unchanged: the named db service
# (what `docker compose up -d db` resolves) still publishes 5432 locally.
devcfg=$(docker compose --env-file "$ENV_FILE" config db --format json) \
  || fail "the dev db service no longer resolves (docker compose up -d db would break)"
echo "$devcfg" | jq -e '
  [ .services["db"].ports[] | (.published // "" | tostring) ]
  | index("5432") != null' >/dev/null \
  || fail "the dev db service no longer publishes 5432 for local development"

echo "check-deploy: OK (prod profile internal-only, credentials operator-managed, ARM64 target present, dev profile unchanged)"
