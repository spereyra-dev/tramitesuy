# TrámitesUY web UI image (add-web-frontend, tasks 24; design §7, proposal P4).
# Multi-stage build on node:22-alpine: npm ci + `next build` in the builder,
# then a minimal non-root runner stage serving `next start` on port 3000.
#
# API_BASE_URL must be present at BUILD time, not runtime: Next 15 resolves
# rewrites() when `next build` runs and bakes the destination into
# .next/routes-manifest.json, so a runtime-only env var is ignored (found by
# the compose e2e gate: the container proxied the baked dev default).
# compose passes it as a build arg (http://api:8080); the local default
# matches next.config.ts's dev default. The runner stage stays env-agnostic.

FROM node:22-alpine AS build
WORKDIR /app
ARG API_BASE_URL=http://localhost:8080
ENV API_BASE_URL=$API_BASE_URL

# Manifests first for layer caching of the dependency install.
COPY package.json package-lock.json ./
RUN npm ci

COPY . .
RUN npm run build

FROM node:22-alpine AS runner
WORKDIR /app
ENV NODE_ENV=production

# Non-root runtime user (cheap on Alpine, so we do it).
RUN addgroup --system nodejs --gid 1001 \
    && adduser --system nextjs --uid 1001 --ingroup nodejs

COPY --from=build --chown=nextjs:nodejs /app/.next ./.next
COPY --from=build --chown=nextjs:nodejs /app/node_modules ./node_modules
COPY --from=build --chown=nextjs:nodejs /app/package.json ./package.json
# next.config.ts owns the rewrites proxy and is evaluated by `next start` at
# boot, so it must ship with the runner stage.
COPY --from=build --chown=nextjs:nodejs /app/next.config.ts ./next.config.ts

USER nextjs
EXPOSE 3000
CMD ["npm", "run", "start"]
