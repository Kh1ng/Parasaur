"""Strict fake CLI: unsupported commands fail and every argv is recorded."""
import fcntl
import json
import os
import re
import sys
from urllib.parse import parse_qsl, urlsplit

from api import api, cli


def parse(argv):
    options, fields, positional = {}, {}, []
    values = {'--method', '-X', '--hostname', '--repo', '-R', '--jq', '--json',
              '--state', '--limit', '--base', '--head', '--title', '--body',
              '--match-head-commit', '--input', '--author', '--commit-id'}
    switches = {'--paginate', '--silent', '--draft', '--squash', '--admin',
                '--delete-branch', '--approve', '--request-changes', '--comment'}
    i = 0
    while i < len(argv):
        arg = argv[i]
        if arg in {'-f', '-F', '--field', '--raw-field'}:
            i += 1
            key, value = argv[i].split('=', 1)
            if arg in {'-F', '--field'}:
                try:
                    value = json.loads(value)
                except ValueError:
                    pass
            if key.endswith('[]'):
                fields.setdefault(key[:-2], []).append(value)
            else:
                fields[key] = value
        elif arg in values:
            i += 1
            options[arg] = argv[i]
        elif arg in switches:
            options[arg] = True
        elif arg.startswith('-'):
            raise ValueError(f'unsupported flag {arg}')
        else:
            positional.append(arg)
        i += 1
    if '--input' in options:
        with sys.stdin if options['--input'] == '-' else open(options['--input']) as source:
            fields.update(json.load(source))
    return positional, options, fields


def render(value, options):
    query = options.get('--jq')
    if options.get('--silent'):
        return ''
    if query:
        if query in {'.login', '.state'}:
            return value[query[1:]]
        if query == '.assignees[].login':
            return '\n'.join(a['login'] for a in value['assignees'])
        if query == '.[].name':
            return '\n'.join(a['name'] for a in value)
        claim = re.fullmatch(r'\.\[\] \| select\(\.body \| startswith\("(.*)"\)\) \| '
                             r'\{id, login: \(\.user.login // ""\), created_at, updated_at\}', query)
        if claim:
            return '\n'.join(json.dumps(dict(id=c['id'], login=c['user']['login'],
                                created_at=c['created_at'], updated_at=c['updated_at']))
                             for c in value if c['body'].startswith(claim[1]))
        raise ValueError(f'unsupported jq {query}')
    if '--json' in options:
        keys = options['--json'].split(',')
        project = lambda row: {key: row[key] for key in keys}
        value = [project(row) for row in value] if isinstance(value, list) else project(value)
    return value if isinstance(value, str) else json.dumps(value)


def run(state, provider, argv):
    args, options, fields = parse(argv)
    if args[0] != 'api':
        return render(cli(state, provider, args, options), options)
    if len(args) != 2:
        raise ValueError('expected one API endpoint')
    url = urlsplit(args[1])
    path = url.path.strip('/')
    fields = dict(parse_qsl(url.query)) | fields
    # gh's fields imply POST unless --method is explicit. glab follows the
    # explicit method at every production call site.
    method = options.get('--method', options.get('-X', 'POST' if fields and not url.query else 'GET'))
    for effect in state['effects']:
        if (effect['endpoint'] == path and effect['method'] == method
                and all(arg in argv for arg in effect.get('argv_contains', []))):
            effect['seen'] = effect.get('seen', 0) + 1
            if effect['seen'] == effect['at']:
                for patch in effect['patches']:
                    target = state
                    for key in patch['path'][:-1]:
                        target = target[key]
                    target[patch['path'][-1]] = patch['value']
    value = api(state, provider, path, method, fields)
    if isinstance(value, list):
        # --paginate returns all pages; otherwise honor explicit page sizes.
        size = int(fields.get('per_page', 30))
        start = (int(fields.get('page', 1)) - 1) * size
        if options.get('--paginate') and not options.get('--jq'):
            pages = [value[i:i + size] for i in range(start, len(value), size)] or [[]]
            return '\n'.join(render(page, options) for page in pages)
        if not options.get('--paginate'):
            value = value[start:start + size]
    return render(value, options)


state_path, log_path, provider, *argv = sys.argv[1:]
with open(state_path + '.lock', 'a') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    with open(state_path) as source:
        state = json.load(source)
    # Inherits the clock seam from #1515 without depending on its Rust code.
    state['now'] = os.environ.get('GAH_TEST_NOW', state['now'])
    state['calls'].append([provider] + argv)
    with open(log_path, 'a') as log:
        log.write(' '.join(argv) + '\n')
    try:
        output = run(state, provider, argv)
        status = 0
    except (ValueError, KeyError, IndexError, StopIteration) as error:
        output, status = '', 1
        print(f'fake {provider}: {error}; argv={argv!r}', file=sys.stderr)
    with open(state_path + '.tmp', 'w') as dest:
        json.dump(state, dest, indent=2)
    os.replace(state_path + '.tmp', state_path)
    if output:
        print(output)
    sys.exit(status)
