import sys
import json
import os
from datetime import datetime
import boto3
from botocore.client import Config

def parse_toml(content):
    backups = []
    current = {}
    for line in content.splitlines():
        line = line.split('#')[0].strip()
        if not line:
            continue
        if line == '[[backups]]':
            if current:
                backups.append(current)
                current = {}
        elif '=' in line:
            k, v = line.split('=', 1)
            k = k.strip()
            v = v.strip()
            if v.startswith('"') and v.endswith('"'):
                v = v[1:-1]
            elif v.startswith("'") and v.endswith("'"):
                v = v[1:-1]
            elif v == 'null':
                v = None
            current[k] = v
    if current:
        backups.append(current)
    return {'backups': backups}

def dump_toml(registry):
    lines = []
    for b in registry.get('backups', []):
        lines.append('[[backups]]')
        lines.append(f'id = "{b["id"]}"')
        lines.append(f'date = "{b["date"]}"')
        lines.append(f'time = "{b["time"]}"')
        lines.append(f'backup_type = "{b["backup_type"]}"')
        if b.get('base') and b['base'] not in ('None', 'null', None):
            lines.append(f'base = "{b["base"]}"')
        lines.append(f'local_path = "{b["local_path"]}"')
        lines.append(f's3_path = "{b["s3_path"]}"')
        lines.append('')
    return '\n'.join(lines)

def dump_metadata_toml(meta):
    lines = []
    lines.append(f'id = "{meta["id"]}"')
    lines.append(f'date = "{meta["date"]}"')
    lines.append(f'time = "{meta["time"]}"')
    lines.append(f'backup_type = "{meta["backup_type"]}"')
    if meta.get('base') and meta['base'] not in ('None', 'null', None):
        lines.append(f'base = "{meta["base"]}"')
    lines.append(f'local_path = "{meta["local_path"]}"')
    lines.append(f's3_path = "{meta["s3_path"]}"')
    return '\n'.join(lines)

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 postgres-backup.py [full|incr]")
        sys.exit(1)
    
    backup_type = sys.argv[1].lower()
    if backup_type not in ('full', 'incr', 'incremental'):
        print("Invalid backup type. Choose 'full' or 'incr'")
        sys.exit(1)

    is_incr = backup_type in ('incr', 'incremental')

    config_path = '/etc/crane/postgres-backup-config.json'
    if not os.path.exists(config_path):
        print(f"Error: Config file not found at {config_path}")
        sys.exit(1)

    with open(config_path, 'r') as f:
        config = json.load(f)

    # Assert that localhost is the primary node
    import subprocess
    try:
        is_recovery = subprocess.check_output(
            ["sudo", "-u", "postgres", "psql", "-t", "-A", "-c", "SELECT pg_is_in_recovery();"],
            text=True
        ).strip()
        if is_recovery == 't':
            print("Error: localhost is not the primary node (in recovery mode). Backups can only be run on the primary node.")
            sys.exit(1)
        elif is_recovery != 'f':
            print(f"Error: Unexpected response from pg_is_in_recovery(): {is_recovery}")
            sys.exit(1)
    except Exception as e:
        print(f"Error: Failed to verify if database is primary: {e}")
        sys.exit(1)

    s3_opts = {
        'region_name': config.get('region', 'us-east-1'),
        'aws_access_key_id': config['access_key'],
        'aws_secret_access_key': config['secret_key'],
        'config': Config(signature_version='s3v4')
    }
    if config.get('endpoint'):
        s3_opts['endpoint_url'] = config['endpoint']

    s3 = boto3.client('s3', **s3_opts)
    bucket = config['bucket']

    now = datetime.now()
    timestamp_ms = now.strftime('%Y%m%d%H%M%S%f')[:-3]
    date_str = now.strftime('%Y-%m-%d')
    time_str = now.strftime('%H:%M:%S')

    local_path = f"/var/lib/postgresql/backups/{timestamp_ms}"
    pg_version = config['pg_version']
    replica_pass = config['replica_pass']

    os.makedirs("/var/lib/postgresql/backups", exist_ok=True)
    os.system("chown postgres:postgres /var/lib/postgresql/backups")
    os.system("chmod 755 /var/lib/postgresql/backups")
    os.system(f"sudo -u postgres mkdir -p {local_path}")

    os.system("sudo -u postgres psql -c \"GRANT pg_read_server_files TO replicator;\"")

    registry = {'backups': []}
    try:
        resp = s3.get_object(Bucket=bucket, Key="backups/registry.toml")
        registry = parse_toml(resp['Body'].read().decode('utf-8'))
    except Exception as e:
        print("No existing registry found or error reading registry. Starting fresh.")

    last_backup = None
    if registry.get('backups'):
        last_backup = registry['backups'][-1]

    base_id = None
    if is_incr:
        if last_backup:
            base_id = last_backup['id']
            parent_manifest = f"/var/lib/postgresql/backups/{base_id}/backup_manifest"
            if not os.path.exists(parent_manifest):
                os.system(f"sudo -u postgres mkdir -p /var/lib/postgresql/backups/{base_id}")
                os.system(f"sudo chmod 755 /var/lib/postgresql/backups/{base_id}")
                s3_key = f"backups/{base_id}/backup_manifest"
                try:
                    manifest_resp = s3.get_object(Bucket=bucket, Key=s3_key)
                    manifest_content = manifest_resp['Body'].read()
                    with open(parent_manifest, 'wb') as f_out:
                        f_out.write(manifest_content)
                    os.system(f"chown postgres:postgres {parent_manifest}")
                    os.system(f"chmod 644 {parent_manifest}")
                except Exception as e:
                    print(f"Error: Failed to fetch parent manifest from S3: {e}")
                    sys.exit(1)
        else:
            print("Error: Cannot perform incremental backup: no previous backup found.")
            sys.exit(1)

    pg_basebackup_path = f"/usr/lib/postgresql/{pg_version}/bin/pg_basebackup"
    pgbasebackup_cmd = f"sudo -u postgres PGPASSWORD='{replica_pass}' {pg_basebackup_path} -h localhost -U replicator -D {local_path} -F t -X s -c fast --manifest-checksums=sha256"
    if is_incr and base_id:
        pgbasebackup_cmd += f" --incremental=/var/lib/postgresql/backups/{base_id}/backup_manifest"

    if is_incr and int(pg_version) >= 17:
        # Force a WAL switch to ensure the active WAL segment is closed and summarized by the walsummarizer.
        os.system("sudo -u postgres psql -c 'SELECT pg_switch_wal();'")
        import time
        time.sleep(1)

    print(f"Running pg_basebackup command: {pgbasebackup_cmd}")
    ret = os.system(pgbasebackup_cmd)
    if ret != 0:
        os.system(f"sudo rm -rf {local_path}")
        print("Error: pg_basebackup failed")
        sys.exit(1)

    pg_verifybackup_path = f"/usr/lib/postgresql/{pg_version}/bin/pg_verifybackup"
    verify_dir = f"/var/lib/postgresql/backups/{timestamp_ms}_verify"
    os.system(f"sudo -u postgres mkdir -p {verify_dir}")
    
    ret_tar = os.system(f"sudo -u postgres tar -xf {local_path}/base.tar -C {verify_dir}")
    if ret_tar != 0:
        os.system(f"sudo rm -rf {verify_dir}")
        os.system(f"sudo rm -rf {local_path}")
        print("Error: Extracting base.tar failed")
        sys.exit(1)

    if os.path.exists(f"{local_path}/pg_wal.tar"):
        os.system(f"sudo -u postgres mkdir -p {verify_dir}/pg_wal")
        os.system(f"sudo -u postgres tar -xf {local_path}/pg_wal.tar -C {verify_dir}/pg_wal/")

    os.system(f"sudo cp {local_path}/backup_manifest {verify_dir}/")
    os.system(f"sudo chown -R postgres:postgres {verify_dir}")

    verify_cmd = f"sudo -u postgres {pg_verifybackup_path} {verify_dir}"
    print(f"Running verifybackup command: {verify_cmd}")
    ret_verify = os.system(verify_cmd)
    
    os.system(f"sudo rm -rf {verify_dir}")

    if ret_verify != 0:
        os.system(f"sudo rm -rf {local_path}")
        print("Error: pg_verifybackup verification failed")
        sys.exit(1)

    os.system(f"sudo chmod -R 755 {local_path}")

    try:
        files = os.listdir(local_path)
        for filename in files:
            filepath = os.path.join(local_path, filename)
            s3_key = f"backups/{timestamp_ms}/{filename}"
            print(f"Uploading {filepath} to S3 bucket {bucket} key {s3_key}...")
            s3.upload_file(filepath, bucket, s3_key)
    except Exception as e:
        print(f"Error uploading files to S3: {e}")
        sys.exit(1)

    meta = {
        'id': timestamp_ms,
        'date': date_str,
        'time': time_str,
        'backup_type': "INCR" if is_incr else "FULL",
        'base': base_id,
        'local_path': local_path,
        's3_path': f"{bucket}/backups/{timestamp_ms}"
    }
    meta_toml = dump_metadata_toml(meta)

    local_meta_path = f"{local_path}/metadata.toml"
    try:
        with open(local_meta_path, 'w') as f_meta:
            f_meta.write(meta_toml)
        os.system(f"chown postgres:postgres {local_meta_path}")

        s3.put_object(
            Bucket=bucket,
            Key=f"backups/{timestamp_ms}/metadata.toml",
            Body=meta_toml.encode('utf-8')
        )
    except Exception as e:
        print(f"Error writing/uploading metadata: {e}")
        sys.exit(1)

    registry.setdefault('backups', []).append(meta)
    registry_toml = dump_toml(registry)

    try:
        s3.put_object(
            Bucket=bucket,
            Key="backups/registry.toml",
            Body=registry_toml.encode('utf-8')
        )

        local_reg_path = "/var/lib/postgresql/backups/registry.toml"
        with open(local_reg_path, 'w') as f_reg:
            f_reg.write(registry_toml)
        os.system(f"chown postgres:postgres {local_reg_path}")
        os.system(f"chmod 644 {local_reg_path}")
    except Exception as e:
        print(f"Error updating registry: {e}")
        sys.exit(1)

    print("Backup completed successfully.")

if __name__ == '__main__':
    main()
