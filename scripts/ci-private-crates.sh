#!/bin/sh
# Let CI fetch the household's private crates over ssh.
#
#   scripts/ci-private-crates.sh household-mount [household]
#
# Each argument names a private repository (ThomasAndJuliet/NAME) the job
# needs; its read-only deploy key must be in the environment:
#
#   household-mount  HOUSEHOLD_MOUNT_DEPLOY_KEY
#   household        HOUSEHOLD_DEPLOY_KEY
#   household-kit    HOUSEHOLD_KIT_DEPLOY_KEY
#   libcosmic        LIBCOSMIC_DEPLOY_KEY
#
# For each repository the script
#   - writes the key to a private file (mode 600),
#   - adds an ssh host alias that uses only that key, with GitHub's ed25519
#     host key pinned (checked against https://api.github.com/meta),
#   - maps exactly that repository's ssh URL, as written in Cargo.toml and
#     packaging/flatpak/cargo-sources.json, to the alias with a global git
#     `url.<alias>.insteadOf` rule, and
#   - points git at that ssh configuration (core.sshCommand), so the system
#     ssh config and agent are left alone.
# It also sets CARGO_NET_GIT_FETCH_WITH_CLI=true in $GITHUB_ENV, so that cargo
# fetches with git, and so with these rules; flatpak-builder clones with git.
#
# Files go under $RUNNER_TEMP (else $HOME/.ssh), which works the same on the
# neo runners and in the hosted job's Flatpak container (root, HOME=/github/home).
# Key material is never printed. A missing key fails with a message: forks
# and other untrusted actors have no secrets, so their jobs stop here, clearly,
# rather than in an authentication error from git.
#
# Run it before the first step that fetches crates, passing only the secrets
# that job needs.
set -eu

HOST_KEY='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl'
OWNER=ThomasAndJuliet

die() {
    echo "ci-private-crates: $*" >&2
    exit 1
}

[ $# -gt 0 ] || die "usage: $0 REPO..."
if ! command -v ssh >/dev/null 2>&1; then
    # A minimal job container may lack the ssh client; root there can add it.
    if [ "$(id -u)" = 0 ]; then
        if command -v dnf >/dev/null 2>&1; then dnf -y install openssh-clients >&2
        elif command -v apt-get >/dev/null 2>&1; then apt-get update >&2 && apt-get install -y --no-install-recommends openssh-client >&2
        elif command -v apk >/dev/null 2>&1; then apk add --no-cache openssh-client >&2
        fi
    fi
    command -v ssh >/dev/null 2>&1 || die "the ssh client is not installed on this runner"
fi
command -v git >/dev/null 2>&1 || die "git is not installed on this runner"

dir=${CI_PRIVATE_CRATES_DIR:-${RUNNER_TEMP:-${HOME:?HOME is not set}/.ssh}/household-private-crates}
umask 077
mkdir -p "$dir"
chmod 700 "$dir"
printf 'github.com %s\n' "$HOST_KEY" >"$dir/known_hosts"
: >"$dir/ssh_config"

for repo in "$@"; do
    case $repo in
    household-mount) var=HOUSEHOLD_MOUNT_DEPLOY_KEY ;;
    household) var=HOUSEHOLD_DEPLOY_KEY ;;
    household-kit) var=HOUSEHOLD_KIT_DEPLOY_KEY ;;
    libcosmic) var=LIBCOSMIC_DEPLOY_KEY ;;
    *) die "unknown private repository '$repo' (known: household-mount, household, household-kit, libcosmic)" ;;
    esac
    # The value of the variable named in $var, without ever printing it.
    key=$(printenv "$var" || true)
    if [ -z "$key" ]; then
        die "$var is empty, so ${OWNER}/$repo cannot be fetched. Its read-only deploy key must be a secret of this repository; workflows run for forks and other untrusted actors receive no secrets."
    fi
    alias=github-ci-$repo
    keyfile=$dir/$repo.key
    printf '%s\n' "$key" >"$keyfile"
    chmod 600 "$keyfile"
    unset key
    if command -v ssh-keygen >/dev/null 2>&1 && ! ssh-keygen -y -f "$keyfile" >/dev/null 2>&1; then
        rm -f "$keyfile"
        die "$var is not a valid ssh private key"
    fi
    cat >>"$dir/ssh_config" <<CONF
Host $alias
    HostName github.com
    User git
    IdentityFile $keyfile
    IdentitiesOnly yes
    IdentityAgent none
    HostKeyAlias github.com
    HostKeyAlgorithms ssh-ed25519
    StrictHostKeyChecking yes
    UserKnownHostsFile $dir/known_hosts
    GlobalKnownHostsFile /dev/null
    BatchMode yes

CONF
    git config --global --replace-all \
        "url.ssh://git@$alias/$OWNER/$repo.git.insteadOf" \
        "ssh://git@github.com/$OWNER/$repo.git"
done

git config --global core.sshCommand "ssh -F $dir/ssh_config"

if [ -n "${GITHUB_ENV:-}" ]; then
    echo 'CARGO_NET_GIT_FETCH_WITH_CLI=true' >>"$GITHUB_ENV"
fi
echo "ci-private-crates: configured access to: $*"
