"""Build LATTICE's complete public context algebra delivery from its native Rust core."""
import argparse
import io
import json
import os
import subprocess
import zipfile
from pathlib import Path

from input_custody import destination, digest, strict_json
from rust_build import configure, finish
from source_custody import archive, encoded, snapshot, verify
from algebra_controls import examples

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def receiver_bundle(files):
    entries = {name:files['tools/'+name][0] for name in (
        'receive-source.py', 'source_custody.py', 'input_custody.py')}
    entries['LICENSE'] = files['LICENSE'][0]
    entries['COMPONENT-NOTICES.txt'] = (files['notices/TERRAIN-COMPONENTS-LICENSE.txt'][0]+b'\n\n'+files['notices/WITNESS-COMPONENTS-LICENSE.txt'][0])
    entries['README.txt'] = (
        'LATTICE trusted source receiver: Python 3.11+ standard library only.\n'
        'Inspect/trust these modules before use. Obtain the archive expected SHA-256\n'
        'through a trusted channel. No downloaded code runs during verification.\n'
        'python -I -S -B receive-source.py --archive /downloads/source.tar.xz '
        '--sha256 EXPECTED --output /new/LATTICE --protect /compiled-target\n'
        'Use a new output outside the helper/download/compiled-target directories.\n'
        'LICENSE and COMPONENT-NOTICES.txt retain the complete applicable MIT grants.\n'
        'This does not install Python, Rust, wasm-bindgen or any npm packages.\n').encode()
    data = io.BytesIO()
    with zipfile.ZipFile(data, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
        for name, value in sorted(entries.items()):
            entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            bundle.writestr(entry, value, compresslevel=9)
    return data.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("dist/LATTICE"))
    parser.add_argument("--inputs",type=Path,required=True,help="New or verified original source input folder outside this checkout")
    parser.add_argument("--cargo-home",type=Path,required=True,help="New or verified owned Cargo home outside this checkout")
    parser.add_argument("--target",type=Path,default=Path(os.environ.get('CARGO_TARGET_DIR',ROOT/'target')))
    args = parser.parse_args()
    output = destination(args.output)
    if output == ROOT / "dist" or ROOT / "dist" not in output.parents:
        raise ValueError("Output must be a directory inside dist")
    files, source_manifest = snapshot(ROOT)
    source_archive = archive(files, source_manifest)
    verify(source_archive, digest(source_archive))
    receiver = receiver_bundle(files)
    runtime_relative = 'assets/'+source_manifest['commit']
    version = subprocess.check_output(["wasm-bindgen", "--version"], text=True).strip()
    if version != "wasm-bindgen 0.2.127":
        raise ValueError(f"Expected wasm-bindgen 0.2.127, found {version}")
    context = configure(args.inputs,args.cargo_home,args.target)
    dependency_notices = (ROOT/'DEPENDENCY-NOTICES.txt').read_bytes()
    if dependency_notices != context['verified']['notices']:
        raise ValueError('Published original dependency notices differ from input archives')
    runtime_notice = strict_json((ROOT/'notices/RUNTIME-NOTICES.json').read_bytes())
    if runtime_notice['file'] != 'rust-1.95.0-COPYRIGHT-library.html':
        raise ValueError('Unexpected Rust runtime notice path')
    runtime_bytes = (ROOT/'notices'/runtime_notice['file']).read_bytes()
    if runtime_notice['toolchain'] != '1.95.0' or runtime_notice['file'] != 'rust-1.95.0-COPYRIGHT-library.html' or len(runtime_bytes) != runtime_notice['bytes'] or digest(runtime_bytes) != runtime_notice['sha256']:
        raise ValueError('Original Rust runtime notices differ')
    run("cargo", "build", "--locked", "--release", "-p", "lattice-web", "--example", "algebra-control")
    native = context['target']/'release/examples'/('algebra-control.exe' if os.name == 'nt' else 'algebra-control')
    example_inputs = examples()
    native_lines = subprocess.check_output([str(native)],input=b''.join((json.dumps(c,separators=(',',':'))+'\n').encode()for c in example_inputs),cwd=ROOT).splitlines()
    if len(native_lines) != len(example_inputs):
        raise ValueError('Incomplete original public algebra examples')
    native_frames = b''.join((json.dumps({'inputs':c,'native':json.loads(line)},separators=(',',':'),ensure_ascii=False)+'\n').encode()for c,line in zip(example_inputs,native_lines))
    run("cargo", "build", "--locked", "--release", "--target", "wasm32-unknown-unknown", "-p", "lattice-web", "--lib", "--features", "wasm")
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT))
    wasm = Path(metadata["target_directory"]) / "wasm32-unknown-unknown/release/lattice_web.wasm"
    output.mkdir(parents=True, exist_ok=False)
    runtime = output/runtime_relative
    runtime.mkdir(parents=True)
    run("wasm-bindgen", str(wasm), "--target", "web", "--out-dir", str(runtime / "pkg"))
    for name in ("style.css", "app.js", "algebra-session.js", "worker.js"):
        (runtime/name).write_bytes(files['web/'+name][0])
    substitutions = {'@@RUNTIME@@':runtime_relative, '@@COMMIT@@':source_manifest['commit'],
                     '@@SOURCE_SHA@@':digest(source_archive), '@@RECEIVER_SHA@@':digest(receiver)}
    for name in ('index.html', 'build.html'):
        template = files['web/'+name][0].decode('utf-8')
        for key, value in substitutions.items():
            template = template.replace(key, value)
        (output/name).write_bytes(template.encode())
    (output/'LICENSE').write_bytes(files['LICENSE'][0])
    (output/'COMPONENT-NOTICES.txt').write_bytes((files['notices/TERRAIN-COMPONENTS-LICENSE.txt'][0]+b'\n\n'+files['notices/WITNESS-COMPONENTS-LICENSE.txt'][0]))
    (output/'DEPENDENCY-NOTICES.txt').write_bytes(dependency_notices)
    (output/'RUST-RUNTIME-NOTICES.html').write_bytes(runtime_bytes)
    (output / ".nojekyll").touch()
    (output/"native-examples.ndjson").write_bytes(native_frames)
    (output/'source.tar.xz').write_bytes(source_archive)
    (output/'receiver.zip').write_bytes(receiver)
    policy = finish(context)
    if snapshot(ROOT) != (files, source_manifest):
        raise ValueError('Complete source changed during compilation')
    published = [dict(path=p.relative_to(output).as_posix(), bytes=p.stat().st_size, sha256=digest(p.read_bytes()))
                 for p in sorted(output.rglob('*')) if p.is_file()]
    manifest = dict(schema='lattice.pages-build.v1', commit=source_manifest['commit'], tree=source_manifest['tree'],
                    runtime=runtime_relative, source_archive_sha256=digest(source_archive),
                    source_files=source_manifest['files'], files=published,
                    original_inputs=policy, toolchain='1.95.0', wasm_bindgen='0.2.127',
                    manifest_scope='Every other published regular file; manifest excludes itself', budget_bytes=5_000_000)
    (output/'BUILD-MANIFEST.json').write_bytes(encoded(manifest))
    size = sum(path.stat().st_size for path in output.rglob("*") if path.is_file())
    if size >= 5_000_000:
        raise ValueError("Context workbench exceeds its 5 MB delivery budget")
    print(f"Pages context workbench: {size:,} bytes at {output}")


if __name__ == "__main__":
    main()
