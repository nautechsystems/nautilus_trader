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