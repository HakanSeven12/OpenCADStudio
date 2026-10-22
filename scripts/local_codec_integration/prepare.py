"""Parent-run offline local resolution. No fetching, restoration, or source edits."""
import difflib
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
APP = ROOT / 'target/worktrees/assoc-arc-integration'
CODEC = ROOT / 'target/codec-assoc-2010'
CONFIG = HERE / 'codec-local.toml'
APP_HEAD = '0b414389cb1a954ed22ddb95c44727fe0a5bd075'
CODEC_HEAD = 'b058a27d36b5bfa83b9ba16e72cae3a8185fb935'
MANIFESTS = ['Cargo.toml', 'crates/ocs_save/Cargo.toml',
             'crates/ocs_web_worker/Cargo.toml', 'crates/ocs_plugin_api/Cargo.toml']
INPUTS = [ROOT / 'diagnostics/region-dwg' / name for name in [
    'originals/8216.dwg', 'originals/8216_v2013.dwg', 'originals/8216_v2013.dxf',
    'originals/8216_v2018.dwg', 'originals/8216_v2018.dxf',
    'variants/8216_v2010.dwg', 'variants/8216_v2010.dxf']]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def git(path, *args):
    return subprocess.check_output(['git', '-C', str(path), *args], text=True).strip()


def dump(name, value):
    (HERE / name).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def snapshot():
    files = INPUTS + [CONFIG, ROOT / 'Cargo.lock', CODEC / 'Cargo.toml']
    # The codec belongs to the ignored target/ workspace, whose lock is not
    # stored inside the codec checkout. Fingerprint a local lock only if present.
    if (CODEC / 'Cargo.lock').is_file():
        files.append(CODEC / 'Cargo.lock')
    files += [base / name for base in (ROOT, APP) for name in MANIFESTS]
    # Fingerprint tracked Rust sources/manifests without reading unrelated private files.
    for base in (APP, CODEC):
        files += [base / name for name in git(base, 'ls-files').splitlines()
                  if Path(name).suffix in {'.rs', '.toml'} and (base / name).is_file()]
    files += [HERE / 'prepare.py', HERE / 'verify.py']
    return {'hashes': {str(p): sha(p) for p in files},
            'root_head': git(ROOT, 'rev-parse', 'HEAD'),
            'root_status': git(ROOT, 'status', '--porcelain'),
            'app_head': git(APP, 'rev-parse', 'HEAD'),
            'app_branch': git(APP, 'branch', '--show-current'),
            'app_status': git(APP, 'status', '--porcelain'),
            'codec_head': git(CODEC, 'rev-parse', 'HEAD'),
            'codec_status': git(CODEC, 'status', '--porcelain')}


def identities(state, resolved=False):
    require(state['app_head'] == APP_HEAD, 'Application HEAD changed')
    require(state['codec_head'] == CODEC_HEAD, 'Codec HEAD changed')
    require(state['app_branch'] == 'build/assoc-arc-codec-integration', 'Wrong application branch')
    require(not state['codec_status'], 'Codec checkout is dirty')
    allowed = ['', 'M Cargo.lock'] if resolved else ['']
    require(state['app_status'] in allowed, 'Unexpected application changes: ' + state['app_status'])


def unchanged(before, after, lock_allowed=False):
    for key in before:
        if lock_allowed and key == 'app_status':
            continue
        require(before[key] == after[key], 'Protected state changed: ' + key)


def run(name, command, cwd, seconds, env=None, stdin=None):
    """Bound process group; retain exact streams and exit, including timeout=124."""
    dump(name + '.command.json', {'argv': command, 'cwd': str(cwd), 'deadline_seconds': seconds})
    with (HERE / (name + '.stdout')).open('wb') as out, (HERE / (name + '.stderr')).open('wb') as err:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.PIPE,
                                   stdout=out, stderr=err, start_new_session=True)
        try:
            process.communicate(None if stdin is None else stdin.encode(), timeout=seconds)
            code = process.returncode
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.communicate()
            code = 124
    dump(name + '.exit.json', {'exit': code})
    require(code == 0, f'{name}: exit {code}; inspect {HERE / (name + ".stderr")}')


def main():
    require(not (HERE / 'prepared.json').exists(), 'Already prepared; do not overwrite provenance')
    before = snapshot()
    identities(before)
    require(before['root_head'] == APP_HEAD, 'Root HEAD differs from expected base')
    dump('baseline.json', before)
    run('rustc-version', ['rustc', '-vV'], APP, 60)
    run('cargo-version', ['cargo', '-vV'], APP, 60)
    old_text = (APP / 'Cargo.lock').read_text()
    (HERE / 'Cargo.lock.before').write_text(old_text)
    command = ['cargo', '--config', str(CONFIG), 'metadata', '--offline', '--format-version', '1']
    try:
        run('metadata', command, APP, 90)
    finally:
        new_text = (APP / 'Cargo.lock').read_text()
        (HERE / 'Cargo.lock.after').write_text(new_text)
        (HERE / 'Cargo.lock.diff').write_text(''.join(difflib.unified_diff(
            old_text.splitlines(True), new_text.splitlines(True), fromfile='before', tofile='after')))
        after = snapshot()
        dump('after-resolution.json', after)
        identities(after, resolved=True)
        unchanged(before, after, lock_allowed=True)
    old = tomllib.loads(old_text)
    new = tomllib.loads(new_text)
    old_codec = [p for p in old['package'] if p['name'] == 'acadrust']
    new_codec = [p for p in new['package'] if p['name'] == 'acadrust']
    require(len(old_codec) == len(new_codec) == 1, 'Expected unique acadrust lock entry')
    require(old_codec[0].get('source', '').startswith('git+https://github.com/HakanSeven12/opencadcodec.git'), 'Unexpected original codec source')
    require('source' not in new_codec[0], 'Codec lock entry is not a path source')
    normalized = dict(old_codec[0])
    normalized.pop('source')
    require(normalized == new_codec[0], 'Codec package changed beyond git-to-path source')
    normalized_old = dict(old)
    normalized_old['package'] = [new_codec[0] if p['name'] == 'acadrust' else p for p in old['package']]
    require(normalized_old == new, 'Unexpected lock dependency/package/version drift; inspect Cargo.lock.diff')
    metadata = json.loads((HERE / 'metadata.stdout').read_text())
    packages = [p for p in metadata['packages'] if p['name'] == 'acadrust']
    require(len(packages) == 1, 'Metadata has multiple codec packages')
    package = packages[0]
    require(package['source'] is None and Path(package['manifest_path']).resolve() == CODEC / 'Cargo.toml', 'Metadata codec does not resolve locally')
    require(package['version'] == '0.6.4', 'Unexpected codec version')
    require(sum(n['id'] == package['id'] for n in metadata['resolve']['nodes']) == 1, 'Codec node missing/duplicated')
    dump('prepared.json', {'state': after, 'app_lock_sha256': sha(APP / 'Cargo.lock'), 'codec': package})
    print('Resolved unique local acadrust 0.6.4:', package['manifest_path'])
    print('Lock delta: acadrust git -> path only; protected inputs and declarations unchanged.')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(124 if 'exit 124;' in str(error) else 1)
