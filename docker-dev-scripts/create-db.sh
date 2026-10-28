#!/bin/sh

docker exec -i -e PGPASSWORD=u2 vps1 psql -U u2 -d myapp <<EOF
create table if not exists audit_test(id serial); 
insert into audit_test default values; 
-- update audit_test set id = 2; 
EOF
