Here is a highly scannable cheat sheet for Ubuntu's Uncomplicated Firewall (UFW).
## 🛡️ Status & Control

* Check status: sudo ufw status
* Verbose status: sudo ufw status verbose
* Numbered status: sudo ufw status numbered
* Enable UFW: sudo ufw enable
* Disable UFW: sudo ufw disable
* Reset UFW: sudo ufw reset

## ⚙️ Default Policies

* Block incoming: sudo ufw default deny incoming
* Allow outgoing: sudo ufw default allow outgoing

## 🔓 Allow Traffic

* By service: sudo ufw allow ssh
* By port: sudo ufw allow 80
* By protocol: sudo ufw allow 443/tcp
* By port range: sudo ufw allow 3000:3005/tcp

## 🔒 Block Traffic

* By service: sudo ufw deny ftp
* By port: sudo ufw deny 21
* By protocol: sudo ufw deny 23/tcp

## 🌐 IP & Network Rules

* Allow specific IP: sudo ufw allow from 192.168.1.50
* Block specific IP: sudo ufw deny from 203.0.113.5
* Allow subnet: sudo ufw allow from 192.168.1.0/24
* Allow IP to port: sudo ufw allow from 192.168.1.50 to any port 22
* Allow network interface: sudo ufw allow in on eth0 to any port 80

## ❌ Delete Rules

* By exact syntax: sudo ufw delete allow 80/tcp
* By rule number: sudo ufw delete 3 (Find number via status numbered)

## 📊 Logs & Apps

* Enable logs: sudo ufw logging on
* Disable logs: sudo ufw logging off
* List app profiles: sudo ufw app list
* Allow app profile: sudo ufw allow 'Nginx Full'

To help you optimize this setup, what specific application or server role (e.g., web server, database, VPN) are you configuring?
