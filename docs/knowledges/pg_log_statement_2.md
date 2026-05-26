## 3. Per-Role or Per-Database Logging (Targeted)

If `log_statement = 'all'` globally is too noisy, override per role:

```sql
-- Log all statements for a specific user only
ALTER ROLE myapp SET log_statement = 'mod';

-- Log all statements for a specific database only
ALTER DATABASE mydb SET log_statement = 'mod';

-- Log all statements for a specific user on a specific database
ALTER ROLE myapp IN DATABASE mydb SET log_statement = 'mod';
```

Useful for auditing a specific app user without flooding logs with other traffic.

---


### Create a foreign table to query CSV logs

```sql
-- PG14 and earlier: use COPY or foreign table
CREATE TABLE pg_log_csv (
  log_time               timestamp with time zone,
  user_name              text,
  database_name          text,
  process_id             integer,
  connection_from        text,
  session_id             text,
  session_line_num       bigint,
  command_tag            text,
  session_start_time     timestamp with time zone,
  virtual_transaction_id text,
  transaction_id         bigint,
  error_severity         text,
  sql_state_code         text,
  message                text,
  detail                 text,
  hint                   text,
  internal_query         text,
  internal_query_pos     integer,
  context                text,
  query                  text,
  query_pos              integer,
  location               text,
  application_name       text,
  backend_type           text,       -- PG13+
  leader_pid             integer,    -- PG14+
  query_id               bigint      -- PG14+
) ;

-- Load today's CSV log
COPY pg_log_csv FROM '/var/lib/postgresql/data/log/postgresql-2024-06-01.csv' CSV;

-- Query DML operations
SELECT
  log_time,
  user_name,
  database_name,
  connection_from,
  message AS statement
FROM pg_log_csv
WHERE error_severity = 'LOG'
  AND message ~* '^\s*statement:.*\b(DELETE|UPDATE|TRUNCATE)\b'
ORDER BY log_time DESC;
```

---

## 8. Querying Logs: JSON Format (PG15+, Best Option)

```ini
# postgresql.conf
log_destination = 'jsonlog'
```

Each log entry is one JSON object per line:

```json
{"timestamp":"2024-06-01 14:32:15.123 UTC","pid":12483,"user":"myapp","dbname":"production","appname":"myapp-server","remote_host":"10.0.1.5","command_tag":"DELETE","message":"statement: DELETE FROM orders WHERE 1=1","backend_type":"client backend"}
```
