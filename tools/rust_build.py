"""Owned offline Rust inputs and stable diagnostic paths for site builds."""
import json
import os
from pathlib import Path
import subprocess

from input_custody import (ROOT, cargo_config, digest, overlaps, plain_path,
                           readback, restore, safe_path, verify)


def configure(inputs, cargo_home, target):
    inputs,cargo_home,target = plain_path(inputs),plain_path(cargo_home),plain_path(target)
    if target == ROOT or target in ROOT.parents or (ROOT in target.parents and ROOT/'target' not in [target,*target.parents]):
        raise ValueError('Use the source target directory or an external compiled target')
    if any(overlaps(p,t)for p in (inputs,cargo_home)for t in (ROOT,target)) or overlaps(inputs,cargo_home):
        raise ValueError('Owned source/home overlaps source, target or each other')
    if any(os.environ.get(key)for key in ['RUSTC','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER']):
        raise ValueError('Qualified builds require the declared compiler without an external wrapper')
    expected = (ROOT/'cargo-inputs/inputs.sha256').read_text().split()[0]
    verified = verify((ROOT/'cargo-inputs/inputs.zip').read_bytes(),expected,(ROOT/'Cargo.lock').read_bytes())
    os.environ['CARGO_TARGET_DIR'] = str(target)
    os.environ['CARGO_HOME'] = str(cargo_home)
    os.environ['CARGO_NET_OFFLINE'] = 'true'
    os.environ['RUSTUP_TOOLCHAIN'] = '1.95.0'
    target_preexisting = target.exists()
    if inputs.exists() or cargo_home.exists():
        if not inputs.is_dir() or not cargo_home.is_dir():
            raise ValueError('Use two new folders or an existing verified input/home pair')
    else:
        restore(verified,inputs,cargo_home,[target])
    before = readback(verified,inputs,cargo_home)
    metadata = json.loads(subprocess.check_output(['cargo','metadata','--locked','--offline','--all-features','--format-version','1'],cwd=ROOT))
    if Path(metadata['target_directory']).resolve() != target:
        raise ValueError('Cargo compiled target differs')
    expected_packages = {(p['name'],p['version'],p['source'])for p in verified['manifest']['packages']}
    actual_packages = {(p['name'],p['version'],p['source'])for p in metadata['packages']if p.get('source')}
    if expected_packages != actual_packages:
        raise ValueError('Cargo package identities differ from owned inputs')
    for package in metadata['packages']:
        manifest = Path(package['manifest_path']).resolve()
        if package.get('source'):
            expected_manifest = inputs/'vendor'/(package['name']+'-'+package['version'])/'Cargo.toml'
            if manifest != expected_manifest:
                raise ValueError('Cargo used a package outside the owned original sources')
        elif ROOT not in manifest.parents or package['id'] not in metadata['workspace_members']:
            raise ValueError('Unexpected local/path dependency or override')
    policy = configure_flags(metadata)
    return dict(verified=verified,inputs=inputs,cargo_home=cargo_home,target=target,
                before=before,target_preexisting=target_preexisting,policy=policy,
                capsule_sha256=expected,source_config_template_sha256=digest(cargo_config(verified['manifest']['packages'])))


def configure_flags(metadata):
    encoded = os.environ.get('CARGO_ENCODED_RUSTFLAGS')
    inherited = (encoded.split('\x1f') if encoded else []) if encoded is not None else os.environ.get('RUSTFLAGS','').split()
    flags = list(inherited)
    flags.append('--remap-path-prefix='+str(ROOT)+'=/lattice-source')
    names = set()
    for package in sorted((p for p in metadata['packages']if p.get('source')),key=lambda p:(p['name'],p['version'])):
        label = package['name']+'-'+package['version']
        safe_path(label)
        if label in names:
            raise ValueError('Virtual dependency path collision')
        names.add(label)
        path = Path(package['manifest_path']).resolve().parent
        flags.append('--remap-path-prefix='+str(path)+'=/lattice-inputs/'+label)
    flags.append('--remap-path-prefix='+str(Path(metadata['target_directory']).resolve())+'=/lattice-target')
    os.environ['CARGO_ENCODED_RUSTFLAGS'] = '\x1f'.join(flags)
    os.environ.pop('RUSTFLAGS',None)
    return dict(source='/lattice-source',target='/lattice-target',dependencies=sorted(names),inherited_flags=inherited)


def finish(context):
    after = readback(context['verified'],context['inputs'],context['cargo_home'])
    if after != context['before']:
        raise ValueError('Original input custody changed during compilation')
    return dict(capsule_sha256=context['capsule_sha256'],source_config_template_sha256=context['source_config_template_sha256'],
                source_readback=after,diagnostic_path_policy=context['policy'])
