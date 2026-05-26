#!/bin/sh

docker exec -i -e PGPASSWORD=u2 vps1 psql -U u2 -d myapp <<EOF
delete from audit_test; 
drop table audit_test;
EOF
