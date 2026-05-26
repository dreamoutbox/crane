#!/bin/sh

docker exec -i -e PGPASSWORD=u2 vps1 psql -U u2 -d myapp <<EOF
insert into audit_test default values; 
EOF
