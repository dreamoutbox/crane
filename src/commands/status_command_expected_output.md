# STATUS COMMAND EXPECTED OUTPUT `crane status`

## TODO:

- [ ] if user provides app name, show only that app status
- [ ] if user does not provide app name, show all apps status
    
## EXPECTED OUTPUT
```log
NODE:

vps1
  Public IP: 10.0.0.11
  Internal IP: 10.0.0.11
  SSH Port: 2221
  CPU Usage:  3.5%
  Memory:     5478 MB / 9943 MB (55.1%)
  Disk (/):   29G / 1007G (3%)
  Network:
    - eth0: Rx: 233.48 MB (5.0 KB/s), Tx: 14.50 MB (18.6 KB/s)

vps2
  Public IP: 10.0.0.12
  Internal IP: 10.0.0.12
  SSH Port: 2222
  CPU Usage:  4.4%
  Memory:     5484 MB / 9943 MB (55.2%)
  Disk (/):   29G / 1007G (3%)
  Network:
    - eth0: Rx: 158.09 MB (3.2 KB/s), Tx: 134.33 MB (16.8 KB/s)

vps3
  Public IP: 10.0.0.13
  Internal IP: 10.0.0.13
  SSH Port: 2223
  CPU Usage:  1.2%
  Memory:     5485 MB / 9943 MB (55.2%)
  Disk (/):   29G / 1007G (3%)
  Network:
    - eth0: Rx: 120.34 MB (7.5 KB/s), Tx: 9.39 MB (17.9 KB/s)

APP:

myapp:
  External URL:    https://myapp.localhost
  Port Range:      3000-3100
  Overall Status:  HEALTHY

  myapp@1 (vps1) (Port 3000) Active 200 OK
  myapp@2 (vps2) (Port 3000) Active 200 OK

myapp2:
  External URL:    https://myapp2.localhost
  Port Range:      4000-4100
  Overall Status:  HEALTHY

  myapp2@1 (vps1) (Port 4000) Active 200 OK
  myapp2@2 (vps1) (Port 4001) Active 200 OK
  myapp2@3 (vps2) (Port 4000) Active 200 OK
  myapp2@4 (vps2) (Port 4001) Active 200 OK

```
