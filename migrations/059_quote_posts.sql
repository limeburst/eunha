-- Migration 059: Add quote post support to statuses
ALTER TABLE statuses ADD COLUMN quote_of_id BIGINT REFERENCES statuses(id) ON DELETE SET NULL;
ALTER TABLE statuses ADD COLUMN quotes_count BIGINT NOT NULL DEFAULT 0;

CREATE INDEX statuses_quote_of_id_idx ON statuses (quote_of_id) WHERE quote_of_id IS NOT NULL;
