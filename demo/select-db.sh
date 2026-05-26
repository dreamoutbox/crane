#!/bin/sh

docker exec -i -e PGPASSWORD=u2 vps1 psql -U u2 -d myapp <<EOF
SELECT * FROM audit_test;
EOF

echo ""

docker exec -i -e PGPASSWORD=u2 vps1 psql -U u2 -d myapp <<EOF
SELECT * FROM integrated_test_table ORDER BY id;
EOF
