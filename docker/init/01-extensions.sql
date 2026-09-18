-- Dev database extensions required by the search engine (design D-6).
-- Mounted into the postgres:16-alpine init hook via docker-compose.yml.
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS unaccent;
