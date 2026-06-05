## Add to .bashrc for best DX

```sh
# CF TOKEN
export CLOUDFLARE_TOKEN="TOK HERE"

# Minio
export S3_ACCESS_KEY_ID="miniominio"
export S3_SECRET_ACCESS_KEY="miniominio"

#VPS sudo pass
export SUDO_PASS_VPS1="cranepass"
export SUDO_PASS_VPS2="cranepass"
export SUDO_PASS_VPS3="cranepass"

# LOG WRAPPER ALIAS COMMANDS
logetcd() { [[ -z "$1" ]] && { echo "Usage: logetcd <n>"; return 1; }; docker exec "vps${1}" journalctl -xeu etcd -n 200 --no-pager; }
logpt()   { [[ -z "$1" ]] && { echo "Usage: logpt <n>"; return 1; }; docker exec "vps${1}" journalctl -xeu patroni.service -n 200 --no-pager; }

# PATRONI LIST WRAPPER
alias ptl="docker exec -u postgres vps1 patronictl -c /etc/patroni/config.yml list"

# VPS CONTAINER EXECUTE WRAPPER COMMANDS
vps1() {
    docker exec vps1 "$@"
}

vps2() {
    docker exec vps2 "$@"
}

vps3() {
    docker exec vps3 "$@"
}

```
