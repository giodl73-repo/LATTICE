"""Verify original locked Rust archives and restore new offline source folders.

Python 3.11+ standard library only. A trusted external SHA anchors integrity,
not authorship. Verification does not execute downloaded source or acquire tools.
"""
import argparse
import hashlib
import gzip
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import tarfile
import tomllib
from urllib.parse import parse_qs, urlsplit, urlunsplit
import unicodedata
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MAX_ARCHIVE_BYTES = 8_000_000
MAX_SOURCE_BYTES = 128_000_000
MAX_FILES = 10000
MAX_TAR_BYTES = 160_000_000
NOTICE_NAMES = ('LICENSE', 'LICENCE', 'COPYING', 'COPYRIGHT', 'NOTICE')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git_object(kind, data):
    return hashlib.sha1(kind.encode()+b' '+str(len(data)).encode()+b'\0'+data).hexdigest()


def strict_json(data):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('Duplicate JSON key')
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=pairs)


def safe_path(name):
    if not isinstance(name, str):
        raise ValueError('Path must be text')
    path = PurePosixPath(name)
    reserved = {'CON','PRN','AUX','NUL','CONIN$','CONOUT$','CLOCK$'} | {f'{stem}{n}' for stem in ('COM','LPT') for n in [*range(1,10),'¹','²','³']}
    if (not name or name == '.' or str(path) != name or path.is_absolute()
            or len(name.encode('utf-8')) > 4096 or any(len(p.encode('utf-8')) > 255 for p in path.parts)
            or any(ord(c) < 32 or c in '\\:<>"|?*' for c in name)
            or any(p.rstrip(' .') != p or p.split('.')[0].rstrip(' ').upper() in reserved for p in path.parts)):
        raise ValueError('Unsafe path: '+repr(name))
    if any(p in ('.','..') for p in path.parts):
        raise ValueError('Unsafe path component')
    return path


def path_spellings(names):
    spellings = {}
    for name in names:
        safe_path(name)
        parts = PurePosixPath(name).parts
        for count in range(1,len(parts)+1):
            prefix = '/'.join(parts[:count])
            key = unicodedata.normalize('NFC',prefix).casefold()
            if key in spellings and spellings[key] != prefix:
                raise ValueError('Case/Unicode-aliasing file or directory prefix')
            spellings[key] = prefix


def path_set(names):
    path_spellings(names)
    folded = {n.casefold() for n in names}
    if len(folded) != len(names):
        raise ValueError('Duplicate or case-colliding paths')
    if any(str(parent).casefold() in folded for name in names for parent in PurePosixPath(name).parents if str(parent) != '.'):
        raise ValueError('File/ancestor path collision')


def zip_files(raw):
    if len(raw) > MAX_ARCHIVE_BYTES:
        raise ValueError('Archive byte limit')
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        entries = archive.infolist()
        if len(entries) > MAX_FILES or sum(x.file_size for x in entries) > MAX_SOURCE_BYTES:
            raise ValueError('ZIP resource limit')
        for entry in entries:
            safe_path(entry.orig_filename)
            if entry.orig_filename != entry.filename:
                raise ValueError('ZIP wire name was normalized')
            if entry.is_dir() or entry.external_attr >> 16 not in (0o100644,0o100755):
                raise ValueError('Only regular source modes are supported')
            if entry.flag_bits&1 or entry.compress_type not in (zipfile.ZIP_STORED,zipfile.ZIP_DEFLATED):
                raise ValueError('Encrypted or unsupported ZIP compression')
        path_set([x.filename for x in entries])
        return {x.filename:(archive.read(x),x.external_attr >> 16) for x in entries}


def crate_files(raw, label):
    if len(raw) > MAX_ARCHIVE_BYTES:
        raise ValueError('Crate byte limit')
    files = {}
    total = 0
    directories = set()
    # Bound the decompressed container before tarfile reads extension/PAX
    # headers, whose allocations are otherwise outside ordinary file sizes.
    decoded = bytearray()
    with gzip.GzipFile(fileobj=io.BytesIO(raw),mode='rb') as source:
        while True:
            chunk = source.read(min(65536,MAX_TAR_BYTES-len(decoded)+1))
            if not chunk:
                break
            decoded.extend(chunk)
            if len(decoded) > MAX_TAR_BYTES:
                raise ValueError('Decompressed tar container limit')
    with tarfile.open(fileobj=io.BytesIO(decoded),mode='r:') as archive:
        count = 0
        for entry in archive:
            count += 1
            if count > MAX_FILES:
                raise ValueError('Crate member limit')
            safe_path(entry.name.rstrip('/') if entry.isdir() else entry.name)
            if entry.isdir():
                if entry.name.rstrip('/') != label and not entry.name.startswith(label+'/'):
                    raise ValueError('Crate directory prefix differs')
                relative = entry.name.rstrip('/')[len(label):].lstrip('/')
                if relative:
                    if relative in directories:
                        raise ValueError('Duplicate crate directory')
                    directories.add(relative)
                continue
            if not entry.isfile() or not entry.name.startswith(label+'/') or entry.mode not in (0o644,0o755):
                raise ValueError('Unsupported crate member/type/mode')
            name = entry.name[len(label)+1:]
            safe_path(name)
            total += entry.size
            if total > MAX_SOURCE_BYTES or name in files:
                raise ValueError('Crate source limit or duplicate')
            files[name] = (archive.extractfile(entry).read(),0o100000|entry.mode)
        if any(decoded[archive.offset:]):
            raise ValueError('Unparsed nonzero data after tar end')
    path_set(list(files))
    path_spellings([*files,*directories])
    directory_folded = {d.casefold() for d in directories}
    if len(directory_folded) != len(directories) or directory_folded & {f.casefold()for f in files}:
        raise ValueError('Crate directory/file collision')
    if any(str(parent).casefold() in {f.casefold()for f in files} for name in directories for parent in PurePosixPath(name).parents if str(parent) != '.'):
        raise ValueError('Crate directory/file ancestor collision')
    return files


def inventory(files):
    return [dict(path=name,bytes=len(data),sha256=digest(data),mode=format(mode,'o'))
            for name,(data,mode) in sorted(files.items())]


def source_tree(files):
    root = {}
    for name,(data,mode) in files.items():
        node = root
        parts = safe_path(name).parts
        for part in parts[:-1]:
            node = node.setdefault(part,{})
        node[parts[-1]] = (mode,git_object('blob',data))
    def tree(node):
        records = []
        for name,value in node.items():
            if isinstance(value,dict):
                records.append((name.encode()+b'/',b'40000 '+name.encode()+b'\0'+bytes.fromhex(tree(value))))
            else:
                mode,oid = value
                records.append((name.encode(),format(mode,'o').encode()+b' '+name.encode()+b'\0'+bytes.fromhex(oid)))
        return git_object('tree',b''.join(row for key,row in sorted(records)))
    return tree(root)


def git_identity(source):
    if not source.startswith('git+'):
        raise ValueError('Expected locked Git source')
    url = urlsplit(source[4:])
    query = parse_qs(url.query, keep_blank_values=True, strict_parsing=True)
    if (url.scheme != 'https' or not url.netloc or url.username or url.password
            or any(ord(c)<32 for c in source)
            or set(query) not in ({'rev'}, {'branch'})
            or any(len(values)!=1 for values in query.values())):
        raise ValueError('Only HTTPS Git sources with one locked selector are supported')
    commit = url.fragment
    if not re.fullmatch('[0-9a-f]{40}',commit):
        raise ValueError('Git lock must identify its full immutable commit')
    if 'rev' in query and query['rev'][0] != commit:
        raise ValueError('Git rev must equal its full immutable commit')
    # Preserve this repository's original branch=master lock source identity.
    # Restore never resolves the branch: verify() binds the exact lock entry to
    # original commit/tree/blob/mode bytes before any offline Cargo replacement.
    if 'branch' in query and query['branch'] != ['master']:
        raise ValueError('Only the original locked master branch is supported')
    return urlunsplit((url.scheme,url.netloc,url.path,'','')),commit


def cargo_config(packages, directory='vendor'):
    lines = ['[source.crates-io]','replace-with = "vendored-sources"','']
    for package in sorted((p for p in packages if p['source'].startswith('git+')),key=lambda p:p['source']):
        source = package['source']
        url,commit = git_identity(source)
        selector = 'branch' if 'branch' in parse_qs(urlsplit(source[4:]).query) else 'rev'
        value = 'master' if selector == 'branch' else commit
        lines += ['[source.'+json.dumps(source.split('#')[0])+']',
                  'git = '+json.dumps(url),selector+' = '+json.dumps(value),'replace-with = "vendored-sources"','']
    lines += ['[source.vendored-sources]','directory = '+json.dumps(directory),'','[net]','offline = true','']
    return '\n'.join(lines).encode()


def grants(files):
    result = [r for r in inventory(files) if PurePosixPath(r['path']).name.upper().startswith(NOTICE_NAMES)]
    if not result:
        raise ValueError('Original grants/notices missing')
    return result


def verify(raw, expected_sha, lock):
    if not re.fullmatch('[0-9a-f]{64}',expected_sha) or digest(raw) != expected_sha:
        raise ValueError('Input capsule SHA mismatch')
    data = zip_files(raw)
    manifest = strict_json(data['INPUT-MANIFEST.json'][0])
    if manifest['schema'] != 'lattice.cargo-inputs.v1':
        raise ValueError('Unsupported input schema')
    # Git checkout CRLF is explicitly canonicalized for lock identity only.
    # Original source/archive bytes and restored files are never normalized.
    canonical_lock = lock.replace(b'\r\n',b'\n')
    if digest(canonical_lock) != manifest['lock_sha256'] or git_object('blob',canonical_lock) != manifest['lock_git_blob']:
        raise ValueError('Locked input identity differs')
    locked = {(p['name'],p['version'],p['source']):p for p in tomllib.loads(lock.decode())['package'] if p.get('source')}
    packages = manifest['packages']
    keys = [(p['name'],p['version'],p['source'])for p in packages]
    labels = [p['name']+'-'+p['version']for p in packages]
    path_set(labels)
    if len(set(keys)) != len(keys) or set(keys) != set(locked) or len(set(labels)) != len(labels):
        raise ValueError('Complete locked package closure differs')
    expected_archives = []
    restored = {}
    notices = [b'LATTICE locked build inputs: original notices follow without text normalization.\nIndividual packages retain their own grants; this is not a blanket license.\n']
    source_bytes = 0
    for package in packages:
        name,version,source = package['name'],package['version'],package['source']
        label = name+'-'+version
        safe_path(label)
        if source == 'registry+https://github.com/rust-lang/crates.io-index':
            archive_name = 'registry/'+label+'.crate'
            value = data[archive_name][0]
            checksum = locked[(name,version,source)]['checksum']
            if digest(value) != checksum or package['checksum'] != checksum:
                raise ValueError('Original registry checksum differs')
            files = crate_files(value,label)
        elif source.startswith('git+'):
            url,commit = git_identity(source)
            archive_name = 'git/'+label+'-'+commit+'.zip'
            files = zip_files(data[archive_name][0])
            commit_bytes = bytes.fromhex(package['commit_bytes_hex'])
            tree = source_tree(files)
            if (package['commit'] != commit or git_object('commit',commit_bytes) != commit
                    or commit_bytes.split(b'\n',1)[0] != b'tree '+tree.encode() or package['tree'] != tree):
                raise ValueError('Original Git commit/tree/blob/mode identity differs')
            checksum = None
        else:
            raise ValueError('Unsupported locked source kind')
        if package['archive'] != archive_name or package['files'] != inventory(files) or package['grants'] != grants(files):
            raise ValueError('Original file/mode/grant inventory differs')
        if '.cargo-checksum.json' in files:
            raise ValueError('Original source collides with managed checksum')
        cargo_package = tomllib.loads(files['Cargo.toml'][0].decode())['package']
        if cargo_package['name'] != name or cargo_package['version'] != version:
            raise ValueError('Original Cargo package name/version differs')
        source_bytes += sum(len(v)for v,m in files.values())
        if source_bytes > MAX_SOURCE_BYTES:
            raise ValueError('Complete source resource limit')
        restored[label] = dict(files=files,checksum=checksum)
        expected_archives.append(archive_name)
        for grant in package['grants']:
            notices += [f'\n\n===== {name} {version} / {grant["path"]} =====\n'.encode(),files[grant['path']][0]]
    if set(data) != set(expected_archives)|{'INPUT-MANIFEST.json'} or len(expected_archives) != len(set(expected_archives)):
        raise ValueError('Unreferenced or duplicate capsule entry')
    if manifest['archives'] != inventory({n:data[n]for n in expected_archives}):
        raise ValueError('Original archive inventory differs')
    return dict(manifest=manifest,packages=restored,notices=b''.join(notices),config=cargo_config(packages))


def plain_path(path):
    lexical = Path(os.path.abspath(path))
    if os.name == 'nt' and len(lexical.parts)>1:
        safe_path('/'.join(lexical.parts[1:]))
    # Inspect the uncollapsed spelling first: resolve/abspath can hide links
    # followed by '..'. Both spelling and resolved destination are checked.
    spelling = Path(path) if Path(path).is_absolute() else Path.cwd()/path
    for part in [spelling,*spelling.parents,lexical,*lexical.parents]:
        if part.is_symlink() or (part.exists() and getattr(part.lstat(),'st_file_attributes',0)&0x400):
            raise ValueError('Destination uses a link/junction')
    return lexical.resolve()


def destination(path):
    result = plain_path(path)
    if result.exists():
        raise ValueError('Use a new destination; existing files are preserved')
    return result


def overlaps(a,b):
    return a == b or a in b.parents or b in a.parents


def restore(verified, inputs, cargo_home, protected_targets=(), source_root=ROOT):
    inputs,cargo_home = destination(inputs),destination(cargo_home)
    source_root = plain_path(source_root)
    targets = [source_root,source_root/'target',*[Path(p).resolve()for p in protected_targets]]
    if os.environ.get('CARGO_TARGET_DIR'):
        targets.append(Path(os.environ['CARGO_TARGET_DIR']).resolve())
    if overlaps(inputs,cargo_home) or any(overlaps(p,t)for p in (inputs,cargo_home)for t in targets):
        raise ValueError('Destinations overlap source, compiled target or each other')
    # Every archive and both destinations have been checked before any write.
    inputs.mkdir(parents=True)
    cargo_home.mkdir(parents=True)
    for label,package in verified['packages'].items():
        directory = inputs/'vendor'/label
        checks = {}
        for name,(data,mode) in package['files'].items():
            target = directory/name
            target.parent.mkdir(parents=True,exist_ok=True)
            with target.open('xb') as stream:
                stream.write(data)
            target.chmod(mode&0o777)
            checks[name] = digest(data)
        with (directory/'.cargo-checksum.json').open('xb') as stream:
            stream.write((json.dumps(dict(files=checks,package=package['checksum']),sort_keys=True)+'\n').encode())
        (directory/'.cargo-checksum.json').chmod(0o644)
    config = cargo_config(verified['manifest']['packages'],(inputs/'vendor').as_posix())
    with (cargo_home/'config.toml').open('xb') as stream:
        stream.write(config)
    (cargo_home/'config.toml').chmod(0o644)
    return inputs,cargo_home


def readback(verified, inputs, cargo_home):
    inputs,cargo_home = plain_path(inputs),plain_path(cargo_home)
    plain_path(cargo_home/'config.toml')
    expected = {}
    modes = {}
    for label,package in verified['packages'].items():
        checks = {}
        for name,(data,mode) in package['files'].items():
            expected['vendor/'+label+'/'+name] = data
            modes['vendor/'+label+'/'+name] = mode&0o777
            checks[name] = digest(data)
        expected['vendor/'+label+'/.cargo-checksum.json'] = (json.dumps(dict(files=checks,package=package['checksum']),sort_keys=True)+'\n').encode()
        modes['vendor/'+label+'/.cargo-checksum.json'] = 0o644
    actual = {}
    for path in inputs.rglob('*'):
        if path.is_symlink() or getattr(path.lstat(),'st_file_attributes',0)&0x400:
            raise ValueError('Restored source link/junction')
        if path.is_file():
            relative = path.relative_to(inputs).as_posix()
            actual[relative] = path.read_bytes()
            if os.name != 'nt' and stat.S_IMODE(path.stat().st_mode) != modes.get(relative):
                raise ValueError('Restored source mode differs')
        elif not path.is_dir():
            raise ValueError('Unexpected restored filesystem node')
    if actual != expected:
        raise ValueError('Restored complete source/config bytes differ')
    config = cargo_config(verified['manifest']['packages'],(inputs.resolve()/'vendor').as_posix())
    if (cargo_home/'config.toml').read_bytes() != config:
        raise ValueError('Owned Cargo configuration differs')
    return dict(packages=len(verified['packages']),original_files=len(expected)-len(verified['packages']),managed_checksums=len(verified['packages']),source_config_exact=True,
                mode_scope='Original archive modes verified; POSIX disk modes checked' if os.name!='nt' else 'Original archive modes verified; Windows does not expose POSIX executable bits')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--capsule',type=Path,default=ROOT/'cargo-inputs/inputs.zip')
    parser.add_argument('--sha256',required=True)
    parser.add_argument('--lock',type=Path,default=ROOT/'Cargo.lock')
    parser.add_argument('--inputs',type=Path)
    parser.add_argument('--cargo-home',type=Path)
    parser.add_argument('--readback',action='store_true')
    parser.add_argument('--protect-target',type=Path,action='append',default=[])
    args = parser.parse_args()
    if bool(args.inputs) != bool(args.cargo_home) or (args.readback and not args.inputs):
        parser.error('Use both --inputs and --cargo-home; --readback needs both')
    if args.capsule.stat().st_size > MAX_ARCHIVE_BYTES:
        raise ValueError('Capsule byte limit')
    verified = verify(args.capsule.read_bytes(),args.sha256,args.lock.read_bytes())
    if args.inputs and not args.readback:
        restore(verified,args.inputs,args.cargo_home,args.protect_target,source_root=plain_path(args.lock).parent)
    result = readback(verified,args.inputs,args.cargo_home) if args.inputs else dict(packages=len(verified['packages']),verified=True)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
