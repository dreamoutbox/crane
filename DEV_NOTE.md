# DEV NOTE

# simulate a app instance down

```sh
docker exec vps1 systemctl stop myapp@3000.service
```

# list myapp services

```sh
docker exec vps1 systemctl list-units | grep myapp
```

# example list services output

```log
root@vps1:/# systemctl list-units | grep myapp
  myapp@3000.service                       loaded active     running   crane managed: myapp instance on port 3000
  myapp@3001.service                       loaded active     running   crane managed: myapp instance on port 3001
  system-myapp.slice                       loaded active     active    Slice /system/myapp
```

# stop service

```sh
docker exec vps1 systemctl stop myapp@3000.service
```

## Testing service name

```sh
# myapp → hits http://myapp2 (via /etc/hosts)
curl -w "\n" -L -k -i myapp.localhost/curl?to=myapp2


# myapp2 → hits http://myapp
curl -w "\n" -L -k -i myapp2.localhost/curl?to=myapp
```

