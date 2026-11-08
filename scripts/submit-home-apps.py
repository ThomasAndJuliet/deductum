#!/usr/bin/env python3
"""Submit this repository's tested builds to the private home-apps ingress on Neo.

A public repository cannot call the household's private reusable workflow, so this
follows its submission client. Signing stays on Neo, which independently checks
GitHub's signed identity, the CI run, the artifact and its digest. Artifacts are
removed only after Neo durably accepts them.
"""
import json
import os
import re
import socket
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

import release_config

OWNER_ID = 341158028
APP_ID = 'dev.twright.Deductum'
ARTIFACT = 'home-apps-x86_64'
AUDIENCE = 'neo-home-apps'
SHA = re.compile(r'^[a-f0-9]{40}$')
DIGEST = re.compile(r'^sha256:[a-f0-9]{64}$')
DURABLE = {'ready', 'archive', 'promote', 'stable', 'published'}


class Failure(Exception):
    def __init__(self, message, http_status=None, reason=None):
        super().__init__(message)
        self.http_status = http_status
        self.reason = reason


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise Failure('A credentialed endpoint attempted a redirect')


def refusal(error):
    """Return Neo's stated refusal reason from an HTTP error body, if it gave one."""
    try:
        reason = json.loads(error.read(4096)).get('refusal')
    except Exception:
        return None
    if isinstance(reason, str) and 0 < len(reason) <= 200 and reason.isprintable():
        return reason
    return None


def request(url, headers, payload=None, method=None):
    encoded = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(url, data=encoded, headers=headers, method=method)
    try:
        with urllib.request.build_opener(NoRedirect).open(req, timeout=120) as response:
            content = response.read(1024 * 1024 + 1)
            if len(content) > 1024 * 1024:
                raise Failure('Endpoint response exceeded its limit')
            return json.loads(content) if content else None
    except urllib.error.HTTPError as error:
        message = 'Endpoint rejected request (HTTP %s)' % error.code
        reason = refusal(error)
        if reason:
            message += ': ' + reason
        raise Failure(message, http_status=error.code, reason=reason) from None
    except (urllib.error.URLError, TimeoutError):
        raise Failure('Endpoint could not be reached') from None


class GitHub:
    def __init__(self, repository, token):
        if not re.fullmatch(r'ThomasAndJuliet/[a-z0-9-]+', repository):
            raise Failure('Unexpected source organization or repository')
        self.repository = repository
        self.prefix = '/repos/' + repository
        self.headers = {'Authorization': 'Bearer ' + token,
                        'Accept': 'application/vnd.github+json',
                        'X-GitHub-Api-Version': '2022-11-28',
                        'User-Agent': 'neo-home-apps-ci'}

    def call(self, suffix, method=None):
        if not suffix.startswith('/'):
            raise Failure('Invalid API resource')
        return request('https://api.github.com' + self.prefix + suffix, self.headers, method=method)

    def collection(self, suffix, field):
        for page in range(1, 1001):
            join = '&' if '?' in suffix else '?'
            rows = self.call(suffix + join + 'per_page=100&page=' + str(page))[field]
            yield from rows
            if len(rows) < 100:
                return
        raise Failure('API pagination exceeded its limit')


def eligible(run, repository, repository_id, branch):
    return (run.get('event') == 'push' and run.get('head_branch') == branch
            and run.get('status') == 'completed' and run.get('conclusion') == 'success'
            and run.get('path') == '.github/workflows/ci.yml'
            and run.get('repository', {}).get('id') == repository_id
            and run.get('head_repository', {}).get('id') == repository_id
            and SHA.fullmatch(run.get('head_sha', '')) is not None
            and type(run.get('id')) is int and run['id'] > 0
            and type(run.get('run_number')) is int and run['run_number'] > 0)


# Fixed here so credentials can never go to a caller-selected host.
PUBLISH_URL = 'http://neo:8101/v1/releases'
# What the tailnet identity must not reach beyond that port.
FORBIDDEN = (('neo', 22), ('neo', 8099))


def require_narrow_network():
    exposed = []
    for host, port in FORBIDDEN:
        try:
            with socket.create_connection((host, port), timeout=5):
                exposed.append(f'{host}:{port}')
        except OSError:
            pass
    if exposed:
        raise Failure('CI identity has excess network access: ' + ', '.join(exposed))


def identity():
    url = os.environ['ACTIONS_ID_TOKEN_REQUEST_URL']
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != 'https' or not parsed.hostname
            or not parsed.hostname.endswith('.actions.githubusercontent.com')
            or parsed.username or parsed.password):
        raise Failure('Unexpected GitHub identity endpoint')
    separator = '&' if '?' in url else '?'
    response = request(url + separator + 'audience=' + urllib.parse.quote(AUDIENCE, safe=''),
                       {'Authorization': 'Bearer ' + os.environ['ACTIONS_ID_TOKEN_REQUEST_TOKEN']})
    token = response['value']
    # GitHub consumes this control record and suppresses the token in its logs.
    print('::add-mask::' + token, flush=True)
    return token


def verify_receipt(row, run, repository):
    if (row.get('repository') != repository or type(row.get('run_id')) is not int
            or row.get('run_id') != run['id']
            or row.get('sha') != run['head_sha'] or row.get('phase') not in DURABLE
            or not DIGEST.fullmatch(row.get('digest', ''))
            or type(row.get('artifact_id')) is not int or row['artifact_id'] <= 0):
        raise Failure('Neo receipt does not prove durable acceptance of this exact CI build')
    return row


def signed(row, run, app_id):
    if row.get('phase') != 'published':
        return False
    archive = row.get('archive_receipt', {})
    stable = row.get('stable_receipt', {})
    for result in (archive, stable):
        if (result.get('app') != app_id or result.get('ci', {}).get('sha') != run['head_sha']
                or result.get('ci', {}).get('run_id') != run['id']):
            raise Failure('Signing receipt does not match this application and source')
    if (archive.get('status') != 'published'
            or not re.fullmatch(r'[a-f0-9]{64}', archive.get('commit', ''))
            or stable.get('status') not in ('published', 'superseded')
            or (stable.get('status') == 'published'
                and not re.fullmatch(r'[a-f0-9]{64}', stable.get('commit', '')))):
        raise Failure('Signed archival/stable receipts are incomplete')
    return True


def submit(api, run, url, app_id, artifact_name, token_supplier=identity, sleeper=time.sleep):
    repository = api.repository
    def send():
        headers = {'Authorization': 'Bearer ' + token_supplier(),
                   'X-GitHub-Actions-Token': os.environ['GITHUB_TOKEN'],
                   'Content-Type': 'application/json'}
        return verify_receipt(request(url, headers, {'run_id': run['id']}), run, repository)
    artifacts = list(api.collection('/actions/runs/' + str(run['id']) + '/artifacts', 'artifacts'))
    expected = [row for row in artifacts if row.get('name') == artifact_name]
    try:
        receipt = send()
    except Failure as error:
        if error.http_status == 422 and expected and all(row.get('expired') is True for row in expected):
            raise Failure('Run %s has an expired artifact and Neo did not accept it; check its retained receipt or rerun CI for its original source SHA' % run['id']) from None
        if error.reason and 'staging reserve' in error.reason:
            raise Failure('Run %s was refused because Neo is below its disk staging reserve; free space on Neo, then run this workflow again (the scheduled recovery also retries it)' % run['id'], http_status=422, reason=error.reason) from None
        raise
    # The server downloaded and fsynced the immutable artifact and committed its ledger.
    matching = [row for row in artifacts if row['id'] == receipt['artifact_id'] and row['name'] == artifact_name]
    if matching:
        try:
            api.call('/actions/artifacts/' + str(receipt['artifact_id']), method='DELETE')
        except Failure as error:
            if error.http_status != 404:
                raise  # Already-removed artifacts are safe only after durable acceptance.
    print('Run %s / %s: durably accepted on Neo' % (run['id'], run['head_sha']), flush=True)
    for attempt in range(90):
        if signed(receipt, run, app_id):
            print('Run %s: signed archive retained; stable status %s' %
                  (run['id'], receipt['stable_receipt']['status']), flush=True)
            return receipt
        sleeper(10)
        receipt = send()
    raise Failure('Build is retained on Neo; signing is still pending and scheduled recovery will retry')


def release_runs(api, repo, selected, minimum, artifact_name):
    if selected:
        if not selected.isdecimal() or int(selected) <= 0:
            raise Failure('Invalid CI run identity')
        runs = [api.call('/actions/runs/' + selected)]
    else:
        # Durable acceptance removes the GitHub artifact. Neo retries its own
        # retained queue without a repository token; recovery needs only the
        # artifact records that have not crossed that durable boundary yet.
        # Include expired records: a held copy may already exist on Neo; if
        # not, submission must fail visibly instead of silently discarding it.
        candidates = set()
        path = '/actions/artifacts?name=' + urllib.parse.quote(artifact_name, safe='')
        for artifact in api.collection(path, 'artifacts'):
            if artifact.get('name') == artifact_name:
                run_id = artifact.get('workflow_run', {}).get('id')
                if type(run_id) is not int or run_id <= 0:
                    raise Failure('Artifact has no valid originating CI run')
                candidates.add(run_id)
        runs = [api.call('/actions/runs/' + str(run_id)) for run_id in sorted(candidates)]
    runs = sorted((run for run in runs if eligible(run, api.repository, repo['id'], repo['default_branch'])
                   and run['run_number'] >= minimum),
                  key=lambda run: (run['run_number'], run['id']))
    if selected and not runs:
        raise Failure('Selected CI did not pass for the canonical adopted release branch')
    return runs


def main():
    repository = os.environ['GITHUB_REPOSITORY']
    if repository != release_config.repository():
        raise Failure('Only the repository named in release-config.json may publish')
    api = GitHub(repository, os.environ['GITHUB_TOKEN'])
    repo = request('https://api.github.com/repos/' + repository, api.headers)
    if repo['owner']['id'] != OWNER_ID or repo['archived']:
        raise Failure('Only active household repositories may publish')
    minimum = os.environ.get('NEO_CI_MINIMUM_RUN_NUMBER', '1')
    if not minimum.isdecimal() or int(minimum) <= 0:
        raise Failure('Invalid publication adoption cutoff')
    selected = os.environ.get('NEO_CI_RUN_ID', '')
    runs = release_runs(api, repo, selected, int(minimum), ARTIFACT)
    mode = sys.argv[1] if len(sys.argv) == 2 else ''
    if mode == 'discover':
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            output.write('has_work=' + ('true' if runs else 'false') + '\n')
        print('Found %s eligible release builds awaiting submission' % len(runs), flush=True)
    elif mode == 'submit':
        require_narrow_network()
        for run in runs:
            submit(api, run, PUBLISH_URL, APP_ID, ARTIFACT)
        print('Processed %s eligible successful release builds' % len(runs), flush=True)
    else:
        raise Failure('Usage: submit-home-apps.py discover|submit')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # Never print HTTP error bodies (beyond Neo's checked refusal reason),
        # signed URLs, exception reprs or credentials.
        print('::error::' + (str(error) if isinstance(error, Failure) else
                           'Private release submission failed; inspect the trusted configuration'), file=sys.stderr)
        sys.exit(1)
