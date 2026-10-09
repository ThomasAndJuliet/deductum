#!/bin/sh
# ct-mount: the other machines' files, mounted with sshfs on this one.
#
# Installed by COSMIC Terminal on every machine it works with, at
# ~/.config/cosmic-term-agents/ct-mount, and rewritten when it changes.
#
# Usage: ct-mount NAME               mount machine NAME, printing where
#        ct-mount --list             the machines: name, ssh destination, mount
#        ct-mount --register NAME DEST   record how to reach machine NAME,
#                                    its host name, over ssh
#        ct-mount --lease NAME       the directory for leases on NAME's
#                                    mount (see below)
#        ct-mount --gc               unmount what nothing needs any more
#        ct-mount --where            "local" if the user is at this machine,
#                                    else "remote NAME" (see below)
#        ct-mount --export [NAME]    have machine NAME (by default the user's)
#                                    mount this one, printing the path there
#        ct-mount --open FILE        open FILE on the user's screen: here, or
#                                    on their machine through its mount of
#                                    this one
#
# Machine NAME is mounted at ~/.cache/cosmic-term/mounts/NAME, so its /x is
# at ~/.cache/cosmic-term/mounts/NAME/x. Mounts are made when first asked for
# and kept while anything here needs them: a running session of the terminal
# (a Zellij session named ct-…), whose agents may use any machine's files,
# or a lease. A lease is a file under the state directory's leases/NAME/ that
# a terminal window keeps locked (flock) while it shows that machine; a
# file nobody holds is stale and removed. --gc unmounts the rest, as well as
# mounts of machines that are no longer registered. The terminal runs it in
# the background when a session ends, when a window lets a lease go, and from
# time to time, so the reference count is worked out afresh each time rather
# than kept, and a crash cannot leave it wrong.
#
# Machines are named by their host names. Which one the user is at, for
# anything graphical, is the first of:
#   1. the machine of the terminal attached to this session ($ZELLIJ_SESSION_NAME)
#      that saw input most recently: each terminal's `zellij attach` carries
#      CT_CLIENT, set by the terminal;
#   2. for a plain ssh session, the known machine $SSH_CONNECTION comes from;
#   3. the one machine, among this one and the known ones, with a local
#      desktop in use (logind);
#   4. the machine whose terminal last opened this session, recorded then
#      under the state directory's clients/.
#
# A POSIX sh script, so that it runs unchanged on every machine.

set -u
# The Zellij the terminal settled on for this machine comes first
export PATH="$HOME/.config/cosmic-term-agents/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
dir="$HOME/.cache/cosmic-term/mounts"
state="${XDG_STATE_HOME:-$HOME/.local/state}/cosmic-term/mounts"
mkdir -p "$dir" "$state/leases"

die() {
    echo "ct-mount: $*" >&2
    exit 1
}

valid_name() {
    case "$1" in
        "" | .* | */* | *[!A-Za-z0-9._@-]*) return 1 ;;
    esac
}

unmount() {
    fusermount3 -u "$1" 2>/dev/null || fusermount -u "$1" 2>/dev/null ||
        fusermount3 -uz "$1" 2>/dev/null || fusermount -uz "$1" 2>/dev/null
}

# Mounted and answering; a mount whose connection died hangs or fails
alive() {
    mountpoint -q "$1" && timeout 5 stat "$1/" >/dev/null 2>&1
}

mount_machine() {
    name=$1
    valid_name "$name" || die "not a machine name: $name"
    dest=$(cat "$state/$name.dest" 2>/dev/null) ||
        die "$name is not a known machine; see ct-mount --list"
    command -v sshfs >/dev/null || die "sshfs is not installed"
    m="$dir/$name"
    mkdir -p "$m"
    # One request at a time mounts a machine; the others wait for it
    exec 9>"$state/$name.lock"
    flock 9
    if ! alive "$m"; then
        unmount "$m"
        # sshfs stays behind as the mount; it must not keep the lock
        sshfs "$dest:/" "$m" -o reconnect,ServerAliveInterval=15,ServerAliveCountMax=3 \
            -o follow_symlinks,BatchMode=yes,ConnectTimeout=10 9>&- ||
            die "could not mount $name ($dest)"
    fi
    echo "$m"
}

register() {
    valid_name "$1" || die "not a machine name: $1"
    [ -n "$2" ] || die "no ssh destination for $1"
    if [ "$(cat "$state/$1.dest" 2>/dev/null)" != "$2" ]; then
        printf '%s\n' "$2" >"$state/$1.dest.tmp" && mv "$state/$1.dest.tmp" "$state/$1.dest"
    fi
}

this_machine() {
    cat /proc/sys/kernel/hostname
}

# The machine of the terminal attached to this session that saw input last:
# each terminal's `zellij attach` here carries CT_CLIENT, its machine's name,
# and its terminal's access time moves as the user types, as `w` reads it
attached_client() {
    [ -n "${ZELLIJ_SESSION_NAME:-}" ] || return 0
    best=
    best_t=-1
    for p in /proc/[0-9]*; do
        cmd=$(tr '\0' ' ' 2>/dev/null <"$p/cmdline") || continue
        case "$cmd" in *zellij*" attach "*) ;; *) continue ;; esac
        case " $cmd " in *" $ZELLIJ_SESSION_NAME "*) ;; *) continue ;; esac
        c=$(tr '\0' '\n' 2>/dev/null <"$p/environ" | sed -n 's/^CT_CLIENT=//p')
        [ -n "$c" ] || continue
        t=$(stat -L -c %X "$p/fd/0" 2>/dev/null) || t=0
        if [ "$t" -gt "$best_t" ]; then
            best=$c
            best_t=$t
        fi
    done
    [ -n "$best" ] && echo "$best"
}

# For a plain ssh session: the known machine its connection comes from
ssh_client() {
    [ -z "${ZELLIJ_SESSION_NAME:-}" ] && [ -n "${SSH_CONNECTION:-}" ] || return 0
    ip=${SSH_CONNECTION%% *}
    for f in "$state"/*.dest; do
        [ -e "$f" ] || continue
        host=$(cat "$f")
        host=${host#*@}
        if getent ahosts "$host" 2>/dev/null | grep -q "^$ip "; then
            basename "$f" .dest
            return
        fi
    done
}

# Whether this machine has a local desktop in use
has_desktop() {
    script='for s in $(loginctl list-sessions --no-legend 2>/dev/null | awk "{print \$1}"); do
        loginctl show-session "$s" -p Type -p Remote -p State 2>/dev/null | tr "\n" " "; echo
    done | grep -E "Type=(wayland|x11)" | grep "Remote=no" | grep -q "State=active"'
    sh -c "$script"
}

# The machine a terminal last opened this session from, as recorded then
last_client() {
    clients="${XDG_STATE_HOME:-$HOME/.local/state}/cosmic-term/clients"
    if [ -n "${ZELLIJ_SESSION_NAME:-}" ] && [ -f "$clients/$ZELLIJ_SESSION_NAME" ]; then
        cat "$clients/$ZELLIJ_SESSION_NAME"
    elif [ -f "$clients/latest" ]; then
        cat "$clients/latest"
    fi
}

# The machine the user is at, or nothing if it cannot be told
client() {
    c=$(attached_client)
    [ -n "$c" ] || c=$(ssh_client)
    if [ -z "$c" ]; then
        # The one machine with a desktop in use, if only one has
        found=
        n=0
        if has_desktop; then
            found=$(this_machine)
            n=1
        fi
        for f in "$state"/*.dest; do
            [ -e "$f" ] || continue
            if has_desktop_on "$(cat "$f")"; then
                found=$(basename "$f" .dest)
                n=$((n + 1))
            fi
        done
        [ "$n" = 1 ] && c=$found
    fi
    [ -n "$c" ] || c=$(last_client)
    [ -n "$c" ] && echo "$c"
}

has_desktop_on() {
    script='for s in $(loginctl list-sessions --no-legend 2>/dev/null | awk "{print \$1}"); do
        loginctl show-session "$s" -p Type -p Remote -p State 2>/dev/null | tr "\n" " "; echo
    done | grep -E "Type=(wayland|x11)" | grep "Remote=no" | grep -q "State=active"'
    timeout 5 ssh -o BatchMode=yes -o ConnectTimeout=3 "$1" \
        "sh -c 'eval \"\$(echo $(printf '%s' "$script" | base64 | tr -d '\n') | base64 -d)\"'" </dev/null >/dev/null 2>&1
}

where() {
    c=$(client)
    if [ -z "$c" ] || [ "$c" = "$(this_machine)" ]; then
        echo local
    else
        echo "remote $c"
    fi
}

# Run a script on a machine with sh, whatever its login shell is
on() {
    dest=$1
    script=$2
    shift 2
    ssh -o BatchMode=yes -o ConnectTimeout=10 "$dest" \
        "sh -c 'eval \"\$(echo $(printf '%s' "$script" | base64 | tr -d '\n') | base64 -d)\"'" "$@"
}

# Have a machine mount this one, and print where
export_to() {
    c=${1:-$(client)}
    [ -n "$c" ] || die "no machine to export to"
    dest=$(cat "$state/$c.dest" 2>/dev/null) || die "$c is not a known machine"
    # Every machine knows the others by their host names
    on "$dest" "exec \"\$HOME/.config/cosmic-term-agents/ct-mount\" $(this_machine)" </dev/null
}

open_file() {
    [ -e "$1" ] || die "no such file: $1"
    file=$(realpath "$1")
    # Detached, since the opener lasts as long as the window it opens
    if [ "$(where)" = local ]; then
        exec setsid -f xdg-open "$file" >/dev/null 2>&1 </dev/null
    fi
    c=$(client)
    m=$(export_to "$c") || exit 1
    dest=$(cat "$state/$c.dest")
    quoted=$(printf '%s' "$m$file" | sed "s/'/'\\\\''/g")
    # The desktop's environment, which an ssh session lacks
    on "$dest" "eval \"\$(systemctl --user show-environment | grep -E '^(WAYLAND_DISPLAY|DISPLAY|DBUS_SESSION_BUS_ADDRESS|XDG_RUNTIME_DIR|XDG_CURRENT_DESKTOP|XDG_DATA_DIRS)=' | sed 's/^/export /')\"
exec setsid -f xdg-open '$quoted' >/dev/null 2>&1 </dev/null" </dev/null
}

list() {
    printf 'This machine: %s\n' "$(cat /proc/sys/kernel/hostname)"
    found=
    for f in "$state"/*.dest; do
        [ -e "$f" ] || continue
        found=1
        name=$(basename "$f" .dest)
        if alive "$dir/$name"; then
            status=mounted
        else
            status="not mounted; ct-mount $name mounts it"
        fi
        printf '%s\tssh %s\t%s/\t%s\n' "$name" "$(cat "$f")" "$dir/$name" "$status"
    done
    [ -n "$found" ] || echo "No other machines are known here yet."
}

# Whether a lease on NAME is held, removing stale ones
leased() {
    held=1
    for f in "$state/leases/$1"/*; do
        [ -e "$f" ] || continue
        if flock -n "$f" true 2>/dev/null; then
            rm -f "$f"
        else
            held=0
        fi
    done
    rmdir "$state/leases/$1" 2>/dev/null
    return $held
}

sessions_running() {
    zellij list-sessions -n 2>/dev/null | grep '^ct-' | grep -qv 'EXITED'
}

gc() {
    exec 8>"$state/gc.lock"
    flock -n 8 || exit 0
    running=1
    sessions_running && running=0
    for m in "$dir"/*; do
        [ -d "$m" ] || continue
        name=$(basename "$m")
        mountpoint -q "$m" || { rmdir "$m" 2>/dev/null; continue; }
        if [ -e "$state/$name.dest" ]; then
            [ "$running" = 0 ] && continue
            leased "$name" && continue
        fi
        unmount "$m" && rmdir "$m" 2>/dev/null
    done
}

case "${1:-}" in
    --list) list ;;
    --register) register "${2:-}" "${3:-}" ;;
    --where) where ;;
    --export) export_to "${2:-}" ;;
    --open) open_file "${2:-}" ;;
    --lease)
        valid_name "${2:-}" || die "not a machine name: ${2:-}"
        mkdir -p "$state/leases/$2"
        echo "$state/leases/$2"
        ;;
    --gc) gc ;;
    "" | -h | --help) sed -n '/^# Usage:/,/^# *$/p' "$0" | sed 's/^# \{0,1\}//' ;;
    -*) die "unknown option: $1" ;;
    *) mount_machine "$1" ;;
esac
