"""Parent-run bounded read-only stdio verification of the native local build."""
import json
import os
import sys

from prepare import (APP, CODEC, CONFIG, HERE, INPUTS, dump, identities,
                     require, run, sha, snapshot, unchanged)

BINARY = HERE / 'build/release/OpenCADStudio'
HANDLES = ['846D', '84CD', '84D0', '84E3', '84FC']


def main():
    prepared = json.loads((HERE / 'prepared.json').read_text())
    before = snapshot()
    identities(before, resolved=True)
    unchanged(prepared['state'], before)
    require(sha(APP / 'Cargo.lock') == prepared['app_lock_sha256'], 'Resolved lock changed')
    binary_hash = sha(BINARY)
    dump('artifact.json', {'binary': str(BINARY), 'sha256': binary_hash,
                           'app_head': before['app_head'], 'codec_head': before['codec_head'],
                           'config_sha256': sha(CONFIG), 'metadata_sha256': sha(HERE / 'metadata.stdout'),
                           'protected_state': before})
    env = os.environ.copy()
    # Leave XDG_RUNTIME_DIR untouched: GPU/session probing may need the real session.
    for key in ['HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_DATA_HOME', 'TMPDIR']:
        directory = HERE / 'runtime' / key.lower()
        directory.mkdir(parents=True, exist_ok=True)
        env[key] = str(directory)
    summary = []
    try:
        run('version', [str(BINARY), '--version'], HERE, 60, env)
        for path in INPUTS:
            name = path.stem + '-' + path.suffix[1:]
            requests = [
                {'op': 'open', 'path': str(path)},
                {'op': 'records', 'collection': 'objects', 'handles': HANDLES},
                {'op': 'query', 'type': 'Region'},
            ]
            dump(name + '.requests.json', requests)
            run(name, [str(BINARY), '--serve', '--read-only'], HERE, 90, env,
                ''.join(json.dumps(request) + '\n' for request in requests))
            responses = [json.loads(line) for line in (HERE / (name + '.stdout')).read_text().splitlines() if line.strip()]
            # The public stdio server emits one ready banner followed by one response per request.
            require(len(responses) == 4, f'{name}: expected ready plus three JSON responses')
            opened, records, regions = responses[1:]
            require(all(response.get('ok') is True for response in [opened, records, regions]), f'{name}: operation failed')
            require(opened['total'] == 211, f'{name}: entity count differs')
            require(regions['count'] == 2 and len(regions['entities']) == 2, f'{name}: expected two REGIONs')
            require(records['count'] == records['returned'] == 5, f'{name}: expected five objects')
            require({record['handle'].upper() for record in records['records']} == set(HANDLES), f'{name}: handles differ')
            if path.suffix.lower() == '.dwg':
                with path.open('rb') as source:
                    version = source.read(6).decode('ascii')
                require(version in {'AC1024', 'AC1027', 'AC1032'}, f'{name}: unsupported DWG version {version}')
                legacy = version == 'AC1024'
            else:
                legacy = 'v2010' in path.name
            expected_reals = 3 if legacy else 4
            for record in records['records']:
                require(record['type'] == 'Associative', f'{name}: Unknown/non-associative fallback')
                data = record['properties']['data']
                require(set(data) == {'EdgeActionParam'}, f'{name}: payload is not typed EdgeActionParam')
                edge = data['EdgeActionParam']
                require(edge['subcurve_kind'] == 'Arc', f'{name}: curve is not Arc')
                curve = edge['curve']
                require(sum(set(value) == {'Point'} for value in curve) == 3, f'{name}: expected three points')
                require(sum(set(value) == {'Real'} for value in curve) == expected_reals, f'{name}: wrong real count')
                require(len(curve) == 3 + expected_reals, f'{name}: unexpected curve values')
            summary.append({'input': str(path), 'entities': 211, 'regions': 2,
                            'typed_handles': HANDLES, 'points': 3, 'reals': expected_reals})
            print(f'{name}: 211 entities, 2 REGIONs, five typed arcs (3 points/{expected_reals} reals)')
    finally:
        after = snapshot()
        dump('after-verification.json', after)
        dump('functional-summary.json', summary)
        identities(after, resolved=True)
        unchanged(before, after)
        require(sha(APP / 'Cargo.lock') == prepared['app_lock_sha256'], 'Resolved lock changed during runtime')
        require(sha(BINARY) == binary_hash, 'Binary changed during runtime')
    print('Seven inputs passed; source/private input hashes unchanged. Binary SHA256:', binary_hash)
    print('Limitation: serve open drops ReadStats; this does not prove zero recovery or strict-open success.')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(124 if 'exit 124;' in str(error) else 1)
