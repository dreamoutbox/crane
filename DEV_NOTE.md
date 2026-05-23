# DEV NOTE

# simulate a app instance down

```sh
docker exec vps1 systemctl stop myapp@3000.service
```

# list myapp services

```sh
systemctl list-units | grep myapp
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
systemctl stop myapp@3000.service
```
