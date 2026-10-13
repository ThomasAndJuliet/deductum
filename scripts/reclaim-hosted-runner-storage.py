#!/usr/bin/env python3
"""Reclaim unused toolchains only on an ephemeral GitHub-hosted Linux runner."""
import http.client
import json
import os
import re
import socket
import struct
import subprocess
import uuid

HOST_TOOLCHAINS = {
    '/usr/local/lib/android': '/ci-reclaim/android',
    '/usr/share/dotnet': '/ci-reclaim/dotnet',
    '/usr/share/swift': '/ci-reclaim/swift',
}
HELPER = """import os, pathlib, shutil
status = dict(line.split(':', 1) for line in pathlib.Path('/proc/self/status').read_text().splitlines() if ':' in line)
if os.getuid() != 0 or int(status['CapEff'].strip(), 16) != 0 or status['NoNewPrivs'].strip() != '1':
    raise RuntimeError('Cleanup requires UID 0, zero capabilities, and no-new-privileges')
for directory in ['/ci-reclaim/android', '/ci-reclaim/dotnet', '/ci-reclaim/swift']:
    root = pathlib.Path(directory)
    if not root.is_dir() or root.is_symlink():
        raise RuntimeError('Cleanup root is not a mounted directory')
    for entry in root.iterdir():
        if entry.is_symlink() or not entry.is_dir():
            entry.unlink()
        else:
            shutil.rmtree(entry)
    print('Reclaimed unused toolchain: ' + directory, flush=True)
"""


class UnixConnection(http.client.HTTPConnection):
    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect('/var/run/docker.sock')


class DockerAPI:
    def raw(self, method, path, payload=None):
        connection = UnixConnection('localhost', timeout=300)
        try:
            body = json.dumps(payload).encode() if payload is not None else None
            connection.request(method, path, body, {'Content-Type': 'application/json'})
            response = connection.getresponse()
            data = response.read()
            if not 200 <= response.status < 300:
                raise RuntimeError(f'Docker API request failed with HTTP {response.status}')
            return data
        finally:
            connection.close()

    def request(self, method, path, payload=None):
        data = self.raw(method, path, payload)
        return json.loads(data) if data else None

    def logs(self, identity):
        wire = self.raw('GET', '/containers/' + identity + '/logs?stdout=1&stderr=1')
        messages = []
        while wire:
            if len(wire) < 8 or wire[0] not in (1, 2) or wire[1:4] != b'\0\0\0':
                raise RuntimeError('Invalid Docker log framing')
            size = struct.unpack('>I', wire[4:8])[0]
            if size > 1024 * 1024 or len(wire) < size + 8:
                raise RuntimeError('Invalid Docker log length')
            messages.append(wire[8:8 + size].decode(errors='replace'))
            wire = wire[8 + size:]
        return ''.join(messages)


def require_hosted(environment, hostname):
    if (environment.get('GITHUB_ACTIONS') != 'true'
            or environment.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or environment.get('RUNNER_OS') != 'Linux'):
        raise RuntimeError('Storage reclamation requires a GitHub-hosted Linux Actions job')
    if not re.fullmatch(r'[a-f0-9]{12,64}', hostname):
        raise RuntimeError('Storage reclamation requires the current Docker job container')


def storage_report(phase):
    print('Runner storage ' + phase, flush=True)
    paths = ['/', os.environ.get('GITHUB_WORKSPACE', os.getcwd())]
    subprocess.run(['df', '-B1', '-P', *paths], check=True)
    subprocess.run(['df', '-i', '-P', *paths], check=True)


def reclaim(api=None, environment=None, hostname=None, report=storage_report):
    environment = os.environ if environment is None else environment
    hostname = socket.gethostname() if hostname is None else hostname
    require_hosted(environment, hostname)
    api = DockerAPI() if api is None else api
    current = api.request('GET', '/containers/' + hostname + '/json')
    image = current.get('Image', '')
    if (not current.get('Id', '').startswith(hostname)
            or not re.fullmatch(r'sha256:[a-f0-9]{64}', image)):
        raise RuntimeError('Current Docker job image identity did not match')
    # Reuse the pulled immutable image ID; never pull another cleanup image.
    specification = {
        'Image': image, 'User': '0:0', 'Entrypoint': ['python3', '-c'],
        'Cmd': [HELPER], 'Env': ['PYTHONDONTWRITEBYTECODE=1'],
        'HostConfig': {
            'Binds': [source + ':' + target + ':rw' for source, target in HOST_TOOLCHAINS.items()],
            'ReadonlyRootfs': True, 'NetworkMode': 'none', 'Privileged': False,
            'CapDrop': ['ALL'], 'SecurityOpt': ['no-new-privileges'],
        },
    }
    report('before cleanup')
    created = api.request('POST', '/containers/create?name=household-ci-reclaim-' + uuid.uuid4().hex, specification)
    identity = created.get('Id', '')
    if not re.fullmatch(r'[a-f0-9]{12,64}', identity):
        raise RuntimeError('Invalid cleanup container identity')
    try:
        api.request('POST', '/containers/' + identity + '/start')
        result = api.request('POST', '/containers/' + identity + '/wait?condition=not-running')
        print(api.logs(identity), end='', flush=True)
        if result.get('StatusCode') != 0 or result.get('Error'):
            raise RuntimeError('Unused runner toolchain cleanup failed')
    finally:
        api.request('DELETE', '/containers/' + identity + '?force=1')
        report('after cleanup')


if __name__ == '__main__':
    reclaim()
