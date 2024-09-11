CREATE OR REPLACE FUNCTION get_all_tables ()
    RETURNS TEXT[] AS $$
DECLARE
    result TEXT[];
BEGIN
    SELECT
        array_agg(t.table_name) INTO result
    FROM information_schema.tables t
    LEFT JOIN pg_class pc ON t.table_name = pc.relname
    WHERE table_schema = current_schema();
    RETURN result;
END
$$ LANGUAGE plpgsql;