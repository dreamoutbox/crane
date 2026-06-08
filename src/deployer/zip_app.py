import os, sys, zipfile

zip_path = sys.argv[1]
deploy_dir = sys.argv[2]
ignores = sys.argv[3:]

with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
    for root, dirs, files in os.walk(deploy_dir):
        dirs[:] = [d for d in dirs if d not in ignores and not d.startswith('.')]
        for file in files:
            if file.startswith('.'):
                continue

            file_path = os.path.join(root, file)
            rel_path = os.path.relpath(file_path, deploy_dir)
            is_ignored = False

            for ig in ignores:
                if rel_path == ig or rel_path.startswith(ig + os.sep) or os.path.basename(rel_path) == ig:
                    is_ignored = True
                    break

            if not is_ignored:
                zipf.write(file_path, rel_path)
