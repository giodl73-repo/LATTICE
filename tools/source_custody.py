"""Complete Git source archives and bounded, standard-library-only reception.

The caller supplies a trusted SHA-256. Hashes prove byte identity, not authorship.
No downloaded code runs during verification or restoration.
"""
import argparse
import io
import json
import lzma
import os
from pathlib import Path
import stat
import subprocess
import tarfile

from input_custody import (destination, digest, git_object, inventory, overlaps,
                           path_set, plain_path, safe_path, source_tree, strict_json)

SUPPLEMENT = '.SOURCE-MANIFEST.json'
PREFIX = 'LATTICE-source/'
MAX_ARCHIVE = 5_000_000
MAX_TAR = 32_000_000
MAX_FILES = 4000


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'))+'\n').encode()


def manifest_for(files, commit_bytes):
    tree = source_tree(files)
    if commit_bytes.split(b'\n', 1)[0] != b'tree '+tree.encode():
        raise ValueError('Source Git commit/tree differs')
    return dict(schema='lattice.complete-source.v1', commit=git_object('commit', commit_bytes),
                tree=tree, commit_bytes_hex=commit_bytes.hex(), files=inventory(files))


def check_manifest(files, manifest):
    if len(files) > MAX_FILES or sum(len(v) for v, m in files.values()) > MAX_TAR:
        raise ValueError('Source resource limit')
    path_set(list(files))
    if (SUPPLEMENT in files or any(m not in (0o100644, 0o100755) for v, m in files.values())
            or any('.git' in [p.casefold() for p in name.split('/')] for name in files)):
        raise ValueError('Unsupported source file/mode')
    if manifest != manifest_for(files, bytes.fromhex(manifest['commit_bytes_hex'])):
        raise ValueError('Complete source inventory or Git identity differs')
    return manifest


def snapshot(root):
    root = plain_path(root)
    if (root/'.git').exists():
        def git(*args):
            return subprocess.check_output(['git', '-C', str(root), *args])
        if git('status', '--porcelain', '--untracked-files=all'):
            raise ValueError('Commit source changes before packaging')
        files = {}
        for record in git('ls-tree', '-rz', '--full-tree', 'HEAD').split(b'\0'):
            if not record:
                continue
            header, name = record.split(b'\t', 1)
            mode, kind, oid = header.split()
            if kind != b'blob' or mode not in (b'100644', b'100755'):
                raise ValueError('Complete source requires regular tracked files')
            files[name.decode('utf-8')] = (git('cat-file', 'blob', oid.decode()), int(mode, 8))
        manifest = manifest_for(files, git('cat-file', 'commit', 'HEAD'))
        check_manifest(files, manifest)
        return files, manifest
    manifest = strict_json((root/SUPPLEMENT).read_bytes())
    files = {}
    for item in manifest['files']:
        name = item['path']
        safe_path(name)
        path = plain_path(root/name)
        if root not in path.parents or not path.is_file():
            raise ValueError('Source path is absent or outside source root')
        mode = item['mode']
        # inventory stores Git modes as six-digit strings.
        mode = int(mode, 8) if isinstance(mode, str) else mode
        if os.name != 'nt' and stat.S_IMODE(path.stat().st_mode) != mode & 0o777:
            raise ValueError('Source disk mode differs')
        files[name] = (path.read_bytes(), mode)
    check_manifest(files, manifest)
    expected = set(files)|{SUPPLEMENT}
    # Only these derived directories may be created by the documented tools.
    generated = {'target', 'dist', 'node_modules', 'test-results', 'playwright-report'}
    for base, dirs, names in os.walk(root, followlinks=False):
        parent = Path(base)
        for name in list(dirs)+names:
            path = parent/name
            if path.is_symlink() or getattr(path.lstat(), 'st_file_attributes', 0) & 0x400:
                raise ValueError('Source tree contains a link/junction')
        dirs[:] = [name for name in dirs if not (
            (parent == root and name in generated) or name == '__pycache__')]
        for name in names:
            if (parent/name).relative_to(root).as_posix() not in expected:
                raise ValueError('Unexpected file in source tree')
    return files, manifest


def archive(files, manifest):
    check_manifest(files, manifest)
    entries = dict(files)
    entries[SUPPLEMENT] = (encoded(manifest), 0o100644)
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode='w', format=tarfile.PAX_FORMAT) as tar:
        for name, (value, mode) in sorted(entries.items()):
            entry = tarfile.TarInfo(PREFIX+name)
            entry.size, entry.mode, entry.mtime = len(value), mode & 0o777, 0
            tar.addfile(entry, io.BytesIO(value))
    if len(data.getvalue()) > MAX_TAR:
        raise ValueError('Source tar byte limit')
    result = lzma.compress(data.getvalue(), format=lzma.FORMAT_XZ, preset=6)
    if len(result) > MAX_ARCHIVE:
        raise ValueError('Source archive byte limit')
    return result


def verify(raw, expected_sha):
    if len(raw) > MAX_ARCHIVE or digest(raw) != expected_sha:
        raise ValueError('Source archive size/SHA-256 differs')
    decoder = lzma.LZMADecompressor(format=lzma.FORMAT_XZ, memlimit=64*1024*1024)
    decoded = decoder.decompress(raw, max_length=MAX_TAR+1)
    if len(decoded) > MAX_TAR or not decoder.eof or decoder.unused_data:
        raise ValueError('Source XZ resource limit, truncation or trailing stream')
    files = {}
    with tarfile.open(fileobj=io.BytesIO(decoded), mode='r:') as tar:
        for entry in tar:
            if (entry.type not in (tarfile.REGTYPE, tarfile.AREGTYPE) or entry.sparse is not None
                    or entry.mode not in (0o644, 0o755) or not entry.name.startswith(PREFIX)
                    or set(entry.pax_headers)-{'path'}):
                raise ValueError('Unsupported source tar entry')
            name = entry.name[len(PREFIX):]
            safe_path(name)
            if name in files or len(files) >= MAX_FILES+1:
                raise ValueError('Duplicate source path or file count limit')
            files[name] = (tar.extractfile(entry).read(), 0o100000 | entry.mode)
        if any(decoded[tar.offset:]):
            raise ValueError('Nonzero trailing source tar data')
    path_set(list(files))
    supplement, mode = files.pop(SUPPLEMENT)
    if mode != 0o100644:
        raise ValueError('Unexpected source supplement mode')
    manifest = strict_json(supplement)
    if supplement != encoded(manifest):
        raise ValueError('Noncanonical source supplement')
    check_manifest(files, manifest)
    return files, manifest


def restore(files, manifest, output, protected=()):
    check_manifest(files, manifest)
    output = destination(output)
    protected = [plain_path(p) for p in protected]
    if os.environ.get('CARGO_TARGET_DIR'):
        protected.append(plain_path(os.environ['CARGO_TARGET_DIR']))
    if any(overlaps(output, path) for path in protected):
        raise ValueError('Source destination overlaps protected data/target')
    output.mkdir(parents=True)
    entries = dict(files)
    entries[SUPPLEMENT] = (encoded(manifest), 0o100644)
    for name, (data, mode) in entries.items():
        target = output/name
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open('xb') as stream:
            stream.write(data)
        target.chmod(mode & 0o777)
    snapshot(output)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--sha256', required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--protect', type=Path, action='append', default=[])
    args = parser.parse_args()
    if args.archive.stat().st_size > MAX_ARCHIVE:
        raise ValueError('Source archive byte limit')
    files, manifest = verify(args.archive.read_bytes(), args.sha256)
    if args.output:
        restore(files, manifest, args.output, [args.archive, Path(__file__).resolve().parent, *args.protect])
    print(json.dumps(dict(commit=manifest['commit'], tree=manifest['tree'], files=len(files),
                          verified=True, restored=bool(args.output))))


if __name__ == '__main__':
    main()
