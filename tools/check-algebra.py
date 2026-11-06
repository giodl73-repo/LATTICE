"""Compare complete original public-fixture native/WASM strings and errors."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

from algebra_controls import ORIGINAL_STREAM_SHA256, controls, encoded_controls, validate
from rust_build import configure, finish

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--site', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--cargo-home', type=Path, required=True)
    parser.add_argument('--target', type=Path, required=True)
    args = parser.parse_args()
    context = configure(args.inputs, args.cargo_home, args.target)
    output = Path(os.path.abspath(args.output))
    for parent in [output, *output.parents]:
        if parent.is_symlink() or getattr(parent.lstat() if parent.exists() else None, 'st_file_attributes', 0) & 0x400:
            raise ValueError('Output uses a link/junction')
    target = context['target']
    if output.exists() or any(output == p or output in p.parents or p in output.parents for p in (ROOT, target)):
        raise ValueError('Use a new output outside source and compiled target')
    output.mkdir(parents=True)
    items = controls()
    data = encoded_controls(items)
    (output/'controls.jsonl').write_bytes(data)
    commands = []

    def run(name, command, data=None):
        result = subprocess.run([str(v) for v in command], cwd=ROOT, input=data, capture_output=True)
        (output/(name+'.stdout')).write_bytes(result.stdout)
        (output/(name+'.stderr')).write_bytes(result.stderr)
        commands.append(dict(name=name, command=[str(v) for v in command], exit_code=result.returncode))
        (output/'commands.json').write_text(json.dumps(commands, indent=2)+'\n', encoding='utf-8')
        if result.returncode:
            raise RuntimeError(name+' failed; raw evidence retained at '+str(output))
        return result.stdout

    run('build-native', ['cargo', 'build', '--locked', '--offline', '--release', '-p', 'lattice-web', '--example', 'algebra-control'])
    native = run('native', [target/'release/examples'/('algebra-control.exe' if os.name == 'nt' else 'algebra-control')], data)
    run('wasm', ['node', ROOT/'tools/check-algebra.mjs', args.site.resolve(), output/'controls.jsonl', output/'wasm.jsonl', output/'raw-js-abi.json'])
    wasm = (output/'wasm.jsonl').read_bytes()
    assert native == wasm and native.endswith(b'\n'), 'Complete native/WASM result streams differ'
    sha = hashlib.sha256(native).hexdigest()
    assert sha == ORIGINAL_STREAM_SHA256, 'Complete result stream differs from frozen original source'
    counts = validate(items, native.splitlines())
    policy = finish(context)
    receipt = dict(schema='lattice.algebra-parity.v1', controls=len(items), bounded_combinations=16384,
                   **counts, all_native_wasm_bytes_exact=True, complete_original_stream_exact=True,
                   independent_fixture_expectations=True, native_sha256=sha, wasm_sha256=hashlib.sha256(wasm).hexdigest(),
                   stream_bytes=len(native), raw_abi_observations=6, policy=policy, commands=commands,
                   scope='All32x32x8x2 original bounded fixture combinations plus14 portable typed invalids. Full native/WASM strings/errors and frozen original byte stream, no normalization. Independent required custody/selection/budget/bond/frontier checks; raw JS coercions separate. Not arbitrary semantic expansion or provider execution.')
    (output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
    print(json.dumps({k:v for k,v in receipt.items() if k not in ('policy', 'commands')}))


if __name__ == '__main__':
    main()
