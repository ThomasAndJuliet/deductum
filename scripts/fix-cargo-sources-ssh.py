#!/usr/bin/env python3
"""Make flatpak-cargo-generator's output usable for git dependencies fetched
over ssh (the household's private crates, such as the libcosmic fork).

The generator copies Cargo.lock's source ids, `git+ssh://…`, into two places
where they are not urls: the git source flatpak-builder clones, and the cargo
source replacement, which must name the dependency's own url for cargo to
find the vendored copy. Run it after regenerating:

    flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
    python3 scripts/fix-cargo-sources-ssh.py packaging/flatpak/cargo-sources.json
"""
import json
import re
import sys

path = sys.argv[1]
with open(path) as f:
    sources = json.load(f)

def url(source_id):
    # git+ssh://git@github.com/OWNER/REPO -> ssh://git@github.com/OWNER/REPO.git
    u = source_id.removeprefix('git+')
    return u if u.endswith('.git') else u + '.git'

for source in sources:
    if source.get('type') == 'git' and source.get('url', '').startswith('git+ssh://'):
        source['url'] = url(source['url'])
    contents = source.get('contents')
    if isinstance(contents, str) and 'vendored-sources' in contents:
        source['contents'] = re.sub(
            r'git = "(git\+ssh://[^"]+)"',
            lambda m: f'git = "{url(m.group(1))}"',
            contents,
        )

with open(path, 'w') as f:
    json.dump(sources, f, indent=4)
    f.write('\n')
