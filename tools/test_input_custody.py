"""Archive rejection and output-preservation controls for the real input kit."""
import copy
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import input_custody as ic


def packed(entries):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream,'w') as archive:
        for name,data,mode in entries:
            info = zipfile.ZipInfo(name,(1980,1,1,0,0,0))
            # Deliberately preserve unsafe wire spellings. ZipInfo's constructor
            # itself otherwise truncates NUL and normalizes Windows slashes.
            info.filename = name
            info.create_system = 3
            info.external_attr = mode << 16
            archive.writestr(info,data,compress_type=zipfile.ZIP_DEFLATED)
    return stream.getvalue()


def tar_bytes(name,kind=tarfile.REGTYPE,mode=0o644,data=b'x'):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream,mode='w:gz') as archive:
        entry = tarfile.TarInfo(name)
        entry.type = kind
        entry.mode = mode
        entry.linkname = '../outside'
        entry.size = len(data) if kind == tarfile.REGTYPE else 0
        archive.addfile(entry,io.BytesIO(data) if entry.size else None)
    return stream.getvalue()


class Inputs(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.raw = (ic.ROOT/'cargo-inputs/inputs.zip').read_bytes()
        cls.sha = ic.digest(cls.raw)
        cls.lock = (ic.ROOT/'Cargo.lock').read_bytes()
        cls.verified = ic.verify(cls.raw,cls.sha,cls.lock)
        cls.entries = ic.zip_files(cls.raw)

    def setUp(self):
        self.base = Path(tempfile.mkdtemp(prefix='lattice-cargo-inputs-')).resolve()
        self.addCleanup(self.cleanup)

    def cleanup(self):
        # All recursive cleanup stays in this explicitly created task directory.
        self.assertEqual(self.base.parent,Path(tempfile.gettempdir()).resolve())
        self.assertTrue(self.base.name.startswith('lattice-cargo-inputs-'))
        self.assertFalse(self.base.is_symlink())
        self.assertFalse(getattr(self.base.lstat(),'st_file_attributes',0)&0x400)
        shutil.rmtree(self.base)

    def modified(self,change):
        entries = dict(self.entries)
        manifest = copy.deepcopy(self.verified['manifest'])
        change(manifest,entries)
        entries['INPUT-MANIFEST.json'] = ((json.dumps(manifest)+'\n').encode(),0o100644)
        return packed([(name,data,mode)for name,(data,mode)in entries.items()])

    def rejects(self,raw):
        inputs,home = self.base/'inputs',self.base/'home'
        with self.assertRaises((ValueError,KeyError,TypeError,zipfile.BadZipFile,tarfile.TarError)):
            verified = ic.verify(raw,ic.digest(raw),self.lock)
            ic.restore(verified,inputs,home)
        self.assertFalse(inputs.exists())
        self.assertFalse(home.exists())

    def junction(self,link,target):
        try:
            link.symlink_to(target,target_is_directory=True)
        except OSError:
            if os.name != 'nt':
                raise
            result = subprocess.run(['cmd','/c','mklink','/J',str(link),str(target)],capture_output=True)
            self.assertEqual(result.returncode,0,result.stderr.decode(errors='replace'))

    def test_actual_complete_original_closure_and_notices(self):
        self.assertEqual(len(self.verified['packages']),26)
        self.assertEqual(sum(len(p['files'])for p in self.verified['packages'].values()),981)
        for package in self.verified['manifest']['packages']:
            source = self.verified['packages'][package['name']+'-'+package['version']]['files']
            for grant in package['grants']:
                self.assertIn(source[grant['path']][0],self.verified['notices'])
        self.assertIn('.gitignore',self.verified['packages']['bumpalo-3.20.3']['files'])
        self.assertIn('tests/fst/.gitignore',self.verified['packages']['unicode-ident-1.0.24']['files'])

    def test_restore_readback_and_mutation_extra_config_rejection(self):
        inputs,home = self.base/'inputs',self.base/'home'
        ic.restore(self.verified,inputs,home)
        self.assertEqual(ic.readback(self.verified,inputs,home)['original_files'],981)
        original = inputs/'vendor/bumpalo-3.20.3/.gitignore'
        data = original.read_bytes()
        original.write_bytes(data+b'changed')
        with self.assertRaises(ValueError):ic.readback(self.verified,inputs,home)
        original.write_bytes(data)
        extra = inputs/'vendor/unexpected.txt'
        extra.write_bytes(b'unexpected')
        with self.assertRaises(ValueError):ic.readback(self.verified,inputs,home)
        extra.unlink()
        config = home/'config.toml'
        config.write_bytes(config.read_bytes()+b'\n# altered\n')
        with self.assertRaises(ValueError):ic.readback(self.verified,inputs,home)

    def test_external_sha_and_changed_lock_rejected(self):
        with self.assertRaises(ValueError):ic.verify(self.raw,'0'*64,self.lock)
        with self.assertRaises(ValueError):ic.verify(self.raw,self.sha,self.lock+b'\n# changed lock\n')

    def test_lock_crlf_identity_policy_does_not_change_source_bytes(self):
        lock = self.lock.replace(b'\r\n',b'\n').replace(b'\n',b'\r\n')
        result = ic.verify(self.raw,self.sha,lock)
        self.assertEqual(result['packages'],self.verified['packages'])

    def test_duplicate_json_key_rejected(self):
        entries = [(name,data,mode)for name,(data,mode)in self.entries.items()if name!='INPUT-MANIFEST.json']
        entries.append(('INPUT-MANIFEST.json',b'{"schema":1,"schema":2}',0o100644))
        self.rejects(packed(entries))

    def test_missing_extra_or_duplicate_package_rejected(self):
        for change in [lambda m,e:m['packages'].pop(),lambda m,e:m['packages'].append(m['packages'][0]),lambda m,e:m['packages'][0].update(source='registry+https://other.invalid/index')]:
            with self.subTest(change=change):self.rejects(self.modified(change))

    def test_unreferenced_archive_entry_rejected(self):
        self.rejects(self.modified(lambda m,e:e.update({'extra.txt':(b'extra',0o100644)})))

    def test_original_archive_corruption_rejected(self):
        def change(m,e):
            name = m['packages'][0]['archive']
            raw,mode = e[name]
            e[name] = (raw+b'changed',mode)
        self.rejects(self.modified(change))

    def test_original_file_inventory_and_grants_cannot_be_trimmed(self):
        for field in ['files','grants']:
            self.rejects(self.modified(lambda m,e:m['packages'][0][field].pop()))

    def test_git_commit_tree_and_source_mode_identity(self):
        for field in ['commit','tree','commit_bytes_hex']:
            def change(m,e):
                record = next(p for p in m['packages']if p['source'].startswith('git+'))
                record[field] = '0'*40 if field!='commit_bytes_hex'else '00'
            self.rejects(self.modified(change))
        def change_mode(m,e):
            record = next(p for p in m['packages']if p['source'].startswith('git+'))
            record['files'][0]['mode'] = '100755' if record['files'][0]['mode']=='100644'else'100644'
        self.rejects(self.modified(change_mode))

    def test_git_sources_require_matching_immutable_https_rev(self):
        for source in ['git+http://example.invalid/a?rev='+'a'*40+'#'+'a'*40,
                       'git+https://example.invalid/a?branch=main#'+'a'*40,
                       'git+https://user:secret@example.invalid/a?rev='+'a'*40+'#'+'a'*40,
                       'git+https://example.invalid/a?rev='+'a'*40+'#'+'b'*40]:
            with self.assertRaises(ValueError):ic.git_identity(source)

    def test_unsafe_wire_paths_and_unsupported_zip_modes(self):
        for name in ['../escape','/absolute','a/../../escape','a//b','./name','a/./b','a\\b','C:/escape','a\0tail','CON','a/NUL.txt','COM¹.txt','LPT²','CONOUT$','NUL .txt','name.','name ','a:name','a\nname']:
            with self.subTest(path=repr(name)):
                with self.assertRaises(ValueError):ic.zip_files(packed([(name,b'x',0o100644)]))
        for mode in [0o120777,0o040755,0o100600,0]:
            with self.assertRaises(ValueError):ic.zip_files(packed([('file',b'x',mode)]))

    def test_nonstandard_zip_compression_rejected(self):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream,'w',compression=zipfile.ZIP_BZIP2)as archive:
            info = zipfile.ZipInfo('file')
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info,b'content',compress_type=zipfile.ZIP_BZIP2)
        with self.assertRaises(ValueError):ic.zip_files(stream.getvalue())

    def test_duplicate_case_and_ancestor_zip_paths(self):
        for names in [('file','file'),('File','file'),('a','a/b'),('A/x','a/y'),('é/x','e\u0301/y')]:
            with self.assertRaises(ValueError):ic.zip_files(packed([(n,b'x',0o100644)for n in names]))

    def test_zip_file_count_and_byte_limits(self):
        raw = packed([('a',b'abc',0o100644),('b',b'abc',0o100644)])
        with patch.object(ic,'MAX_FILES',1):
            with self.assertRaises(ValueError):ic.zip_files(raw)
        with patch.object(ic,'MAX_SOURCE_BYTES',5):
            with self.assertRaises(ValueError):ic.zip_files(raw)
        with patch.object(ic,'MAX_ARCHIVE_BYTES',1):
            with self.assertRaises(ValueError):ic.zip_files(raw)

    def test_crate_traversal_links_devices_and_modes(self):
        for name in ['pkg-1/../escape','/absolute','other/file','pkg-1/a//b','pkg-1/CON']:
            with self.assertRaises(ValueError):ic.crate_files(tar_bytes(name),'pkg-1')
        for kind in [tarfile.SYMTYPE,tarfile.LNKTYPE,tarfile.CHRTYPE,tarfile.FIFOTYPE]:
            with self.assertRaises(ValueError):ic.crate_files(tar_bytes('pkg-1/file',kind),'pkg-1')
        with self.assertRaises(ValueError):ic.crate_files(tar_bytes('pkg-1/file',mode=0o4755),'pkg-1')

    def test_crate_unsafe_ignored_directory_rejected(self):
        with self.assertRaises(ValueError):ic.crate_files(tar_bytes('pkg-1/../escape',tarfile.DIRTYPE,0o755),'pkg-1')

    def test_decompressed_tar_container_bound_precedes_header_parsing(self):
        with patch.object(ic,'MAX_TAR_BYTES',8):
            with self.assertRaises(ValueError):ic.crate_files(tar_bytes('pkg-1/file'),'pkg-1')

    def test_existing_destinations_are_not_overwritten(self):
        inputs,home = self.base/'inputs',self.base/'home'
        home.mkdir()
        sentinel = home/'keep.txt'
        sentinel.write_bytes(b'keep')
        with self.assertRaises(ValueError):ic.restore(self.verified,inputs,home)
        self.assertFalse(inputs.exists())
        self.assertEqual(sentinel.read_bytes(),b'keep')

    def test_nested_source_and_compiled_target_overlap_rejected(self):
        inputs = self.base/'inputs'
        for home in [inputs,inputs/'home']:
            with self.assertRaises(ValueError):ic.restore(self.verified,inputs,home)
            self.assertFalse(inputs.exists())
        with self.assertRaises(ValueError):ic.restore(self.verified,ic.ROOT/'must-not-create-inputs',self.base/'home')
        with patch.dict(os.environ,{'CARGO_TARGET_DIR':str(self.base/'compiled')}):
            with self.assertRaises(ValueError):ic.restore(self.verified,self.base/'compiled/input',self.base/'home')
        with self.assertRaises(ValueError):ic.restore(self.verified,inputs,self.base/'home',[inputs/'compiled'])
        self.assertFalse(inputs.exists())
        self.assertFalse((self.base/'home').exists())

    def test_parent_junction_and_link_dotdot_spelling_rejected(self):
        target = self.base/'target'
        target.mkdir()
        link = self.base/'link'
        self.junction(link,target)
        for path in [link/'new',link/'..'/'other']:
            with self.assertRaises(ValueError):ic.destination(path)
        self.assertFalse((target/'new').exists())

    def test_readback_rejects_replaced_root_junction(self):
        target = self.base/'target'
        target.mkdir()
        link = self.base/'link'
        self.junction(link,target)
        with self.assertRaises(ValueError):ic.readback(self.verified,link,self.base/'home')

    def test_copied_receiver_works_without_git_cargo_or_site_packages(self):
        program = self.base/'receiver/input_custody.py'
        program.parent.mkdir()
        shutil.copyfile(Path(ic.__file__),program)
        inputs,home = self.base/'sources',self.base/'home'
        env = os.environ.copy()
        env['PATH'] = ''
        result = subprocess.run([sys.executable,'-I','-S','-B',str(program),
            '--capsule',str(ic.ROOT/'cargo-inputs/inputs.zip'),'--sha256',self.sha,
            '--lock',str(ic.ROOT/'Cargo.lock'),'--inputs',str(inputs),'--cargo-home',str(home)],
            env=env,capture_output=True)
        self.assertEqual(result.returncode,0,result.stderr.decode(errors='replace'))
        self.assertEqual(json.loads(result.stdout)['original_files'],981)
        self.assertEqual(ic.readback(self.verified,inputs,home)['packages'],26)




class LockedMasterIdentity(unittest.TestCase):
    def test_original_master_selector_preserved_with_immutable_commit(self):
        source='git+https://github.com/giodl73-repo/METIS-CORE.git?branch=master#78ae34090e043e79a206f2daffaa3889389b4790'
        self.assertEqual(ic.git_identity(source),('https://github.com/giodl73-repo/METIS-CORE.git','78ae34090e043e79a206f2daffaa3889389b4790'))
        config=ic.cargo_config([{'source':source}]).decode()
        self.assertIn('branch = "master"',config)
        self.assertNotIn('rev = ',config)
        self.assertIn('offline = true',config)

    def test_master_aliases_missing_commit_and_mixed_selectors_rejected(self):
        base='git+https://example.invalid/a?'
        for selector,fragment in [('branch=master',''),('branch=master','abc'),('branch=master','A'*40),('branch=master&branch=master','a'*40),('branch=master&rev='+'a'*40,'a'*40),('branch=master&extra=','a'*40),('branch=','a'*40)]:
            with self.subTest(selector=selector,fragment=fragment):
                with self.assertRaises(ValueError):ic.git_identity(base+selector+'#'+fragment)


if __name__ == '__main__':
    unittest.main()
