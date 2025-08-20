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

CREATE OR REPLACE FUNCTION create_block_partition(blockchain_id INTEGER)
    RETURNS TEXT AS $$
DECLARE
    blockchain_name TEXT;
    partition_name TEXT;
BEGIN
    -- Get the blockchain name from the chain table
    SELECT lower(name) INTO blockchain_name FROM chain where chain.chain_id = blockchain_id;
    -- Check if blockchain was found
    IF blockchain_name IS NULL THEN
        RETURN 'Chain ID ' || blockchain_id || ' not found';
    END IF;

    partition_name := 'block_' || blockchain_name;

    -- Check if partition already exists
    IF EXISTS(
        SELECT 1 FROM pg_class c
                          JOIN pg_namespace ON pg_namespace.oid = c.relnamespace
        WHERE c."relkind" = 'r' AND c."relname" = partition_name
    ) THEN
        RETURN 'Partition ' || partition_name || ' already exists';
    END IF;

    -- Create the partition
    BEGIN
        EXECUTE format('CREATE TABLE %I PARTITION OF block FOR VALUES IN (%s)', partition_name, blockchain_id);
        RETURN 'Created partition ' || partition_name;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN 'Failed to create partition ' || partition_name || ': ' || SQLERRM;
    END;
END
$$ LANGUAGE plpgsql SECURITY DEFINER;

CREATE OR REPLACE FUNCTION delete_block_partition(blockchain_id INTEGER, force_delete BOOLEAN DEFAULT FALSE)
    RETURNS TEXT AS $$
DECLARE
    blockchain_name TEXT;
    partition_name TEXT;
    data_count BIGINT;
BEGIN
    -- Get the blockchain name from the chain table
    SELECT lower(name) INTO blockchain_name FROM chain where chain.chain_id = blockchain_id;

    -- Check if a blockchain was found
    IF blockchain_name IS NULL THEN
        RETURN 'Chain ID ' || blockchain_id || ' not found';
    END IF;

    partition_name := 'block_' || blockchain_name;

    -- Check if partition exists
    IF NOT EXISTS(
        SELECT 1 FROM pg_class c
                          JOIN pg_namespace ON pg_namespace.oid = c.relnamespace
        WHERE c."relkind" = 'r' AND c."relname" = partition_name
    ) THEN
        RETURN 'Partition ' || partition_name || ' does not exist';
    END IF;

    -- Check if partition has data
    EXECUTE format('SELECT COUNT(*) FROM %I', partition_name) INTO data_count;

    IF data_count > 0 AND NOT force_delete THEN
        RETURN format('Partition %s contains %s blocks. Use force=true to delete anyway', partition_name, data_count);
    END IF;

    -- Actually delete the partition and all data
    BEGIN
        -- WARNING: CASCADE will also delete all dependent blockchain data from other tables
        -- This permanently removes ALL blocks and related transaction data for this chain
        EXECUTE format('DROP TABLE %I CASCADE', partition_name);
        RETURN format('Successfully dropped partition %s (contained %s blocks)', partition_name, data_count);
    EXCEPTION
        WHEN OTHERS THEN
            RETURN 'Failed to drop partition ' || partition_name || ': ' || SQLERRM;
    END;
END
$$ LANGUAGE plpgsql SECURITY DEFINER;

CREATE OR REPLACE FUNCTION create_token_partition(blockchain_id INTEGER)
    RETURNS TEXT AS $$
DECLARE
    blockchain_name TEXT;
    partition_name TEXT;
BEGIN
    -- Get the blockchain name from the chain table
    SELECT lower(name) INTO blockchain_name FROM chain where chain.chain_id = blockchain_id;
    -- Check if blockchain was found
    IF blockchain_name IS NULL THEN
        RETURN 'Chain ID ' || blockchain_id || ' not found';
    END IF;

    partition_name := 'token_' || blockchain_name;

    -- Check if partition already exists
    IF EXISTS(
        SELECT 1 FROM pg_class c
                          JOIN pg_namespace ON pg_namespace.oid = c.relnamespace
        WHERE c."relkind" = 'r' AND c."relname" = partition_name
    ) THEN
        RETURN 'Partition ' || partition_name || ' already exists';
    END IF;

    -- Create the partition
    BEGIN
        EXECUTE format('CREATE TABLE %I PARTITION OF token FOR VALUES IN (%s)', partition_name, blockchain_id);
        RETURN 'Created partition ' || partition_name;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN 'Failed to create partition ' || partition_name || ': ' || SQLERRM;
    END;
END
$$ LANGUAGE plpgsql SECURITY DEFINER;

CREATE OR REPLACE FUNCTION delete_token_partition(blockchain_id INTEGER, force_delete BOOLEAN DEFAULT FALSE)
    RETURNS TEXT AS $$
DECLARE
    blockchain_name TEXT;
    partition_name TEXT;
    data_count BIGINT;
BEGIN
    -- Get the blockchain name from the chain table
    SELECT lower(name) INTO blockchain_name FROM chain where chain.chain_id = blockchain_id;

    -- Check if a blockchain was found
    IF blockchain_name IS NULL THEN
        RETURN 'Chain ID ' || blockchain_id || ' not found';
    END IF;

    partition_name := 'token_' || blockchain_name;

    -- Check if partition exists
    IF NOT EXISTS(
        SELECT 1 FROM pg_class c
                          JOIN pg_namespace ON pg_namespace.oid = c.relnamespace
        WHERE c."relkind" = 'r' AND c."relname" = partition_name
    ) THEN
        RETURN 'Partition ' || partition_name || ' does not exist';
    END IF;

    -- Check if partition has data
    EXECUTE format('SELECT COUNT(*) FROM %I', partition_name) INTO data_count;

    IF data_count > 0 AND NOT force_delete THEN
        RETURN format('Partition %s contains %s tokens. Use force=true to delete anyway', partition_name, data_count);
    END IF;

    -- Actually delete the partition and all data
    BEGIN
        -- WARNING: CASCADE will also delete all dependent token data from other tables
        -- This permanently removes ALL tokens and related data for this chain
        EXECUTE format('DROP TABLE %I CASCADE', partition_name);
        RETURN format('Successfully dropped partition %s (contained %s tokens)', partition_name, data_count);
    EXCEPTION
        WHEN OTHERS THEN
            RETURN 'Failed to drop partition ' || partition_name || ': ' || SQLERRM;
    END;
END
$$ LANGUAGE plpgsql SECURITY DEFINER;

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