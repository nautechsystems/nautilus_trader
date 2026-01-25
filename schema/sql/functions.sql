CREATE OR REPLACE FUNCTION get_all_tables ()
    RETURNS TEXT[] AS $$
DECLARE
    result TEXT[];
BEGIN
    SELECT
        array_agg(t.table_name) INTO result
    FROM information_schema.tables t
    WHERE table_schema = current_schema();
    RETURN result;
END
$$ LANGUAGE plpgsql;

CREATE OR REPLACE function truncate_all_tables()
    RETURNS VOID AS $$
DECLARE
    tables TEXT[];
    quoted_tables TEXT[];
    truncate_statement TEXT;
BEGIN
    SELECT get_all_tables() INTO tables;

    -- Quote each table name
    SELECT array_agg(quote_ident(t)) INTO quoted_tables FROM unnest(tables) AS t;

    -- Construct the TRUNCATE statement
    truncate_statement := 'TRUNCATE TABLE ' || array_to_string(quoted_tables, ', ') || ' CASCADE';

    -- Execute the TRUNCATE statement
    EXECUTE truncate_statement;
END
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION get_last_continuous_block(blockchain_id INTEGER)
  RETURNS BIGINT AS $$
  DECLARE
      min_block BIGINT;
      max_block BIGINT;
      block_count BIGINT;
  BEGIN
      -- Fast: Get MAX using index
      SELECT number INTO max_block FROM block WHERE chain_id = blockchain_id ORDER BY number DESC LIMIT 1;
      -- If no blocks
      IF max_block IS NULL THEN
          RETURN 0;
      END IF;

      -- Fast: Get MIN using index
      SELECT number INTO min_block FROM block WHERE chain_id = blockchain_id ORDER BY number ASC LIMIT 1;

      -- Slower but necessary: Get COUNT
      SELECT COUNT(*) INTO block_count
      FROM block
      WHERE chain_id = blockchain_id;

      -- If continuous: count should equal (max - min + 1)
      IF block_count = (max_block - min_block + 1) THEN
          RETURN max_block;  -- No gaps, return max
      ELSE
          -- Only if gaps exist, use slower gap detection
          RETURN (SELECT COALESCE(
              (SELECT CASE
                  WHEN gap_start = 1 THEN 0
                  ELSE gap_start - 1
               END
               FROM (
                   SELECT number + 1 AS gap_start
                   FROM (
                       SELECT number,
                              LEAD(number) OVER (ORDER BY number) AS next_number
                       FROM block
                       WHERE chain_id = blockchain_id
                   ) gaps
                   WHERE next_number != number + 1
                   ORDER BY number
                   LIMIT 1
               ) first_gap),
              max_block,
              0
          ));
      END IF;
  END
  $$ LANGUAGE plpgsql;