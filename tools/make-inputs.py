"""Author a new capsule from already installed, checksum-verified locked inputs.

Git and Cargo are authoring prerequisites only; input_custody.py restores with
Python's standard library, without Git, network or a pre-populated Cargo cache.
"""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import tomllib
import zipfile

from input_custody import (ROOT, cargo_config, crate_files, destination, digest,
                           git_identity, git_object, grants, inventory, overlaps,
                           source_tree, verify)


def zip_bytes(files):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as archive:
        for name,(data,mode) in sorted(files.items()):
            info = zipfile.ZipInfo(name,(1980,1,1,0,0,0))
            info.create_system = 3
            info.external_attr = mode << 16
            archive.writestr(info,data,compress_type=zipfile.ZIP_DEFLATED,compresslevel=9)
    return stream.getvalue()


def git_files(package):
    directory = Path(package['manifest_path']).parent
    def git(*args):
        return subprocess.check_output(['git',*args],cwd=directory)
    repo = Path(git('rev-parse','--show-toplevel').decode().strip())
    if repo.resolve() != directory.resolve():
        raise ValueError('Nested Git workspace packages need an explicit restore policy')
    url,commit = git_identity(package['source'])
    if git('rev-parse','HEAD').decode().strip() != commit:
        raise ValueError('Installed Git input differs from lock')
    files = {}
    for row in git('ls-tree','-r','-z',commit).split(b'\0'):
        if not row:
            continue
        header,name = row.split(b'\t',1)
        mode,kind,oid = header.decode().split()
        if kind != 'blob' or mode not in ('100644','100755'):
            raise ValueError('Git inputs must contain regular files, without submodules')
        data = git('cat-file','blob',oid)
        if git_object('blob',data) != oid:
            raise ValueError('Original Git blob differs')
        files[name.decode()] = (data,int(mode,8))
    tree = git('rev-parse',commit+'^{tree}').decode().strip()
    if source_tree(files) != tree:
        raise ValueError('Original Git tree differs')
    return files,dict(commit=commit,tree=tree,commit_bytes_hex=git('cat-file','commit',commit).hex())


def author(metadata, lock):
    locked = {(p['name'],p['version'],p['source']):p for p in tomllib.loads(lock.decode())['package'] if p.get('source')}
    selected = [p for p in metadata['packages']if p.get('source')]
    if {(p['name'],p['version'],p['source'])for p in selected} != set(locked):
        raise ValueError('Metadata and lock package closure differ')
    packages = []
    archives = {}
    for package in sorted(selected,key=lambda p:(p['name'],p['version'],p['source'])):
        name,version,source = package['name'],package['version'],package['source']
        label = name+'-'+version
        record = dict(name=name,version=version,source=source)
        if source == 'registry+https://github.com/rust-lang/crates.io-index':
            directory = Path(package['manifest_path']).parent
            path = directory.parents[2]/'cache'/directory.parent.name/(label+'.crate')
            raw = path.read_bytes()
            checksum = locked[(name,version,source)]['checksum']
            if digest(raw) != checksum:
                raise ValueError('Cached original registry archive differs')
            files = crate_files(raw,label)
            record.update(checksum=checksum,archive='registry/'+label+'.crate')
        elif source.startswith('git+'):
            files,identity = git_files(package)
            raw = zip_bytes(files)
            record.update(identity,archive='git/'+label+'-'+identity['commit']+'.zip')
        else:
            raise ValueError('Unsupported source kind')
        record.update(files=inventory(files),grants=grants(files))
        packages.append(record)
        archives[record['archive']] = (raw,0o100644)
    canonical = lock.replace(b'\r\n',b'\n')
    manifest = dict(schema='lattice.cargo-inputs.v1',lock_sha256=digest(canonical),lock_git_blob=git_object('blob',canonical),
                    packages=packages,archives=inventory(archives),
                    scope='Complete original registry archives and Git file trees, without Cargo vendor filtering or Git manifest rewriting. All inputs retain their original grants; no installed compiler/browser/bindgen is redistributed.')
    contents = dict(archives)
    contents['INPUT-MANIFEST.json'] = ((json.dumps(manifest,indent=2,ensure_ascii=False)+'\n').encode(),0o100644)
    raw = zip_bytes(contents)
    verified = verify(raw,digest(raw),lock)
    return raw,verified


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'cargo-inputs/inputs.zip')
    args = parser.parse_args()
    output = destination(args.output)
    targets = [ROOT/'target']
    if os.environ.get('CARGO_TARGET_DIR'):
        targets.append(Path(os.environ['CARGO_TARGET_DIR']).resolve())
    if any(overlaps(output,t)for t in targets):
        raise ValueError('Do not author into compiled targets')
    metadata = json.loads(subprocess.check_output(['cargo','metadata','--locked','--offline','--all-features','--format-version','1'],cwd=ROOT))
    raw,verified = author(metadata,(ROOT/'Cargo.lock').read_bytes())
    output.parent.mkdir(parents=True,exist_ok=True)
    with output.open('xb') as stream:
        stream.write(raw)
    print(json.dumps(dict(path=str(output),sha256=digest(raw),bytes=len(raw),packages=len(verified['packages']),
                          original_files=sum(len(p['files'])for p in verified['packages'].values()),
                          original_notices_bytes=len(verified['notices']))))


if __name__ == '__main__':
    main()
