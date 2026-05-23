# Traefik debug command

## Follow Redirects during Testing:

When testing HTTP endpoints, use the -L (follow redirects) and -k (insecure TLS) flags:

```shell
curl -L -k -i myapp.localhost/health
```


## default config
cat /etc/traefik/traefik.yml

## list dynamic config
ls -la /etc/traefik/dynamic

## read specific config
cat /etc/traefik/dynamic/myapp.toml

## example myapp.toml

```toml
[http.routers.myapp]
  rule = "Host(`myapp.localhost`)"
  service = "myapp"
  [http.routers.myapp.tls]
    certResolver = "letsencrypt"

[http.services.myapp.loadBalancer]
  [[http.services.myapp.loadBalancer.servers]]
    url = "http://127.0.0.1:3000"
  [[http.services.myapp.loadBalancer.servers]]
    url = "http://127.0.0.1:3001"
```
