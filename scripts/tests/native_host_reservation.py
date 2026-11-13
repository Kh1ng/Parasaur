#!/usr/bin/env python3
"""Linux/systemd acceptance: python3 scripts/tests/native_host_reservation.py /abs/gah.

Uses disposable configuration and units, real host admission settings and kernel
locks. No model, forge, controller or installed service is invoked/reconfigured.
Requires a unified cgroup user manager with systemd-notify --fd (systemd 254+)
and enough real headroom for the default 4 GiB implementation reservation.
"""
import fcntl
import datetime
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import uuid


def run(*argv, env=None, check=True):
    return subprocess.run(argv, env=env, check=check, capture_output=True,
                          text=True, timeout=20)


def ctl(*argv, check=True):
    return run('/usr/bin/systemctl', '--user', *argv, check=check)


def properties(unit):
    result = ctl('show', unit, '--property=LoadState,ActiveState,SubState,MainPID,ControlGroup,NFileDescriptorStore', check=False)
    return dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)


def wait_for(predicate):
    deadline = time.monotonic() + 8
    while not predicate():
        assert time.monotonic() < deadline, 'Timed out awaiting disposable unit state'
        time.sleep(.05)


def is_locked(path):
    with path.open('r+') as file:
        try:
            fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return False
        except BlockingIOError:
            return True


def empty(group):
    return not group.exists() or 'populated 0' in (group / 'cgroup.events').read_text().splitlines()


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    mode = sys.argv[2] if len(sys.argv) > 2 else 'running'
    assert mode in ('running', 'release-unused', 'release-unused-lost-reply', 'release-unused-broker', 'http-broker')
    assert Path('/sys/fs/cgroup/cgroup.controllers').exists()
    # Read-only inventory before the fixture. Never clean another worker's file.
    runtime = Path(os.environ['XDG_RUNTIME_DIR'])
    registry = runtime / 'gah-node-capacity'
    previous = set(registry.glob('*.lease'))
    run_id = str(uuid.uuid4())
    holder = f'gah-native-capacity-{run_id}.service'
    job = f'gah-native-job-{run_id}.service'
    with tempfile.TemporaryDirectory(prefix='parasaur-native-host-', dir=Path.home() / '.cache') as directory:
        root = Path(directory)
        repo = root / 'repo'
        repo.mkdir()
        canary = root / 'backend-invoked'
        executable = root / 'codex'
        executable.write_text(f'#!/bin/sh\ntouch {canary}\nexit 91\n')
        executable.chmod(0o700)
        common = f'''registry_central_url = "https://native-host-fixture.invalid"
artifact_root = {json.dumps(str(root / 'artifacts'))}
[profiles.fixture]
display_name = "Native host fixture"
repo_id = "native-host-fixture"
repo = "fixture/diagnostic"
provider = "github"
local_path = {json.dumps(str(repo))}
artifact_root = {json.dumps(str(root / 'artifacts'))}
default_target_branch = "main"
codex_path = {json.dumps(str(executable))}
[profiles.fixture.routing]
improve_candidates = [{{backend="codex",model="fixture-model"}}]
'''
        central, worker = root / 'central.toml', root / 'worker.toml'
        for path, role in [(central, 'central'), (worker, 'worker')]:
            path.write_text(f'[defaults]\nnode_role="{role}"\n' + common)
            path.chmod(0o600)
        identity = root / 'identity.json'
        identity.write_text('{"node_id":"native-host-fixture"}')
        identity.chmod(0o600)
        environment = {k: v for k, v in os.environ.items() if not k.startswith(('PARASAUR_', 'GAH_'))}
        environment['PARASAUR_COORDINATOR_IDENTITY_PATH'] = str(identity)
        environment['PARASAUR_LEDGER_PATH'] = str(root / 'history.jsonl')
        request = root / 'request.json'
        request.write_text(json.dumps({'args': {
            'profile': 'fixture', 'mode': 'fix', 'backend': 'codex', 'model': 'fixture-model',
            'target': '#1', 'current_branch': False, 'dry_run': False, 'retries': 2,
            'allow_draft_fail': False, 'prod': False, 'issue_intake_override': False,
            'allow_unknown_red_baseline': False, 'escalate': False, 'skip_validation_gate': False,
            'work_id': '#1', 'run_id': run_id, 'dispatch_reason': 'initial',
        }}))
        context = root / 'runs' / run_id / 'context'
        context.parent.parent.mkdir(mode=0o700)
        context.parent.mkdir(mode=0o700)
        binding = json.loads(run(str(binary), 'native-handoff', 'prepare', '--request-file', str(request),
                                 '--output', str(context), '--node-id', 'native-host-fixture',
                                 '--central-url', 'https://native-host-fixture.invalid',
                                 '--config-path', str(central), env=environment).stdout)
        if mode == 'http-broker':
            expected = root / 'binding.json'
            expected.write_text(json.dumps(binding))
            expected.chmod(0o600)
            probe = Path(__file__).with_name('native_worker_http.mjs')
            result = run(sys.argv[3], str(probe), sys.argv[4], str(binary), str(root), env=environment, check=False)
            assert result.returncode == 0, result.stderr
            report = json.loads(result.stdout)
            assert report['passed'] and not canary.exists()
            print(json.dumps(report, indent=2))
            return
        arguments = [str(binary), 'native-handoff', 'host-reserve', '--context-dir', str(context),
                     '--expected-context', binding['context_sha256'], '--config-path', str(worker)]
        outside = run(*arguments, '--work-dir', str(root / 'outside'), env=environment, check=False)
        assert outside.returncode != 0 and not outside.stdout.strip(), 'Non-holder must not reserve'
        # exec preserves the holder's main PID; fixture output stays in this
        # private directory rather than being copied to the user journal.
        launcher = root / 'launch.py'
        stdout, stderr = root / 'reserved.json', root / 'reserve-error'
        launcher.write_text('import os\n'
                            f'env={{k:v for k,v in os.environ.items() if not k.startswith(("PARASAUR_", "GAH_"))}}\n'
                            f'env["PARASAUR_COORDINATOR_IDENTITY_PATH"]={str(identity)!r}\n'
                            f'os.dup2(os.open({str(stdout)!r},os.O_CREAT|os.O_WRONLY,0o600),1)\n'
                            f'os.dup2(os.open({str(stderr)!r},os.O_CREAT|os.O_WRONLY,0o600),2)\n'
                            f'os.execve({str(binary)!r},{arguments + ["--work-dir", str(root / "work")]!r},env)\n')
        group = None
        stop_count = root / 'stop-count'
        stopper = root / 'stopper.py'
        stopper.write_text(f'from pathlib import Path\nimport time\nwith Path({str(stop_count)!r}).open("a") as f:f.write("stop\\n")\ntime.sleep(4)\n')
        stop_properties = ['-p', f'ExecStop=/usr/bin/python3 {stopper}'] if mode == 'release-unused-lost-reply' else []
        try:
            result = run('/usr/bin/systemd-run', '--user', '--collect', '--unit=' + holder,
                         '-p', 'Type=oneshot', '-p', 'RemainAfterExit=yes',
                         '-p', 'FileDescriptorStoreMax=1', '-p', 'FileDescriptorStorePreserve=yes',
                         '-p', 'NotifyAccess=all', '-p', 'MemoryMax=512M', '-p', 'TimeoutStartSec=20',
                         *stop_properties,
                         '/usr/bin/python3', str(launcher), check=False)
            assert result.returncode == 0, stderr.read_text() if stderr.exists() else result.stderr
            held = properties(holder)
            assert held['ActiveState'] == 'active' and held['SubState'] == 'exited', held
            assert held['MainPID'] == '0' and held['NFileDescriptorStore'] == '1', held
            response = json.loads(stdout.read_text())
            assert response['binding'] == binding
            assert response['capacity_reserved'] and response['execution_authorized'] is False
            assert response['holder_unit'] == holder
            journal = json.loads((root / 'work' / 'worker.json').read_text())
            assert journal['binding'] == binding and journal['phase'] == 'ready'
            assert journal['holder_invocation'] == response['holder_invocation']
            assert journal['start'] is None
            expected = root / 'binding.json'
            expected.write_text(json.dumps(binding))
            expected.chmod(0o600)

            def inspect():
                return json.loads(run(str(binary), 'native-handoff', 'worker-status',
                                      '--binding-file', str(expected), '--work-dir', str(root / 'work'),
                                      env=environment).stdout)

            observed = inspect()
            assert observed['state'] == 'ready' and observed['capacity_held']
            assert observed['start_offered'] is False and observed['execution_authorized'] is False
            lock_ready = root / 'writer-ready'
            lock_code = ('import fcntl,time\nfrom pathlib import Path\n'
                         f'file=open({str(root / "work" / "worker.lock")!r},"r+")\n'
                         'fcntl.flock(file,fcntl.LOCK_EX)\n'
                         f'Path({str(lock_ready)!r}).write_text("locked")\n'
                         'time.sleep(20)\n')
            writer = subprocess.Popen(['/usr/bin/python3', '-c', lock_code],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                wait_for(lock_ready.exists)
                busy = run(str(binary), 'native-handoff', 'worker-status', '--binding-file', str(expected),
                           '--work-dir', str(root / 'work'), env=environment, check=False)
                assert busy.returncode != 0 and 'already in progress' in busy.stderr
                writer.kill()
                writer.wait(timeout=3)
                assert inspect()['state'] == 'ready'
                assert json.loads((root / 'work' / 'worker.json').read_text()) == journal
            finally:
                if writer.poll() is None:
                    writer.kill()
                    writer.wait(timeout=3)
            leases = [path for path in set(registry.glob('*.lease')) - previous
                      if path.read_text() == '4294967296 2.000\n' and is_locked(path)]
            assert len(leases) == 1, 'Expected this real default admission lease'
            lease = leases[0]
            if mode == 'release-unused-broker':
                probe = Path(__file__).with_name('native_reservation_broker.mjs')
                result = run(sys.argv[3], str(probe), sys.argv[4], str(binary), str(root), env=environment, check=False)
                assert result.returncode == 0, result.stderr
                report = json.loads(result.stdout)
                assert report['passed'] and not is_locked(lease)
                assert inspect()['state'] == 'reservation_released'
                assert not canary.exists() and not (root / 'work' / 'result.json').exists()
                print(json.dumps(report, indent=2))
                return
            if mode != 'running':
                release_arguments = [str(binary), 'native-handoff', 'release-reservation', '--binding-file', str(expected),
                                     '--work-dir', str(root / 'work'), '--reservation-id']

                def release(reservation=response['holder_invocation'], check=True):
                    return run(*release_arguments, reservation, env=environment, check=check)

                assert release('f' * 32, False).returncode != 0 and is_locked(lease)
                assert not (root / 'work' / 'reservation-release.json').exists()
                # Even a job absent from the journal is not safe to stop or ignore.
                run('/usr/bin/systemd-run', '--user', '--collect', '--unit=' + job,
                    '-p', 'Type=exec', '-p', 'MemoryMax=64M', '/bin/sleep', '30')
                group = Path('/sys/fs/cgroup') / properties(job)['ControlGroup'].lstrip('/')
                assert release(check=False).returncode != 0 and is_locked(lease)
                assert properties(job)['MainPID'] != '0'
                assert inspect()['state'] == 'unknown'
                ctl('stop', job)
                wait_for(lambda: properties(job)['LoadState'] == 'not-found')
                # A cancelled reservation cannot later consume a start grant.
                grant = {key: binding[key] for key in ('run_id', 'node_id', 'context_sha256')}
                grant.update(reservation_id=response['holder_invocation'], token='c' * 64,
                             expires_at=(datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=10)).isoformat())
                entry_environment = {key: value for key, value in environment.items()
                                     if key not in ('PARASAUR_LEDGER_PATH', 'PARASAUR_EVENTS_PATH', 'PARASAUR_RECONCILIATION_PATH')}
                replay = subprocess.run([str(binary), 'native-handoff', 'start', '--context-dir', str(context),
                                         '--expected-context', binding['context_sha256'], '--work-dir', str(root / 'work'),
                                         '--config-path', str(worker)], env=entry_environment,
                                        input=json.dumps({'service_token': 'fixture-only', 'grant': grant}),
                                        capture_output=True, text=True, timeout=20)
                assert replay.returncode != 0 and 'reservation release was offered' in replay.stderr, replay.stderr
                assert json.loads((root / 'work' / 'worker.json').read_text())['start'] is None
                capacity_path = root / 'work' / 'capacity.json'
                capacity = capacity_path.read_bytes()
                changed = json.loads(capacity)
                changed['holder_invocation'] = 'f' * 32
                capacity_path.write_text(json.dumps(changed))
                assert release(check=False).returncode != 0 and is_locked(lease)
                assert properties(holder)['ActiveState'] == 'active'
                capacity_path.write_bytes(capacity)
                if mode == 'release-unused-lost-reply':
                    assert release(check=False).returncode != 0
                    assert json.loads((root / 'work' / 'reservation-release.json').read_text())['phase'] == 'stop_intent'
                    wait_for(lambda: properties(holder)['LoadState'] == 'not-found')
                released = json.loads(release().stdout)
                assert released['binding'] == binding and released['capacity_released']
                assert released['dispatch_started'] is False and not is_locked(lease)
                assert json.loads(release().stdout) == released
                assert inspect()['state'] == 'reservation_released' and inspect()['capacity_held'] is False
                assert (context / 'history-0000.jsonl').exists()
                assert not (root / 'work' / 'result.json').exists() and not canary.exists()
                if mode == 'release-unused-lost-reply':
                    assert stop_count.read_text().splitlines() == ['stop']
                print(json.dumps({'passed': True, 'mode': mode, 'realDefaultHostAdmission': True,
                                  'unrecordedJobRetainsCapacity': True, 'changedHolderRefused': True,
                                  'cancelledStartRefused': True, 'physicalCapacityReleased': True,
                                  'repeatedReleaseIdempotent': True, 'canonicalResultInvented': False,
                                  'lostStopReplyRecovered': mode == 'release-unused-lost-reply', 'agentInvoked': False}, indent=2))
                return
            ready = root / 'job.json'
            child = root / 'child.py'
            child.write_text('import json,os,subprocess,time\nfrom pathlib import Path\n'
                             'child=subprocess.Popen(["/bin/sleep","30"],close_fds=True)\n'
                             f'Path({str(ready)!r}).write_text(json.dumps({{"pid":os.getpid(),"child":child.pid}}))\n'
                             'time.sleep(30)\n')
            run('/usr/bin/systemd-run', '--user', '--collect', '--unit=' + job,
                '-p', 'Type=exec', '-p', 'ExitType=cgroup', '-p', 'RemainAfterExit=yes', '-p', 'KillMode=control-group',
                '-p', 'MemoryMax=64M', '-p', 'TimeoutStopSec=2', '-p', 'RuntimeMaxSec=30',
                '/usr/bin/python3', str(child))
            wait_for(ready.exists)
            group = Path('/sys/fs/cgroup') / properties(job)['ControlGroup'].lstrip('/')
            actual = json.loads(ready.read_text())
            start_ticks = Path(f'/proc/{actual["pid"]}/stat').read_text().rsplit(')', 1)[1].split()[19]
            invocation = ctl('show', job, '-p', 'InvocationID', '--value').stdout.strip()
            # Controlled entry record: this part proves real process/service
            # inspection, not a central grant exchange or model execution.
            now = datetime.datetime.now(datetime.timezone.utc)
            journal.update(phase='start_intent', start={'token_sha256': 'c' * 64,
                           'expires_at': (now + datetime.timedelta(seconds=10)).isoformat()},
                           consumption_offered=True,
                           launch={'invocation': invocation, 'pid': actual['pid'], 'start_ticks': start_ticks},
                           execution={'invocation': invocation, 'pid': actual['pid'], 'start_ticks': start_ticks,
                                      'consumed_at': now.isoformat()})
            journal_path = root / 'work' / 'worker.json'
            journal_path.write_text(json.dumps(journal))
            assert inspect()['state'] == 'running'
            refused_release = run(str(binary), 'native-handoff', 'release-reservation', '--binding-file', str(expected),
                                  '--work-dir', str(root / 'work'), '--reservation-id', response['holder_invocation'],
                                  env=environment, check=False)
            assert refused_release.returncode != 0 and is_locked(lease)
            assert not (root / 'work' / 'reservation-release.json').exists()
            journal['execution']['start_ticks'] = str(int(start_ticks) + 1)
            journal['launch']['start_ticks'] = str(int(start_ticks) + 1)
            journal_path.write_text(json.dumps(journal))
            assert inspect()['state'] == 'unknown'
            journal['execution']['start_ticks'] = start_ticks
            journal['launch']['start_ticks'] = start_ticks
            journal['execution']['invocation'] = 'f' * 32
            journal['launch']['invocation'] = 'f' * 32
            journal_path.write_text(json.dumps(journal))
            assert inspect()['state'] == 'unknown'
            journal['execution']['invocation'] = invocation
            journal['launch']['invocation'] = invocation
            journal_path.write_text(json.dumps(journal))
            os.kill(json.loads(ready.read_text())['pid'], signal.SIGKILL)
            wait_for(lambda: properties(job)['MainPID'] == '0')
            assert not empty(group) and is_locked(lease)
            observed = inspect()
            assert observed['state'] == 'unknown' and observed['capacity_held'] is True
            refused_result = run(str(binary), 'native-handoff', 'result', '--binding-file', str(expected),
                                 '--context-dir', str(context), '--work-dir', str(root / 'work'),
                                 env=environment, check=False)
            assert refused_result.returncode != 0 and 'Native job is not terminal' in refused_result.stderr
            assert not (root / 'work' / 'result.json').exists()
            ctl('stop', job)
            assert empty(group) and is_locked(lease)
            ctl('stop', holder)
            assert not is_locked(lease)
            observed = inspect()
            assert observed['state'] == 'unknown' and observed['capacity_held'] is None
            assert not canary.exists(), 'Admission must never invoke a backend'
            print(json.dumps({'passed': True, 'realDefaultHostAdmission': True,
                              'workerJournalWriterCrashRecovery': True,
                              'realJobIdentityInspectionWithControlledEntry': True,
                              'mainCrashWithLiveDescendantRetainsCapacity': True,
                              'jobStopDoesNotReleaseCapacity': True,
                              'holderExitReleasesKernelLock': True, 'agentInvoked': False}, indent=2))
        finally:
            stopped = ctl('stop', job, check=False)
            remaining = properties(job)
            cgroup = remaining.get('ControlGroup')
            if cgroup:
                group = Path('/sys/fs/cgroup') / cgroup.lstrip('/')
            # A timed-out start may have created the service before returning.
            # Observe it after stop; a missing local handle is not proof of exit.
            if ((stopped.returncode == 0 or remaining.get('LoadState') == 'not-found')
                    and remaining.get('ActiveState') in ('inactive', 'failed')
                    and (group is None or empty(group))):
                ctl('stop', holder, check=False)
                ctl('clean', '--what=fdstore', holder, check=False)
            for unit in (holder, job):
                ctl('reset-failed', unit, check=False)


if __name__ == '__main__':
    main()
