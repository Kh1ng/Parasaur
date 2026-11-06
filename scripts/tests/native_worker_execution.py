#!/usr/bin/env python3
"""Real WSL/systemd worker wrapper with controlled central and backend fixtures.

Usage: python3 scripts/tests/native_worker_execution.py /absolute/gah
No paid model or real forge. Default physical capacity guards remain enabled.
Launch, durable StartIntent, consumption, process scope and operations are real.
The start CLI receives private stdin and delivers it through its PID-verified socket.
"""
import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile
import threading
import time
import uuid

from native_host_reservation import ctl, empty, properties, run as host_run


def run(*args, **kwargs):
    result = host_run(*args, **dict(kwargs, check=False))
    assert result.returncode == 0, result.stderr[-2000:]
    return result


def stamp():
    return datetime.datetime.now(datetime.timezone.utc)


def text_date(value):
    return value.isoformat(timespec='milliseconds').replace('+00:00', 'Z')


def main():
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    owner = secrets.token_hex(32)
    calls = []
    grant = None
    denied = False
    publication_denied = False

    class Central(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            assert self.path.startswith('/api/skills/resolve?')
            data = json.dumps({'profile': 'fixture', 'backend': 'codex', 'instance': None,
                               'source': 'fixture', 'skills': []}).encode()
            self.send_response(200)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_POST(self):
            if not self.path.startswith('/api/native-dispatch/'):
                # Ordinary best-effort telemetry in this disposable workflow.
                self.send_response(204)
                self.end_headers()
                return
            assert self.headers.get('Authorization') == 'Bearer ' + owner
            size = int(self.headers['Content-Length'])
            assert size < 16384
            body = json.loads(self.rfile.read(size))
            calls.append(self.path)
            now = stamp()
            if self.path == '/api/native-dispatch/consume':
                assert body == grant
                result = {key: body[key] for key in ('run_id', 'node_id', 'context_sha256', 'reservation_id')}
                result.update(allowed=True, consumed_at=text_date(now), expires_at=grant['expires_at'])
                status = 200
            else:
                assert self.path == '/api/native-dispatch/operation'
                expected_sequence = 2 if body['purpose'] == 'publish' else 1
                assert body['sequence'] == expected_sequence
                assert body['purpose'] == 'attempt' or publication_denied
                assert all(body[key] == grant[key] for key in ('run_id', 'node_id', 'context_sha256', 'reservation_id'))
                result = dict(body, allowed=True, authorized_at=text_date(now),
                              expires_at=text_date(now + datetime.timedelta(seconds=9)))
                status = 200
                if denied or (publication_denied and body['purpose'] == 'publish'):
                    result, status = {'allowed': False, 'reason': 'factory_paused'}, 409
            data = json.dumps(result).encode()
            self.send_response(status)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Central)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for case in ('allowed', 'denied', 'publication-denied', 'lost-start-reply', 'running-result', 'provider-claims'):
            denied = case == 'denied'
            publication_denied = case == 'publication-denied'
            run_id = str(uuid.uuid4())
            holder = f'gah-native-capacity-{run_id}.service'
            job = f'gah-native-job-{run_id}.service'
            with tempfile.TemporaryDirectory(prefix='parasaur-native-execution-', dir=Path.home() / '.cache') as directory:
                root = Path(directory)
                repo, bins = root / 'repo', root / 'bin'
                repo.mkdir()
                bins.mkdir()
                run('git', 'init', '-b', 'main', str(repo))
                run('git', '-C', str(repo), 'config', 'user.name', 'Native Fixture')
                run('git', '-C', str(repo), 'config', 'user.email', 'fixture@invalid')
                (repo / 'README.md').write_text('fixture\n')
                target = repo / 'task.md'
                target.write_text('# Fixture task\n\nUpdate the README in this disposable repository.\n')
                run('git', '-C', str(repo), 'add', 'README.md', 'task.md')
                run('git', '-C', str(repo), 'commit', '-m', 'fixture')
                origin = root / 'github' / 'fixture' / 'diagnostic.git'
                origin.parent.mkdir(parents=True)
                run('git', 'init', '--bare', str(origin))
                run('git', '-C', str(repo), 'remote', 'add', 'origin', str(origin))
                run('git', '-C', str(repo), 'push', '-u', 'origin', 'main')
                marker = root / 'backend-invoked'
                backend = bins / 'codex'
                backend.write_text('#!/bin/sh\n' +
                    f'touch {marker}\n' + ('sleep 2\n' if case == 'running-result' else '') +
                    'printf "worker edit\\n" >> README.md\n' +
                    'printf \'{"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":7}}\\n\'\n')
                backend.chmod(0o700)
                forge = bins / 'gh'
                forge.write_text('#!/bin/sh\nprintf "[]\\n"\n')
                forge.chmod(0o700)
                origin_url = f'http://127.0.0.1:{server.server_port}'
                claim_policy = '[profiles.fixture.publishing.issue_claim]\nmode="github_assignee"\n' if case == 'provider-claims' else ''
                common = f'''registry_central_url = {json.dumps(origin_url)}
artifact_root = {json.dumps(str(root / 'artifacts'))}
worktree_base = {json.dumps(str(root / 'worktrees'))}
[profiles.fixture]
display_name = "Native execution fixture"
repo_id = "native-execution-fixture"
repo = "fixture/diagnostic"
provider = "github"
local_path = {json.dumps(str(repo))}
artifact_root = {json.dumps(str(root / 'artifacts'))}
default_target_branch = "main"
codex_path = {json.dumps(str(backend))}
validation_commands = ["true"]
[profiles.fixture.publishing]
allow_pull_request_creation = {str(publication_denied).lower()}
{claim_policy}
[profiles.fixture.routing]
improve_candidates = [{{backend="codex",model="fixture-model"}}]
'''
                central, worker = root / 'central.toml', root / 'worker.toml'
                for path, role in ((central, 'central'), (worker, 'worker')):
                    path.write_text(f'[defaults]\nnode_role="{role}"\n' + common)
                    path.chmod(0o600)
                identity = root / 'identity.json'
                identity.write_text('{"node_id":"native-execution-fixture"}')
                identity.chmod(0o600)
                environment = {k: v for k, v in os.environ.items() if not k.startswith('GAH_')}
                environment['GAH_COORDINATOR_IDENTITY_PATH'] = str(identity)
                environment['PATH'] = str(bins) + ':/usr/bin:/bin'
                home = root / 'home'
                home.mkdir()
                environment['HOME'] = str(home)
                run('git', 'config', '--file', str(home / '.gitconfig'),
                    'url.file://' + str(root / 'github') + '/.insteadOf', 'https://github.com/')
                request = root / 'request.json'
                request.write_text(json.dumps({'args': {
                    'profile': 'fixture', 'mode': 'fix', 'backend': 'codex', 'model': 'fixture-model',
                    'target': str(target), 'current_branch': False, 'dry_run': False, 'retries': 0,
                    'allow_draft_fail': False, 'prod': False, 'issue_intake_override': False,
                    'allow_unknown_red_baseline': False, 'escalate': False, 'skip_validation_gate': True,
                    'work_id': '#fixture', 'run_id': run_id, 'dispatch_reason': 'initial',
                }}))
                context, work = root / 'context', root / 'work'
                binding = json.loads(run(binary, 'native-handoff', 'prepare', '--request-file', str(request),
                    '--output', str(context), '--node-id', 'native-execution-fixture',
                    '--central-url', origin_url, '--config-path', str(central), env=environment).stdout)
                stdout, stderr = root / 'stdout', root / 'stderr'
                reserve = [binary, 'native-handoff', 'host-reserve', '--context-dir', str(context),
                           '--expected-context', binding['context_sha256'], '--work-dir', str(work),
                           '--config-path', str(worker)]
                flags = []
                for key in ('GAH_COORDINATOR_IDENTITY_PATH', 'PATH', 'HOME'):
                    flags += ['--setenv=' + key + '=' + environment[key]]
                group = None
                # Capture the exact delegated path before systemd drops its
                # ControlGroup property after terminal ExitType=cgroup state.
                group_path = None
                try:
                    run('/usr/bin/systemd-run', '--user', '--unit=' + holder, *flags,
                        '-p', 'Type=oneshot', '-p', 'RemainAfterExit=yes', '-p', 'FileDescriptorStoreMax=1',
                        '-p', 'FileDescriptorStorePreserve=yes', '-p', 'NotifyAccess=all', '-p', 'MemoryMax=512M',
                        '-p', 'StandardOutput=file:' + str(stdout), '-p', 'StandardError=file:' + str(stderr), *reserve)
                    journal_path = work / 'worker.json'
                    journal = json.loads(journal_path.read_text())
                    assert journal['phase'] == 'ready'
                    grant = {key: binding[key] for key in ('run_id', 'node_id', 'context_sha256')}
                    grant.update(reservation_id=journal['holder_invocation'], token=secrets.token_hex(32),
                                 expires_at=text_date(stamp() + datetime.timedelta(seconds=9)))
                    start = [binary, 'native-handoff', 'start', '--context-dir', str(context),
                               '--expected-context', binding['context_sha256'], '--work-dir', str(work),
                               '--config-path', str(worker)]
                    calls.clear()
                    launched = subprocess.run(start, env=environment, input=json.dumps({'grant': grant, 'service_token': owner}),
                        stdout=subprocess.DEVNULL if case == 'lost-start-reply' else subprocess.PIPE,
                        stderr=subprocess.PIPE, text=True, timeout=10)
                    if case == 'provider-claims':
                        journal = json.loads(journal_path.read_text())
                        assert launched.returncode != 0 and not calls and not marker.exists()
                        assert journal['phase'] == 'ready' and not journal['consumption_offered'] and not journal['execution']
                        assert 'provider-assignee claim integration is unavailable' in launched.stderr
                        print(json.dumps({'case': case, 'refusedBeforeConsumption': True,
                            'backendFixtureInvoked': False, 'realModelInvoked': False}), flush=True)
                        continue
                    assert launched.returncode == 0, launched.stderr
                    if case != 'lost-start-reply':
                        reply = json.loads(launched.stdout)
                        assert reply['launch_confirmed'] and reply['execution_authorized'] is False
                    stdout, stderr = work / 'job.stdout', work / 'job.stderr'
                    collect = [binary, 'native-handoff', 'result', '--context-dir', str(context),
                               '--binding-file', str(context / 'binding.json'), '--work-dir', str(work)]
                    if case == 'running-result':
                        deadline = time.monotonic() + 5
                        while not marker.exists():
                            assert time.monotonic() < deadline
                            time.sleep(.02)
                        pending = host_run(*collect, env=environment, check=False)
                        assert pending.returncode != 0 and not pending.stdout and 'Native job' in pending.stderr
                        assert not (work / 'result.json').exists()
                    group_path = properties(job).get('ControlGroup')
                    deadline = time.monotonic() + 60
                    while True:
                        observed = properties(job)
                        if observed.get('SubState') in ('exited', 'failed'):
                            break
                        assert time.monotonic() < deadline, 'Disposable wrapper did not terminate'
                        time.sleep(.1)
                    # Diagnostics are fixture-only and still must not contain
                    # either credential. Never print them wholesale.
                    output = stdout.read_text() + stderr.read_text()
                    assert owner not in output and grant['token'] not in output
                    for path in root.rglob('*'):
                        if path.is_file():
                            data = path.read_bytes()
                            assert owner.encode() not in data and grant['token'].encode() not in data, 'Credential persisted in fixture artifact'
                    expected_calls = ['/api/native-dispatch/consume', '/api/native-dispatch/operation']
                    if publication_denied:
                        expected_calls.append('/api/native-dispatch/operation')
                    assert calls == expected_calls, (calls, output[-2000:])
                    journal = json.loads(journal_path.read_text())
                    assert journal['execution']['pid'] > 0 and journal['consumption_offered']
                    assert journal['version'] == 3
                    assert all(journal['launch'][key] == journal['execution'][key] for key in ('invocation', 'pid', 'start_ticks'))
                    assert marker.exists() is not denied, output[-2000:]
                    entries = [json.loads(row) for row in (work / 'ledger.jsonl').read_text().splitlines()]
                    terminal = [entry for entry in entries if entry.get('session_id') == run_id]
                    assert len(terminal) == 1, output[-2000:]
                    entry = terminal[0]
                    if denied:
                        assert not entry.get('attempts_started') and not entry.get('attempts')
                        assert journal['operation']['phase'] == 'pending'
                    else:
                        assert entry['attempts_started'] == 1 and len(entry['attempts']) == 1
                        assert journal['operation']['phase'] == ('pending' if publication_denied else 'completed'), output[-2000:]
                        assert entry['backend_exit_code'] == 0
                        if publication_denied:
                            assert not entry.get('push_attempted') and not entry.get('push_succeeded')
                            assert journal['operation']['sequence'] == 2
                    assert properties(holder)['NFileDescriptorStore'] == '1'
                    if group_path:
                        group = Path('/sys/fs/cgroup') / group_path.lstrip('/')
                    else:
                        # If the tiny fixture already exited, the full unique
                        # path is recoverable from its enclosing user slice.
                        base = ctl('show', 'app.slice', '--property=ControlGroup', '--value').stdout.strip()
                        assert base.startswith('/'), 'Missing user app slice evidence'
                        group = Path('/sys/fs/cgroup') / base.lstrip('/') / job
                    assert empty(group), 'Wrapper exit did not prove an empty cgroup'
                    recovered = run(*collect, env=environment)
                    assert owner not in recovered.stdout and grant['token'] not in recovered.stdout
                    result = json.loads(recovered.stdout)
                    receipt = json.loads(result['receipt_json'])
                    differences = {key: (entry.get(key), receipt['ledger'].get(key))
                        if isinstance(entry.get(key), (int, float)) else 'changed'
                        for key in set(entry) | set(receipt['ledger']) if entry.get(key) != receipt['ledger'].get(key)}
                    assert result['state'] == 'terminal' and receipt['ledger'] == entry, differences
                    assert receipt['execution']['termination']['exit_code'] == (1 if denied or publication_denied else 0)
                    receipt_file = root / 'receipt.json'
                    receipt_file.write_text(result['receipt_json'])
                    for expected in ('recorded', 'already_recorded'):
                        imported = json.loads(run(binary, 'ledger', 'receive-dispatch', '--binding-file',
                            str(context / 'binding.json'), '--receipt-file', str(receipt_file),
                            '--acknowledgement', '--config-path', str(central), env=environment).stdout)
                        assert imported['outcome'] == expected
                        assert imported['receipt_sha256'] == result['receipt_sha256']
                    # A cold recovery replays the committed original, even if
                    # mutable ledger data or unit metadata later disappears.
                    with (work / 'ledger.jsonl').open('a') as ledger:
                        ledger.write('CORRUPT_TAIL_AFTER_RECOVERY\n')
                    ctl('stop', job, check=False)
                    assert json.loads(run(*collect, env=environment).stdout) == result
                    spool_path = work / 'result.json'
                    original = spool_path.read_bytes()
                    changed = json.loads(original)
                    changed['receipt_sha256'] = '0' * 64
                    spool_path.write_text(json.dumps(changed))
                    assert host_run(*collect, env=environment, check=False).returncode != 0
                    spool_path.write_bytes(original)
                    assert properties(holder)['NFileDescriptorStore'] == '1'
                    # Cold start/execute processes cannot offer/consume again,
                    # including when the original launch response was discarded.
                    for action in ('start', 'execute'):
                        replay_args = list(start)
                        replay_args[2] = action
                        replay = subprocess.run(replay_args, env=environment, input=json.dumps({
                            'grant': grant, 'service_token': owner}), capture_output=True, text=True, timeout=10)
                        assert replay.returncode != 0 and len(calls) == len(expected_calls)
                    print(json.dumps({'case': case, 'actualStartConsumption': True,
                        'actualProcessScope': True, 'backendFixtureInvoked': marker.exists(),
                        'operation': journal['operation']['phase'], 'capacityRetainedAfterExit': True,
                        'replayRejected': True, 'immutableResultRecovered': True, 'canonicalImportOnce': True,
                        'startReplyDiscarded': case == 'lost-start-reply', 'realModelInvoked': False}), flush=True)
                finally:
                    ctl('stop', job, check=False)
                    try:
                        if group is not None:
                            assert empty(group)
                    finally:
                        ctl('stop', holder, check=False)
                        ctl('reset-failed', job, holder, check=False)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


if __name__ == '__main__':
    main()
