# PostgreSQL: Logging & Querying DML Operations (UPDATE, DELETE)

---

## 1. Configure Logging

### `postgresql.conf`

```ini
# Log only DML + DDL (INSERT, UPDATE, DELETE, TRUNCATE, DDL statements)
# Options: none | ddl | mod | all
log_statement = 'mod'

# Include duration of each logged statement (useful for slow query hunting)
log_duration = on

# Minimum statement duration to log (0 = log all, -1 = disable)
# Use this instead of log_duration if you only want slow queries
log_min_duration_statement = 0     # log all; set to e.g. 500 for 500ms+

# Add timestamp, user, db, app name to each log line
log_line_prefix = '%t [%p]: user=%u db=%d app=%a client=%h '
# %t = timestamp, %p = PID, %u = user, %d = database
# %a = application_name, %h = client host

# Log the actual parameters passed to parameterized queries
log_parameter_max_length = -1          # -1 = unlimited (PG13+)
log_parameter_max_length_on_error = -1 # also log params on error

# Where to write logs
log_destination = 'stderr'             # stderr | csvlog | jsonlog | syslog
logging_collector = on                 # must be on to write to files
log_directory = 'log'                  # relative to $PGDATA, or absolute path
log_filename = 'postgresql-%Y-%m-%d.log'
log_rotation_age = 1d
log_rotation_size = 100MB
```

> **`log_statement` values:**
> - `none` — log nothing
> - `ddl` — DDL only (CREATE, DROP, ALTER)
> - `mod` — DDL + DML that modifies data (INSERT, UPDATE, DELETE, TRUNCATE, COPY FROM) ← use this
> - `all` — every statement including SELECT (very noisy, high I/O)

### Apply Without Restart (most settings)

```sql
-- Reload config without restarting
SELECT pg_reload_conf();

-- Verify active settings
SHOW log_statement;
SHOW log_line_prefix;
SHOW log_directory;

-- Full path to current log file
SELECT pg_current_logfile();
```

> `logging_collector` requires a **full restart** if changed. Everything else can be reloaded.

---

## 2. Log File Location

```bash
# Find log directory
psql -U postgres -c "SHOW log_directory;"
psql -U postgres -c "SELECT pg_current_logfile();"

# Common paths
ls /var/lib/postgresql/*/main/log/          # Debian/Ubuntu (pg_ctlcluster)
ls /var/log/postgresql/                     # system-managed
ls $PGDATA/log/                             # generic

# Current active log file
psql -U postgres -tAc "SELECT current_setting('data_directory') || '/' || pg_current_logfile();"
```

---

## 4. What the Log Looks Like

With `log_line_prefix = '%t [%p]: user=%u db=%d app=%a client=%h '`:

```
2024-06-01 14:32:15 UTC [12483]: user=myapp db=production app=myapp-server client=10.0.1.5 LOG:  statement: DELETE FROM orders WHERE 1=1
2024-06-01 14:32:15 UTC [12483]: user=myapp db=production app=myapp-server client=10.0.1.5 LOG:  duration: 342.871 ms
2024-06-01 14:32:17 UTC [12490]: user=admin db=production app=psql client=[local] LOG:  statement: UPDATE users SET active = false WHERE last_login < '2023-01-01'
2024-06-01 14:32:17 UTC [12490]: user=admin db=production app=psql client=[local] LOG:  duration: 88.312 ms
```

---

## 5. Querying Logs: Shell

### Tail live log

```bash
LOG=$(psql -U postgres -tAc "SELECT current_setting('data_directory') || '/' || pg_current_logfile();")
tail -f "$LOG"
```

### Filter only DELETE and UPDATE

```bash
grep -E "LOG:  statement:.*\b(DELETE|UPDATE)\b" "$LOG"
```

### Show timestamp + statement (clean output)

```bash
grep -E "LOG:  statement:.*\b(DELETE|UPDATE|TRUNCATE)\b" "$LOG" \
  | sed 's/ LOG:  statement: /\n  SQL: /g'
```

### Search by time range

```bash
# Everything between 14:30 and 14:35 on June 1
awk '/^2024-06-01 14:3[0-5]/' "$LOG" \
  | grep -E "\b(DELETE|UPDATE)\b"
```

### Search across rotated logs

```bash
grep -hE "LOG:  statement:.*\b(DELETE|UPDATE)\b" \
  /var/lib/postgresql/*/main/log/postgresql-2024-06-*.log \
  | sort
```

### Find which user ran a DELETE

```bash
grep "LOG:  statement:.*DELETE" "$LOG" \
  | grep -oP "user=\K\S+"    # extract user= field
```

---

## 6. Querying Logs: `pg_read_file` (SQL, No Shell Access Needed)

PostgreSQL can read its own log files via SQL if `log_destination = 'stderr'` and `logging_collector = on`.

```sql
-- Read last 50KB of current log
SELECT pg_read_file(pg_current_logfile(), 0, 51200);
```

Parse line by line and filter:

```sql
SELECT
  line_number,
  line
FROM regexp_split_to_table(
  pg_read_file(pg_current_logfile()),
  E'\n'
) WITH ORDINALITY AS t(line, line_number)
WHERE line ~* '\b(DELETE|UPDATE|TRUNCATE)\b'
  AND line ~ 'LOG:  statement:';
```

With timestamp extraction:

```sql
SELECT
  (regexp_match(line, '^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})'))[1]  AS ts,
  (regexp_match(line, 'user=(\S+)'))[1]                                AS username,
  (regexp_match(line, 'db=(\S+)'))[1]                                  AS database,
  (regexp_match(line, 'client=(\S+)'))[1]                              AS client_ip,
  (regexp_match(line, 'statement: (.+)$'))[1]                          AS statement
FROM regexp_split_to_table(
  pg_read_file(pg_current_logfile()),
  E'\n'
) AS t(line)
WHERE line ~* 'LOG:  statement:.*\b(DELETE|UPDATE|TRUNCATE)\b';
```

---


## 7. Querying Logs: CSV Format (Best for SQL Queries)

Switch to CSV logging for structured, easily queryable output.

### Enable CSV logging

```ini
# postgresql.conf
log_destination = 'csvlog'   # writes both stderr + CSV
logging_collector = on
log_filename = 'postgresql-%Y-%m-%d'   # no extension — PG adds .log and .csv
```

```sql
SELECT pg_reload_conf();
```

---

### Query with `jq`

```bash
LOG_JSON="${LOG%.log}.json"    # same name, .json extension

# All DELETE/UPDATE with timestamp and user
jq -r 'select(.message | test("\\b(DELETE|UPDATE|TRUNCATE)\\b"; "i"))
  | [.timestamp, .user, .dbname, .remote_host, .message]
  | @tsv' "$LOG_JSON"

# Only statements after a specific time
jq -r 'select(.timestamp >= "2024-06-01 14:00:00")
  | select(.message | test("\\b(DELETE|UPDATE)\\b"; "i"))
  | [.timestamp, .user, .message] | @tsv' "$LOG_JSON"

# Count deletes per user today
jq -r 'select(.message | test("^statement: DELETE"; "i")) | .user' "$LOG_JSON" \
  | sort | uniq -c | sort -rn
```

---