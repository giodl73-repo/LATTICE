"""Compiler flag precedence and stable path policy, without compiling Rust."""
import os
from pathlib import Path
import unittest
from unittest.mock import patch

from rust_build import configure_flags


class RustFlags(unittest.TestCase):
    def metadata(self):
        return dict(target_directory=str(Path.cwd()/'target'),packages=[
            dict(name='example',version='1.0.0',source='registry+example',manifest_path=str(Path.cwd()/'inputs/example/Cargo.toml'))])

    def test_empty_encoded_flags_override_plain_flags(self):
        with patch.dict(os.environ,{'RUSTFLAGS':'--cfg must_not_activate','CARGO_ENCODED_RUSTFLAGS':''}):
            policy = configure_flags(self.metadata())
            self.assertEqual(policy['inherited_flags'],[])
            self.assertNotIn('must_not_activate',os.environ['CARGO_ENCODED_RUSTFLAGS'])
            self.assertNotIn('RUSTFLAGS',os.environ)

    def test_encoded_flags_and_target_remap_are_preserved(self):
        with patch.dict(os.environ,{'RUSTFLAGS':'--cfg ignored','CARGO_ENCODED_RUSTFLAGS':'--cfg\x1fdeclared'}):
            policy = configure_flags(self.metadata())
            self.assertEqual(policy['inherited_flags'],['--cfg','declared'])
            flags = os.environ['CARGO_ENCODED_RUSTFLAGS'].split('\x1f')
            self.assertEqual(flags[:2],['--cfg','declared'])
            self.assertTrue(flags[-1].endswith('=/lattice-target'))
            self.assertEqual(policy['dependencies'],['example-1.0.0'])


if __name__ == '__main__':
    unittest.main()
