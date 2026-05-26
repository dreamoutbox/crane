#!/bin/sh

docker exec -i -e PGPASSWORD=u2 vps1 psql -U u2 -d myapp <<EOF
SELECT * FROM audit_test;
EOF
