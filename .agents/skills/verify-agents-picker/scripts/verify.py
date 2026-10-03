#!/usr/bin/env python3
"""Drive the release TUI on an isolated tmux server; retain proof artifacts."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[4]
BINARY = ROOT / 'target/release/agents-picker'


def doctor(binary):
    for name in ('tmux',):
        if not shutil.which(name):
            raise RuntimeError(f'{name} is missing')
    result = subprocess.run([str(binary), '--help'], capture_output=True, text=True, timeout=5)
    if result.returncode or 'run the picker TUI' not in result.stdout:
        raise RuntimeError(f'binary doctor failed: {result.stderr}')
    return {'binary': str(binary), 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'help': result.stdout, 'tmux': subprocess.check_output(['tmux', '-V'], text=True).strip()}


def fixture(args):
    directory = Path(os.environ['PICKER_VERIFY_SCRATCH'])
    with (directory / 'calls.jsonl').open('a') as out:
        out.write(json.dumps({'time': time.time(), 'args': args}) + '\n')
    state = json.loads((directory / 'fixture.json').read_text())
    if args == ['agent', 'list']:
        result = {'agents': state['agents']}
    elif args == ['workspace', 'list']:
        result = {'workspaces': [{'workspace_id': 'verify-w', 'label': 'Verify', 'tab_count': 2}]}
    elif args == ['tab', 'list']:
        result = {'tabs': [{'tab_id': 'verify-t', 'label': 'Checks', 'number': 1}]}
    elif args[:2] == ['agent', 'read']:
        result = {'read': {'text': 'SNAPSHOT ' + args[2]}}
    elif args[:3] == ['terminal', 'session', 'observe']:
        if state.get('fallback'):
            print('fixture stream unavailable', file=sys.stderr)
            return 1
        width = int(args[args.index('--cols') + 1])
        height = int(args[args.index('--rows') + 1])
        for seq in range(1, 41):
            text = '\x1b[2J\x1b[HLIVE ' + args[3] + f' frame {seq}'
            print(json.dumps({'type': 'terminal.frame', 'encoding': 'ansi', 'full': True,
                              'width': width, 'height': height, 'seq': seq,
                              'bytes': base64.b64encode(text.encode()).decode()}), flush=True)
            time.sleep(.2)
        return 0
    elif len(args) == 3 and args[0] in ('workspace', 'tab', 'agent') and args[1] == 'focus':
        result = {}
    elif args == ['plugin', 'pane', 'open', '--plugin', 'yxhta.agents-picker', '--entrypoint', 'picker', '--focus']:
        result = {}
    else:
        print('unsupported fixture command: ' + repr(args), file=sys.stderr)
        return 2
    print(json.dumps({'result': result}))
    return 0


def run(feature, binary, evidence):
    evidence.mkdir(parents=True, exist_ok=False)
    scratch = Path(tempfile.mkdtemp(prefix='picker-verify-', dir='/private/tmp' if sys.platform == 'darwin' else '/tmp'))
    socket = scratch / 'tmux.sock'
    script = Path(__file__).resolve()
    wrapper = scratch / 'herdr-fixture'
    wrapper.write_text('#!/bin/sh\nexec ' + shlex.join([sys.executable, str(script), 'fixture']) + ' "$@"\n')
    wrapper.chmod(0o755)
    agents = [{'agent': 'codex', 'agent_status': 'idle', 'name': name,
               'pane_id': f'verify-{name.lower()}', 'terminal_id': f'term-{name.lower()}',
               'workspace_id': 'verify-w', 'tab_id': 'verify-t', 'cwd': '/verify/' + name.lower()}
              for name in ('Alpha', 'Beta')]
    state = {'agents': agents}
    (scratch / 'fixture.json').write_text(json.dumps(state))
    (scratch / 'config.toml').write_text('[ui]\nagent_panel_sort = "spaces"\n')
    env = os.environ.copy()
    env.update(HERDR_BIN_PATH=str(wrapper), HERDR_CONFIG_PATH=str(scratch / 'config.toml'),
               HERDR_PLUGIN_STATE_DIR=str(scratch / 'state'), HERDR_PANE_ID='verify-picker',
               HERDR_PLUGIN_ID='yxhta.agents-picker', PICKER_VERIFY_SCRATCH=str(scratch), TERM='xterm-256color')
    env.pop('TMUX', None)
    journal = evidence / 'actions.jsonl'
    started = False

    def record(action, **fields):
        with journal.open('a') as out:
            out.write(json.dumps({'time': time.time(), 'feature': feature, 'action': action, **fields}) + '\n')

    def tmux(*args, check=True):
        result = subprocess.run(['tmux', '-f', '/dev/null', '-S', str(socket), *args], env=env,
                                capture_output=True, text=True, timeout=8)
        if check and result.returncode:
            raise RuntimeError(f'tmux {args!r}: {result.stderr}')
        return result

    def screen():
        return tmux('capture-pane', '-p', '-t', 'verify:0.0').stdout

    def snapshot(name):
        (evidence / (name + '.txt')).write_text(screen())
        (evidence / (name + '.ansi')).write_text(tmux('capture-pane', '-p', '-e', '-t', 'verify:0.0').stdout)
        record('capture', name=name)

    def wait_for(text):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if text in screen():
                return
            time.sleep(.1)
        snapshot('failure')
        raise AssertionError(f'screen did not contain {text!r}')

    def keys(*names):
        record('keys', keys=names)
        tmux('send-keys', '-t', 'verify:0.0', *names)

    def text(value):
        record('text', value=value)
        tmux('send-keys', '-l', '-t', 'verify:0.0', value)

    def wait_exit():
        deadline = time.monotonic() + 8
        while not (scratch / 'exit.json').exists() and time.monotonic() < deadline:
            time.sleep(.1)
        result = json.loads((scratch / 'exit.json').read_text())
        assert result['exit_code'] == 0, result
        (evidence / 'exit.json').write_text(json.dumps(result))

    try:
        (evidence / 'doctor.json').write_text(json.dumps(doctor(binary), indent=2))
        record('launch', binary=str(binary), socket=str(socket), scratch=str(scratch))
        tmux('new-session', '-d', '-s', 'verify', '-x', '160', '-y', '32', '/bin/sleep 60')
        started = True
        tmux('set-option', '-t', 'verify', 'remain-on-exit', 'on')
        command = shlex.join([sys.executable, str(script), 'child', str(binary)])
        tmux('respawn-pane', '-k', '-t', 'verify:0.0', command)
        wait_for('2/2')
        wait_for('LIVE verify-alpha')
        snapshot('ready')
        if feature == 'search':
            keys('/')
            text('Beta')
            wait_for('1/2')
            wait_for('LIVE verify-beta')
            snapshot('match')
            keys('C-u')
            text('zznomatchzz')
            wait_for('No matches for "zznomatchzz"')
            snapshot('no-match')
            keys('Escape')
            wait_for('/ to filter')
            wait_for('2/2')
            snapshot('cancel-search')
            keys('q')
        elif feature == 'navigation':
            keys('j')
            wait_for('LIVE verify-beta')
            snapshot('selected-beta')
            keys('Enter')
        elif feature == 'preview':
            wait_for('frame 2')
            snapshot('live-update')
            state['fallback'] = True
            (scratch / 'fixture.json').write_text(json.dumps(state))
            keys('j')
            wait_for('SNAPSHOT verify-beta')
            wait_for('live preview unavailable')
            snapshot('fallback')
            keys('q')
        elif feature == 'refresh':
            state['agents'] = agents[:1]
            (scratch / 'fixture.json').write_text(json.dumps(state))
            keys('r')
            wait_for('1/1')
            snapshot('manual-reload')
            state['agents'] = agents
            (scratch / 'fixture.json').write_text(json.dumps(state))
            wait_for('2/2')
            snapshot('automatic-reload')
            keys('q')
        elif feature == 'launch':
            keys('q')
        wait_exit()
        if feature == 'launch':
            result = subprocess.run([str(binary), '--open'], env=env, capture_output=True, text=True, timeout=8)
            record('command', argv=[str(binary), '--open'], exit_code=result.returncode, stdout=result.stdout, stderr=result.stderr)
            assert result.returncode == 0
        calls = [json.loads(line)['args'] for line in (scratch / 'calls.jsonl').read_text().splitlines()]
        focuses = [call for call in calls if len(call) > 1 and call[1] == 'focus']
        if feature == 'navigation':
            assert focuses == [['workspace', 'focus', 'verify-w'], ['tab', 'focus', 'verify-t'], ['agent', 'focus', 'verify-beta']], focuses
        else:
            assert not focuses, focuses
        if feature == 'launch':
            assert ['plugin', 'pane', 'open', '--plugin', 'yxhta.agents-picker', '--entrypoint', 'picker', '--focus'] in calls
        (evidence / 'result.json').write_text(json.dumps({'feature': feature, 'passed': True, 'boundary': 'fixture Herdr; real release TUI'}))
    finally:
        if started:
            tmux('send-keys', '-t', 'verify:0.0', 'C-c', check=False)
            # Give the real picker time to reap its preview observer before server teardown.
            deadline = time.monotonic() + 3
            while not (scratch / 'exit.json').exists() and time.monotonic() < deadline:
                time.sleep(.1)
            tmux('kill-server', check=False)
        for name in ('calls.jsonl', 'fixture.json', 'config.toml', 'exit.json'):
            if (scratch / name).exists():
                shutil.copy2(scratch / name, evidence / name)
        server_remaining = False
        if started:
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                server_remaining = tmux('list-sessions', check=False).returncode == 0
                if not server_remaining:
                    break
                time.sleep(.1)
        record('cleanup', scratch=str(scratch), server_remaining=server_remaining)
        if server_remaining:
            raise RuntimeError(f'verification server still running at {socket}; scratch retained')
        # The fixture observer is bounded to eight seconds even if a failed TUI cannot reap it.
        shutil.rmtree(scratch)
    print(evidence)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('command', choices=['doctor', 'run'])
    parser.add_argument('feature', nargs='?', choices=['search', 'navigation', 'preview', 'refresh', 'launch'])
    parser.add_argument('--binary', type=Path, default=BINARY)
    parser.add_argument('--evidence', type=Path)
    args = parser.parse_args()
    if args.command == 'doctor':
        print(json.dumps(doctor(args.binary.resolve()), indent=2))
    else:
        if not args.feature or not args.evidence:
            parser.error('run requires a feature and a new --evidence directory')
        run(args.feature, args.binary.resolve(), args.evidence.resolve())


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == 'fixture':
        sys.exit(fixture(sys.argv[2:]))
    if len(sys.argv) > 1 and sys.argv[1] == 'child':
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        code = subprocess.run([sys.argv[2]]).returncode
        (Path(os.environ['PICKER_VERIFY_SCRATCH']) / 'exit.json').write_text(json.dumps({'exit_code': code}))
        sys.exit(code)
    main()
