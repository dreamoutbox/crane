#!/bin/sh

docker exec -i -e PGPASSWORD=u2 vps1 psql -U u2 -d mydb <<EOF
SELECT table_name 
FROM information_schema.tables 
WHERE table_schema = 'public' 
AND table_type = 'BASE TABLE';
EOF
