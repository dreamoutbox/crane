# Traefik as internal routing

## TODO:

We will modify /etc/hosts to point app instance name -> traefik for erase port specify when a service call another service. and load balancing between app instances.

Traefik already runs on each node. Add a second entrypoint bound to `127.0.0.1` only — no TLS, no external exposure.



**Static config example:**
```yaml
entryPoints:
  web:
    address: ":80"
  websecure:
    address: ":443"
  internal:
    address: "127.0.0.1"   # ← new, localhost only
```

**Dynamic config example:**
```toml
[http.routers.myapp-internal]
  rule = "Host(`myapp`)"
  entryPoints = ["internal"]
  service = "myapp"
  # no TLS

[http.routers.myapp-external]
  rule = "Host(`myapp.com`)"
  entryPoints = ["websecure"]
  service = "myapp"
  [http.routers.myapp-external.tls]
    certResolver = "letsencrypt"

# the url use port_start - port_end in crane.toml
[http.services.myapp.loadBalancer]
  [[http.services.myapp.loadBalancer.servers]]
    url = "http://127.0.0.1:3000"
  [[http.services.myapp.loadBalancer.servers]]
    url = "http://127.0.0.1:3001"
```


**Modify `/etc/hosts` (crane writes this):**
```
127.0.0.1  myapp
127.0.0.1  auth-service
127.0.0.1  worker
```

## Expected behavior:

App calls `http://auth-service/verify` → Traefik routes + load balances across auth-service instances. No port knowledge needed by callers.

No port specified by the caller — just the service name. Traefik handles the rest.
