"""Source archive trust-boundary controls; no Rust, Git or package manager needed."""
import io
import lzma
from pathlib import Path
import tempfile
import tarfile
import unittest

from input_custody import digest, source_tree
import source_custody as source


class SourceCustody(unittest.TestCase):
    def setUp(self):
        self.files = {'Cargo.toml': (b'[workspace]\n', 0o100644),
                      'tools/run.sh': (b'#!/bin/sh\nexit 0\n', 0o100755),
                      'LICENSE': (b'Original complete grant.\n', 0o100644)}
        self.commit = ('tree '+source_tree(self.files)+'\nauthor A <a@b> 0 +0000\n'
                       'committer A <a@b> 0 +0000\n\nTest\n').encode()
        self.manifest = source.manifest_for(self.files, self.commit)
        self.raw = source.archive(self.files, self.manifest)

    def verify(self, raw):
        return source.verify(raw, digest(raw))

    def altered_tar(self, change):
        data = lzma.decompress(self.raw)
        out = io.BytesIO()
        with tarfile.open(fileobj=io.BytesIO(data)) as before, tarfile.open(fileobj=out, mode='w') as after:
            for entry in before:
                content = before.extractfile(entry).read()
                entry, content = change(entry, content)
                entry.size = len(content)
                after.addfile(entry, io.BytesIO(content))
        return lzma.compress(out.getvalue())

    def test_complete_deterministic_replay(self):
        files, manifest = self.verify(self.raw)
        self.assertEqual(files, self.files)
        with tempfile.TemporaryDirectory(prefix='lattice-source-test-') as temp:
            output = Path(temp)/'source'
            source.restore(files, manifest, output)
            self.assertFalse((output/'.git').exists())
            replay_files, replay_manifest = source.snapshot(output)
            self.assertEqual(source.archive(replay_files, replay_manifest), self.raw)

    def test_external_sha(self):
        with self.assertRaises(ValueError):
            source.verify(self.raw, '0'*64)

    def test_changed_original(self):
        raw = self.altered_tar(lambda e, b: (e, b+b'x') if e.name.endswith('LICENSE') else (e, b))
        with self.assertRaises(ValueError):
            self.verify(raw)

    def test_unsafe_paths_and_modes(self):
        for name in ['../escape', '/absolute', 'a/../b', 'NUL', 'a\\b', 'a\0b']:
            with self.subTest(name=name):
                def change(e, b):
                    if e.name.endswith('LICENSE'):
                        e.name = source.PREFIX+name
                    return e, b
                with self.assertRaises((ValueError, KeyError)):
                    self.verify(self.altered_tar(change))
        for mode in [0o600, 0o4755]:
            def change(e, b):
                e.mode = mode
                return e, b
            with self.assertRaises(ValueError):
                self.verify(self.altered_tar(change))

    def test_links_and_sparse(self):
        for kind in [tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.GNUTYPE_SPARSE]:
            def change(e, b):
                e.type = kind
                e.linkname = 'elsewhere'
                return e, b
            with self.subTest(kind=kind), self.assertRaises((ValueError, tarfile.TarError)):
                self.verify(self.altered_tar(change))

    def test_trailing_truncated_and_bounded_xz(self):
        for raw in [self.raw[:-1], self.raw+self.raw,
                    lzma.compress(lzma.decompress(self.raw)+b'nonzero')]:
            with self.assertRaises(ValueError):
                self.verify(raw)
        from unittest.mock import patch
        with patch.object(source, 'MAX_TAR', 50), self.assertRaises(ValueError):
            self.verify(self.raw)

    def test_git_commit_identity(self):
        changed = dict(self.manifest, commit='0'*40)
        with self.assertRaises(ValueError):
            source.check_manifest(self.files, changed)
        with self.assertRaises(ValueError):
            source.manifest_for(self.files, self.commit.replace(b'tree ', b'tree 0'))

    def test_git_administrative_paths_never_reach_restore(self):
        for name in ['.git/config', '.GIT', 'nested/.git/hooks/run']:
            files = dict(self.files)
            files[name] = (b'do not execute', 0o100644)
            commit = b'tree '+source_tree(files).encode()+b'\n\nTest\n'
            with self.assertRaises(ValueError):
                source.archive(files, source.manifest_for(files, commit))

    def test_preserve_existing_and_protected_destinations(self):
        with tempfile.TemporaryDirectory(prefix='lattice-source-test-') as temp:
            root = Path(temp)
            marker = root/'original'
            marker.write_bytes(b'keep')
            with self.assertRaises(ValueError):
                source.restore(self.files, self.manifest, root)
            with self.assertRaises(ValueError):
                source.restore(self.files, self.manifest, root/'compiled'/'source', [root/'compiled'])
            self.assertEqual(marker.read_bytes(), b'keep')
            self.assertFalse((root/'compiled').exists())

    def test_readback_rejects_mutation_and_local_override(self):
        with tempfile.TemporaryDirectory(prefix='lattice-source-test-') as temp:
            root = Path(temp)/'source'
            source.restore(self.files, self.manifest, root)
            (root/'LICENSE').write_bytes(b'changed')
            with self.assertRaises(ValueError):
                source.snapshot(root)
            (root/'LICENSE').write_bytes(self.files['LICENSE'][0])
            (root/'.cargo').mkdir()
            (root/'.cargo/config.toml').write_text('[patch]')
            with self.assertRaises(ValueError):
                source.snapshot(root)


if __name__ == '__main__':
    unittest.main()
