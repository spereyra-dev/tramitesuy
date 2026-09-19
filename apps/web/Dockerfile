# TrámitesUY web UI image (add-web-frontend, tasks 24; design §7, proposal P4).
# Multi-stage build on node:22-alpine: npm ci + `next build` in the builder,
# then a minimal non-root runner stage serving `next start` on port 3000.
#
# API_BASE_URL is a runtime value, not a build argument: next.config.ts reads
# it when the server boots to evaluate the same-origin rewrite proxy (compose
# sets http://api:8080; dev default http://localhost:8080). The image stays
# env-agnostic.

FROM node:22-alpine AS build
WORKDIR /app

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
