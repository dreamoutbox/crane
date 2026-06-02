## Add to .bashrc for best DX

```sh
# CF TOKEN
export CLOUDFLARE_TOKEN="TOK HERE"

# Minio
export S3_ACCESS_KEY_ID="miniominio"
export S3_SECRET_ACCESS_KEY="miniominio"

#VPS sudo
export SUDO_PASS_VPS1="cranepass"
export SUDO_PASS_VPS2="cranepass"
export SUDO_PASS_VPS3="cranepass"

logetcd() { [[ -z "$1" ]] && { echo "Usage: logetcd <n>"; return 1; }; docker exec "vps${1}" journalctl -xeu etcd -n 100 -ocat; }

logpg()   { [[ -z "$1" ]] && { echo "Usage: logpg <n>"; return 1; }; docker exec "vps${1}" journalctl -xeu patroni.service -n 100 -ocat; }

alias ptl="docker exec -u postgres vps1 patronictl -c /etc/patroni/config.yml list"
```
