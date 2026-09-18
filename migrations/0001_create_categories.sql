-- 0001: categories — taxonomy root projection (DM-1 table 3).
CREATE TABLE categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    icon TEXT,
    order_index INTEGER NOT NULL
);
