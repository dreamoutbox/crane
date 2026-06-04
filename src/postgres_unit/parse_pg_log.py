import csv
import sys
import re
import argparse
from datetime import datetime

parser = argparse.ArgumentParser()
parser.add_argument('logfile')
parser.add_argument('--since', help='Filter since date (YYYY-MM-DD HH:MM:SS)')
parser.add_argument('--until', help='Filter until date (YYYY-MM-DD HH:MM:SS)')
parser.add_argument('--user', help='Filter by user name')
parser.add_argument('--db', help='Filter by database name')
parser.add_argument('--sql', help='Filter by SQL statement substring')
args = parser.parse_args()

since_dt = None
if args.since:
    try:
        since_dt = datetime.fromisoformat(args.since.replace(' ', 'T'))
    except Exception:
        pass

until_dt = None
if args.until:
    try:
        until_dt = datetime.fromisoformat(args.until.replace(' ', 'T'))
    except Exception:
        pass

with open(args.logfile, 'r', encoding='utf-8', errors='replace') as f:
    reader = csv.reader(f)
    for row in reader:
        if len(row) < 14:
            continue
        
        log_time_str = row[0]
        user_name = row[1]
        database_name = row[2]
        client = row[4]
        severity = row[11]
        message = row[13]
        
        if since_dt or until_dt:
            try:
                dt_part = log_time_str.split('.')[0]
                row_dt = datetime.strptime(dt_part, '%Y-%m-%d %H:%M:%S')
                if since_dt and row_dt < since_dt:
                    continue
                if until_dt and row_dt > until_dt:
                    continue
            except Exception:
                pass
        
        if args.user and args.user.lower() != user_name.lower():
            continue
        if args.db and args.db.lower() != database_name.lower():
            continue
            
        is_statement = severity == 'LOG' and message.startswith('statement: ')
        if is_statement:
            sql = message[len('statement: '):].strip()
            if args.sql and args.sql.lower() not in sql.lower():
                continue
            print(f"{log_time_str} | user={user_name} db={database_name} client={client} | SQL: {sql}")
        else:
            if args.sql:
                continue
            print(f"{log_time_str} | user={user_name} db={database_name} client={client} | [{severity}] {message}")
