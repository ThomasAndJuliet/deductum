#!/bin/sh
# ct-sudo-setup: set a machine up to accept sudo approvals from COSMIC
# Terminal, or undo that.
#
# Installed by COSMIC Terminal at ~/.config/cosmic-term-agents/ct-sudo-setup.
#
# Usage: sudo ct-sudo-setup install MODULE KEYFILE
#            install the PAM module MODULE and the public key in KEYFILE
#            for the user who ran sudo, and list the module in sudo's PAM
#            stack
#        sudo ct-sudo-setup uninstall
#            take all of that out again
#        ct-sudo-setup status
#            what is installed, without needing root
#
# The module is listed first in /etc/pam.d/sudo and sudo-i, as
#     auth sufficient /usr/local/lib/cosmic-term/pam_cosmic_term_sudo.so
# between marker comments, so a good signature from the user's terminal lets
# sudo through and anything else leaves sudo to ask for the password as
# before: a broken module cannot lock sudo. The user's public key goes in
# /etc/cosmic-term/sudo-keys/USER.pub, owned by root, as the module requires.
#
# A POSIX sh script, so that it runs unchanged on every machine.

set -eu

module_name=pam_cosmic_term_sudo.so
module_dir=/usr/local/lib/cosmic-term
module_path=$module_dir/$module_name
keys=/etc/cosmic-term/sudo-keys
begin='# cosmic-term sudo: begin (approvals from COSMIC Terminal; see ct-sudo-setup)'
end='# cosmic-term sudo: end'

die() {
    echo "ct-sudo-setup: $*" >&2
    exit 1
}

# The directory PAM loads modules from, wherever this distribution keeps it
pam_dir() {
    for d in /usr/lib64/security /lib64/security /usr/lib/x86_64-linux-gnu/security \
        /lib/x86_64-linux-gnu/security /usr/lib/aarch64-linux-gnu/security \
        /usr/lib/security /lib/security; do
        if [ -e "$d/pam_unix.so" ]; then
            echo "$d"
            return
        fi
    done
    return 1
}

pam_files() {
    for f in /etc/pam.d/sudo /etc/pam.d/sudo-i; do
        [ -f "$f" ] && echo "$f"
    done
}

the_user() {
    user=${SUDO_USER:-}
    [ -n "$user" ] && [ "$user" != root ] || die "run this with sudo, as the user to set up"
    case "$user" in */* | .*) die "odd user name: $user" ;; esac
    echo "$user"
}

need_root() {
    [ "$(id -u)" = 0 ] || die "run this with sudo"
}

# Insert the module's lines before the first auth line of a PAM file
list_module() {
    f=$1
    [ -e "$f.cosmic-term-backup" ] || cp -p "$f" "$f.cosmic-term-backup"
    tmp="$f.cosmic-term-new"
    awk -v begin="$begin" -v end="$end" -v module="$module_path" '
        $0 == begin {
            if (!done) {
                print begin
                print "auth       sufficient   " module
                print end
                done = 1
            }
            skip = 1
            next
        }
        skip && $0 == end { skip = 0; next }
        skip { next }
        !done && ($1 == "auth" || $1 == "-auth" || $0 ~ /^@include[ \t]+common-auth/) {
            print begin
            print "auth       sufficient   " module
            print end
            done = 1
        }
        { print }
        END { if (skip || !done) exit 1 }
    ' "$f" >"$tmp" || { rm -f "$tmp"; die "$f has no auth line or an unfinished cosmic-term block"; }
    chmod --reference="$f" "$tmp" 2>/dev/null || chmod 644 "$tmp"
    chown root:root "$tmp"
    mv "$tmp" "$f"
    command -v restorecon >/dev/null && restorecon "$f" 2>/dev/null || true
}

unlist_module() {
    f=$1
    grep -qF "$begin" "$f" || return 0
    tmp="$f.cosmic-term-new"
    awk -v begin="$begin" -v end="$end" '
        $0 == begin { skip = 1; next }
        skip && $0 == end { skip = 0; next }
        !skip { print }
        END { if (skip) exit 1 }
    ' "$f" >"$tmp" || { rm -f "$tmp"; die "$f has an unfinished cosmic-term block"; }
    chmod --reference="$f" "$tmp" 2>/dev/null || chmod 644 "$tmp"
    chown root:root "$tmp"
    mv "$tmp" "$f"
    command -v restorecon >/dev/null && restorecon "$f" 2>/dev/null || true
}

install_all() {
    need_root
    module=${1:-}
    keyfile=${2:-}
    [ -f "$module" ] || die "no module at $module"
    [ -f "$keyfile" ] || die "no key file at $keyfile"
    user=$(the_user)
    key=$(grep -v '^#' "$keyfile" | grep -m1 '^ssh-ed25519 ') || die "$keyfile holds no ssh-ed25519 key"
    # Atomic desktops keep /usr read-only, but /usr/local is writable
    # (on Fedora it points into /var/usrlocal). Its libraries already have
    # SELinux's lib_t label; an absolute PAM path avoids package layering.
    install -d -o root -g root -m 0755 "$module_dir"
    install -o root -g root -m 0644 "$module" "$module_path.new"
    mv "$module_path.new" "$module_path"
    command -v restorecon >/dev/null && restorecon -R "$module_dir" 2>/dev/null || true

    install -d -o root -g root -m 0755 /etc/cosmic-term "$keys"
    printf '# The key COSMIC Terminal signs sudo approvals for %s with\n%s\n' "$user" "$key" \
        >"$keys/$user.pub.new"
    chown root:root "$keys/$user.pub.new"
    chmod 0644 "$keys/$user.pub.new"
    mv "$keys/$user.pub.new" "$keys/$user.pub"
    command -v restorecon >/dev/null && restorecon -R /etc/cosmic-term 2>/dev/null || true

    for f in $(pam_files); do
        list_module "$f"
    done
    echo "Set up: sudo on $(cat /proc/sys/kernel/hostname) accepts approvals from $user's terminal."
}

uninstall_all() {
    need_root
    for f in $(pam_files); do
        unlist_module "$f"
    done
    rm -f "$module_path"
    rmdir "$module_dir" 2>/dev/null || true
    # Earlier versions used the distribution's module directory. A
    # read-only legacy file must not prevent disabling its PAM rules.
    if dir=$(pam_dir) && [ -f "$dir/$module_name" ]; then
        rm -f "$dir/$module_name" 2>/dev/null || true
    fi
    user=${SUDO_USER:-}
    [ -n "$user" ] && rm -f "$keys/$user.pub"
    rmdir "$keys" /etc/cosmic-term 2>/dev/null || true
    echo "Removed: sudo asks for the password as before."
}

status() {
    user=${SUDO_USER:-$(id -un)}
    if [ -f "$module_path" ]; then
        echo "module: $module_path"
    elif dir=$(pam_dir) && [ -f "$dir/$module_name" ]; then
        echo "module: $dir/$module_name"
    else
        echo "module: none"
    fi
    [ -f "$keys/$user.pub" ] && echo "key: $keys/$user.pub" || echo "key: none"
    for f in $(pam_files); do
        if grep -qF "$begin" "$f" 2>/dev/null; then
            echo "listed: $f"
        else
            echo "not listed: $f"
        fi
    done
}

case "${1:-}" in
    install) install_all "${2:-}" "${3:-}" ;;
    uninstall) uninstall_all ;;
    status) status ;;
    *) sed -n '/^# Usage:/,/^# *$/p' "$0" | sed 's/^# \{0,1\}//' ;;
esac
