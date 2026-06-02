# DEV NOTE

## list myapp services

```sh
docker exec vps1 systemctl list-units | grep myapp
```


## example list services output

```log
root@vps1:/# systemctl list-units | grep myapp
  myapp@3000.service                       loaded active     running   crane managed: myapp instance on port 3000
  myapp@3001.service                       loaded active     running   crane managed: myapp instance on port 3001
  system-myapp.slice                       loaded active     active    Slice /system/myapp
```

## see myapp systemd unit file

```sh
docker exec vps1 cat /etc/systemd/system/myapp@.service
```

## stop service

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

## fix can't ssh into vps
```sh
docker exec vps1 rm -f /run/nologin && docker exec vps1 systemctl restart systemd-user-sessions.service
```

## Get myapp logs on port 3000
```sh
journalctl -xeu myapp@3000.service
```

## Get myapp2 logs on port 4000
```sh
journalctl -xeu myapp2@4000.service
```

