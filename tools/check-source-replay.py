"""Receive the published archive without Git, rebuild with a new empty target.

Run only after trusting the source and builder. Retains raw commands and logs.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

from input_custody import destination, digest, overlaps, plain_path, strict_json, zip_files
from source_custody import snapshot

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--site', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--browser', action='store_true')
    args = parser.parse_args()
    site, output = plain_path(args.site), destination(args.output)
    if overlaps(output, ROOT) or overlaps(output, site):
        raise ValueError('Replay output overlaps original source/site')
    original_files, original_source = snapshot(ROOT)
    manifest = strict_json((site/'BUILD-MANIFEST.json').read_bytes())
    expected = {item['path']:(item['bytes'], item['sha256']) for item in manifest['files']}
    actual = {p.relative_to(site).as_posix():(p.stat().st_size, digest(p.read_bytes()))
              for p in site.rglob('*') if p.is_file() and p.name != 'BUILD-MANIFEST.json'}
    if expected != actual or manifest['commit'] != original_source['commit']:
        raise ValueError('Published site differs from its manifest/source')
    output.mkdir(parents=True)
    helper = output/'trusted-receiver'
    helper.mkdir()
    for name, (data, mode) in zip_files((site/'receiver.zip').read_bytes()).items():
        path = helper/name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open('xb') as stream:
            stream.write(data)
    env = os.environ.copy()
    env.pop('RUSTFLAGS', None)
    env.pop('CARGO_ENCODED_RUSTFLAGS', None)
    env.pop('LATTICE_SITE_URL', None)
    env['CARGO_INCREMENTAL'] = '0'
    commands = []
    source = output/'source'
    inputs, home, target = [output/name for name in ('inputs', 'cargo-home', 'empty-target')]
    def run(name, command, cwd, environment=env):
        process = subprocess.run([str(v) for v in command], cwd=cwd, env=environment, capture_output=True)
        (output/(name+'.stdout')).write_bytes(process.stdout)
        (output/(name+'.stderr')).write_bytes(process.stderr)
        commands.append(dict(name=name, command=[str(v) for v in command], exit_code=process.returncode))
        (output/'commands.json').write_text(json.dumps(commands, indent=2)+'\n', encoding='utf-8')
        if process.returncode:
            raise RuntimeError(name+' failed; raw evidence retained at '+str(output))
    isolated = dict(env, PATH='')
    run('receive-without-git-cargo-path', [sys.executable, '-I', '-S', '-B', helper/'receive-source.py',
        '--archive', site/'source.tar.xz', '--sha256', manifest['source_archive_sha256'], '--output', source,
        '--protect', target], helper, isolated)
    if (source/'.git').exists() or any(path.exists() for path in (inputs, home, target)):
        raise ValueError('Replay was not received outside Git with new inputs/home/empty target')
    if snapshot(source) != (original_files, original_source):
        raise ValueError('Received complete original source differs')
    run('build-empty-target', [sys.executable, '-B', source/'tools/build-pages.py',
        '--inputs', inputs, '--cargo-home', home, '--target', target], source)
    rebuilt = source/'dist/LATTICE'
    def inventory(directory):
        return {p.relative_to(directory).as_posix():(p.stat().st_size, digest(p.read_bytes()))
                for p in directory.rglob('*') if p.is_file()}
    before, after = inventory(site), inventory(rebuilt)
    if before != after:
        (output/'site-differences.json').write_text(json.dumps(dict(before=before, after=after), indent=2)+'\n')
        raise ValueError('Complete published files/manifest differ after empty-target rebuild')
    env.update(CARGO_HOME=str(home), CARGO_TARGET_DIR=str(target), CARGO_NET_OFFLINE='true', RUSTUP_TOOLCHAIN='1.95.0')
    run('native-tests', ['cargo', 'test', '--locked', '--workspace'], source)
    run('native-format', ['cargo', 'fmt', '--all', '--', '--check'], source)
    run('native-clippy', ['cargo', 'clippy', '--locked', '--no-deps', '--workspace', '--all-targets', '--', '-D', 'warnings', '-A', 'clippy::too-many-arguments'], source)
    run('full-native-wasm', [sys.executable, '-B', source/'tools/check-algebra.py', '--site', rebuilt, '--output', output/'algebra-parity', '--inputs', inputs, '--cargo-home', home, '--target', target], source)
    if args.browser:
        browser_env = dict(env, LATTICE_DIST=str(source/'dist'))
        npm = 'npm.cmd' if os.name == 'nt' else 'npm'
        run('browser', [npm, 'test'], ROOT, browser_env)
    sha = (source/'cargo-inputs/inputs.sha256').read_text().split()[0]
    run('input-readback', [sys.executable, '-I', '-S', '-B', source/'tools/input_custody.py',
        '--sha256', sha, '--inputs', inputs, '--cargo-home', home, '--readback'], source)
    if snapshot(source) != (original_files, original_source):
        raise ValueError('Complete source changed during replay qualification')
    receipt = dict(schema='lattice.complete-source-replay.v1', commit=original_source['commit'],
        tree=original_source['tree'], original_files=len(original_files), source_sha256=manifest['source_archive_sha256'],
        source_received_without_git_cargo_path=True, inputs_home_target_initially_absent=True,
        published_files=len(before), published_bytes=sum(n for n, sha in before.values()),
        all_files_and_manifest_byte_exact=True, browser_run=args.browser, files=before, commands=commands,
        scope='Same installed platform/toolchain; original source/no Git, new owned inputs/home/empty target. External tools reused. No cross-platform compiler-byte or default/live release claim.')
    (output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
    print(json.dumps({k:v for k,v in receipt.items() if k not in ('files', 'commands')}))


if __name__ == '__main__':
    main()
