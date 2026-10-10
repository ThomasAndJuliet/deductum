cosmic-terminal = COSMIC Terminal
comment = Terminal emulator for the COSMIC desktop
keywords = Command;Shell;Terminal;CLI;
new-terminal = New terminal

# Context Pages

## About
repository = Repository
support = Support

## Color schemes
color-schemes = Color schemes
rename = Rename
export = Export
delete = Delete
import = Import
import-errors = Import errors

## Profiles
profiles = Profiles
name = Name
command-line = Command line
tab-title = Tab title
tab-title-description = Override the default tab title
add-profile = Add profile
new-profile = New profile
make-default = Make default
working-directory = Working directory
hold = Hold
remain-open = Remain open after child process exits.

## Settings
settings = Settings

### Appearance
appearance = Appearance
theme = Theme
match-desktop = Match desktop
dark = Dark
light = Light
syntax-dark = Color scheme dark
syntax-light = Color scheme light
default-zoom-step = Zoom steps
opacity = Background opacity

### Font
font = Font
advanced-font-settings = Advanced font settings
default-font = Font
default-font-size = Font size
default-font-stretch = Font stretch
default-font-weight = Normal font weight
default-dim-font-weight = Dim font weight
default-bold-font-weight = Bold font weight
use-bright-bold = Make bold text brighter

### Splits
splits = Splits
focus-follow-mouse = Typing focus follows mouse
show-pane-borders = Show pane borders
accent-follows-tab = Accent colour follows the tab
accent-follows-tab-description = The tab in front's colour, such as Claude's orange, becomes the accent colour of the window.

### Advanced
advanced = Advanced
show-headerbar = Show header
window-header = Window header
show-header-description = Reveal the header from the right-click menu
tab-new-inherit-working-directory = New tabs and windows use current directory
tab-new-inherit-working-directory-description = Open new tabs and windows in the active tab's working directory

### Keyboard shortcuts
add-another-keybinding = Add another keybinding
cancel = Cancel
close-window = Close window
copy-or-sigint = Copy or SIGINT
disable = Disable
focus-pane-down = Focus pane down
focus-pane-left = Focus pane left
focus-pane-right = Focus pane right
focus-pane-up = Focus pane up
keyboard-shortcuts = Keyboard shortcuts
menu-keyboard-shortcuts = Keyboard shortcuts...
no-shortcuts = No shortcuts
password-manager = Password manager
paste-primary = Paste primary
replace = Replace
reset-to-default = Reset to default
shortcut-capture-hint = Press the key combination
shortcut-group-clipboard = Clipboard
shortcut-group-other = Other
shortcut-group-tabs = Tabs
shortcut-group-window = Window
shortcut-group-zoom = Zoom
shortcut-replace-body = { $binding } is already assigned to { $existing }. Replace it with { $new_action }?
shortcut-replace-title = Replace shortcut?
tab-activate = Activate tab { $number }
toggle-fullscreen = Toggle fullscreen
type-to-search = Type to search...

# Find
find-placeholder = Find...
find-previous = Find previous
find-next = Find next

# Menu

## File
file = File
new-tab = New tab
new-window = New window
profile = Profile
menu-profiles = Profiles...
close-tab = Close tab
quit = Quit

## Edit
edit = Edit
copy = Copy
paste = Paste
select-all = Select all
find = Find
clear-scrollback = Clear scrollback

## Open
open-link = Open Link
copy-link = Copy Link

## View
view = View
zoom-in = Larger text
zoom-reset = Default text size
zoom-out = Smaller text
next-tab = Next tab
previous-tab = Previous tab
split-horizontal = Split horizontal
split-vertical = Split vertical
pane-toggle-maximize = Maximize or restore pane
menu-color-schemes = Color schemes...
menu-settings = Settings...
menu-about = About COSMIC Terminal...

# Password Manager
menu-password-manager = Passwords...
passwords-title = Passwords
add-password = Add Password
password-input = Password
password-input-description = Description

## Agent sessions
agent = Agent
agent-shell = Terminal
host = Remote host
host-description = An ssh destination such as user@host. Leave empty to run on this computer. Remote sessions run in Zellij and keep running when the tab is closed.
machine = Runs on
machine-description = This computer, or one of the machines above. Remote sessions run in Zellij and keep running when the tab is closed.
machines = Machines
machine-detail = {$destination} · {$hostname}
machine-host = Host
machine-host-description = What ssh connects to: an alias from ~/.ssh/config, a host name or an address.
machine-user = User name
machine-user-description = The account to log in as there. Leave empty for ssh's choice; a check fills it in.
machine-check = Check
machine-checking = Checking…
machine-checked = Logged in as {$user} on {$hostname}, home {$home}.
machine-is-this-computer = This is this computer.
machine-same-as = This is the same computer as {$name}.
machine-add-profiles = Add Claude, Codex and Terminal profiles for {$name}
machine-profile-name = {$agent} on {$name}
machine-add = Add machine…
machine-found = Computers found
machine-looking = Looking in Tailscale and ~/.ssh/config…
machine-none-found = No other computers found. Add one by hand.
machine-add-this = Add
machine-add-blank = Add by hand
machine-from-tailscale = Tailscale
machine-from-ssh-config = ~/.ssh/config
machine-offline = offline
machine-in-use = Profiles run on this machine: {$profiles}. Move or delete them first.
new-machine = New machine
agent-arguments = Agent arguments
new-tab-menu = Show profiles and sessions
filter = Filter…
no-matching-results = No matching results
this-computer = This computer
on-host = On {$host}
remote-sessions = Sessions on {$host}
no-remote-sessions = No other sessions
listing-sessions = Listing sessions…
session-list-failed = Could not list sessions: {$error}
end-remote-session = End session
default-profile-marker = default
open-in-zed = Open in Zed
open-in-zed-failed = Could not open in Zed: {$error}
open-in-files = Open in file manager
open-in-files-failed = Could not open in the file manager: {$error}
tab-color = Tab colour
tab-color-default = Default
other-zellij-sessions = Other Zellij sessions on {$host}
session-exited = {$name} (exited)
this-computer-lower = this computer
move-to-new-window = Move to new window
open-locally = Open locally
open-locally-failed = Could not open locally: {$error}
detach-session = Detach (keep running)
launcher-home = Home
launcher-browse = Browse…
launcher-filter = Filter projects and conversations…
launcher-loading = Looking for projects and conversations…
launcher-continuing = Preparing the conversation…
launcher-mounting = Connecting to the computer’s files…
launcher-busy-close = Wait for this operation to finish before closing its tab.
opening-locally = Opening the selected remote item locally…
opening-files = Opening the file manager…
opening-zed = Opening the project in Zed…
renaming-conversation = Renaming the conversation…
ending-session = Ending the session…
host-setup = Preparing {$host}: {$step}
host-setup-tools = checking required tools
host-setup-supervisor = installing the session supervisor
host-setup-transfer = installing the transfer helper
host-setup-sudo = installing sudo support
host-setup-zellij = checking Zellij
host-setup-chartgen = installing the chart checker
host-setup-waiting = No setup step completed in 15 seconds. Still waiting for this host…
color-scheme-importing = Importing colour schemes…
color-scheme-exporting = Exporting colour scheme…
color-scheme-waiting = No file operation completed in 15 seconds. Still waiting for the file system…
password-store-waiting = Waiting for the password store…
launcher-failed = Could not list projects: {$error}
launcher-running = Running
launcher-projects = Projects
launcher-conversations = Recent conversations
launcher-new-worktree = New worktree
launcher-move-here = Move here
launcher-fork-here = Fork here
launcher-on-host = {$detail} · {$agent} on {$host}
launcher-other-agent = {$detail} · {$agent}
launcher-offline = {$when} · offline
launcher-host-offline = {$host} is unreachable ({$error}); its conversations are as last listed.

## New worktree
worktree-title = New worktree
worktree-loading = Fetching and listing branches…
worktree-creating = Creating the worktree…
worktree-repository = Repository: {$path}
worktree-branch = New branch
worktree-base = Start from
worktree-base-current = This worktree ({$branch})
worktree-base-current-dirty = This worktree ({$branch}), with its uncommitted changes
worktree-detached = detached
worktree-not-fetched = Could not fetch ({$error}), so this is the last fetched state.
worktree-path = Folder
worktree-create = Create and open

## Transferring a session from the other computer
transfer-move-title = Move here
transfer-fork-title = Fork here
transfer-checking = Checking both computers…
transfer-scan-partial = The repository search stopped after its time limit; some local repositories may be missing.
tab-transfer-checking = Checking the running conversation before transfer…
transfer-source = Branch {$branch} on {$host}, in {$path}
transfer-seeded = Continues as a new {$agent} conversation, given the old one's transcript.
transfer-repository = Into the repository {$path}
transfer-choose-repository = Which repository on this computer is this project's? The choice is remembered.
transfer-same-remote = {$path} (same remote)
transfer-new-repository = A new repository at {$path}
transfer-reuse = Updates {$path}, the copy left here by an earlier transfer.
transfer-fork-branch = On the new branch {$branch}
transfer-running = Transferring…
transfer-stage-check = Checking the source and saving a snapshot…
transfer-stage-copy = Copying the repository…
transfer-stage-prepare = Preparing the destination worktree…
transfer-stage-files = Copying carried files…
transfer-stage-conversation = Copying the conversation…
transfer-stage-commit = Committing the destination…
transfer-stage-cleanup = Finishing the transfer…
transfer-stage-waiting = No step completed in 15 seconds. Still waiting for this operation…
transfer-recovering = Recovering the transfer…
transfer-move = Move
transfer-fork = Fork
transfer-unfinished = Unfinished transfer of {$branch} from {$host}
transfer-resume = Resume
transfer-roll-back = Roll back
transfer-check-again = Check again
move-to = Move to {$profile}…
fork-to = Fork to {$profile}…
tab-transfer-no-agent = Cannot move this tab: {$error}
time-just-now = just now
time-minutes-ago = {$n} min ago
time-hours-ago = {$n} h ago
time-days-ago = {$n} d ago
tab-disconnected = {$title} (disconnected)
disconnected-banner = Disconnected from {$host}. Reconnecting when it is reachable again…
reconnect-now = Reconnect now
session-ended-while-disconnected = A session on {$host} ended while it was out of reach

## Sidebar
sidebar-toggle = Show running sessions
sidebar-new = New
sidebar-new-menu = Choose a profile or session
sidebar-open = Open
sidebar-sudo-waiting = Waiting for your sudo approval
sidebar-search = Search sessions…
sidebar-compact = One line per session
sidebar-expand = Show previews
sidebar-no-matches = No matching sessions
sidebar-loading = Looking for sessions on {$host}…
sidebar-refreshing = Refreshing sessions on {$host}…
sidebar-failed = Could not list the sessions on {$host}: {$error}
sidebar-empty = Nothing running. Sessions started here, or on your other machines, are listed here.
sidebar-no-project = Other
sidebar-working = Working
sidebar-ready = Waiting for you
sidebar-running = Running a command
sidebar-open-elsewhere = open elsewhere
sidebar-new-session = New session in {$project}
sidebar-new-worktree = New worktree of {$project}
sidebar-no-profile = No profile runs on {$host}
shared-banner = This session is open in another window too.
shared-go-there = Go to that window
shared-move-here = Move it here
shared-keep-both = Keep it in both windows
sidebar-rename = Rename…
sidebar-rename-auto = Name automatically
rename-title = Rename conversation
rename-body = The agent is asked to rename it, as with its /rename command.
rename-failed = Could not rename the conversation: {$error}

## Sudo approvals
sudo-password = {$host} password
sudo-approve = Approve
sudo-request = sudo on {$host}: {$command}
sudo-on = sudo on {$host}:
sudo-popup-title = sudo on {$host}
sudo-popup-from = in {$cwd}, from {$session}
sudo-popup-unknown = in {$cwd}
sudo-touch = Touch the reader
sudo-touch-or-type = Touch the reader, or
sudo-auto-approved = Approved by your policy: sudo on {$host}: {$command}
sudo-reading = Reading…
sudo-checking = Checking the password…
sudo-no-reader = Fingerprint unavailable: {$why}
secrets-store-password = Secrets store password
sudo-deny = Deny
sudo-settings = Sudo approvals
sudo-settings-about = Approve agents' sudo here
sudo-settings-description = An agent's sudo, on this computer or another, asks you here instead of failing. You type that machine's sudo password only here: it unlocks a key that approves the request, and never leaves this computer.
sudo-no-agent = The sudo agent is not running
sudo-key-ready = Set up
sudo-key-none = Not set up
sudo-set-up = Set up…
sudo-set-up-again = Set up again…
sudo-no-fingers = No finger enrolled: approvals are by click
sudo-fingerprint = Fingerprint
sudo-enrol = Enrol
sudo-enrol-touch = Touch the reader, again and again until it is done
sudo-setup-title = Set up sudo approvals on {$machine}
sudo-setup-body = Type {$machine}'s sudo password twice. It stays on this computer, locking the key that approves sudo there. A tab on {$machine} then asks for the password once more, to install what checks the approvals.
sudo-setup-password = Password
sudo-setup-again = The password again
sudo-setup-go = Set up
sudo-failed = Could not set up sudo approvals: {$error}
sudo-notify-title = sudo on {$host}

## Menus
session-menu = Session
help-menu = Help
menu-sidebar = Sidebar
rename-menu = Rename…
move-to-menu = Move to
fork-to-menu = Fork to

## Secrets
menu-secrets = Passwords and sudo…
secrets-title = Passwords and sudo
this-computer-named = This computer ({$name})
secrets-create-title = Make a secrets store
secrets-create-body = Saved passwords and the keys that approve sudo are kept here, sealed with a password of your choosing. It stays on this computer.
secrets-create = Make the store
secrets-unlock = Unlock
secrets-lock = Lock
secrets-unlocked = Unlocked · locks in {$minutes} min
secrets-sudo-set-up = Approves sudo
secrets-sudo-not-set-up = Does not approve sudo yet
secrets-paste = Paste
secrets-edit = Edit
secrets-delete = Delete
secrets-account = Account
secrets-name = Name
secrets-secret = Password
secrets-secret-unchanged = Password (unchanged if empty)
secrets-note = Note
secrets-always-ask = Ask for the store password each time
secrets-save = Save
secrets-add-password = + Add a password
secrets-other = Other
secrets-add = + Add…
secrets-import = Import from the keyring
secrets-imported = from the keyring
secrets-policy = Policy
secrets-lock-after = Lock after
secrets-minutes = {$minutes} min
secrets-lock-with-screen = Lock with the screen and on sleep
secrets-sudo-approval = Approve sudo by
secrets-approve-nothing = Nothing: approve while the store is open
secrets-approve-fingerprint = Fingerprint or the store password
secrets-approve-click = A click
secrets-approve-password = The store password
secrets-per-session = Each session's first sudo needs the password
secrets-paste-asks = Pasting needs the password
secrets-policy-locked = Unlock the store to change these.

# Programs a machine lacks
tools-missing = {$machine} is missing {$what}
tool-missing = {$tool} (needed for {$needed})
tool-python3 = the sidebar, previews and opening in an editor
tool-git = worktrees and moving sessions
tool-mounts = other machines' files

# Pasting saved passwords from the right-click menu
paste-password = Paste password
paste-password-on = {$account} on {$machine}
paste-password-unlock = Unlock to paste a password…

# Host setup for opening other computers' files
remote-files-title = Remote files
remote-files-intro = SSHFS lets you browse another computer's files here. Local terminals work without it.
remote-files-ssh = Use your usual SSH connection and keys. If a computer is new, connect once in a terminal to accept its host key. For access in both directions, SSH must work both ways.
remote-files-checking = Checking tools…
remote-files-not-checked = Check this computer to see what it needs.
remote-files-check-failed = We couldn't check this computer. Review the message below, then try again.
remote-files-ready = The tools are ready. Try opening your remote folder again.
remote-files-missing = To open remote files, this computer needs: {$tools}.
remote-files-install-on = Install on {$machine}. This opens a terminal there and may ask for your administrator password.
remote-files-install = Install tools…
remote-files-copy = Copy command
remote-files-manual = Install the missing tools using this computer's package manager, then check again. Your system administrator can help if you don't have permission to install packages.
remote-files-reboot = After installation, restart {$machine} when you're ready. Then return here and check again. Atomic activates packages at the next boot.
remote-files-after-install = When installation finishes, return here and check again.
remote-files-fuse = The tools are installed, but this account can't access FUSE. Ask your system administrator to enable FUSE access, then check again.
remote-files-connection = The last connection attempt needs attention:
remote-files-check = Check again
remote-files-help = Help set up
remote-files-connect-failed = Remote file access to {$machine} isn't ready.

remote-files-pending = Installation is ready for your next boot.

## Agent threads
threads = Threads
thread-paper = Maths typesetting
thread-paper-description = Set threads as a paper: serif text, justified, in a column, with LaTeX set as in print
thread-match-colours = Images match the colours
thread-match-colours-description = Ask Claude to draw its plots and diagrams in the thread's colour scheme
thread-paper-font = Paper font
thread-font-libertinus = Libertinus Serif (Typst)
thread-font-new-computer-modern = New Computer Modern (LaTeX)
thread-font-serif = System serif
thread-font-sans = System sans-serif
thread-paper-size = Paper text size
thread-paper-line-height = Paper line spacing
thread-paper-measure = Paper text width
thread-ems = {$n} ems
thread-paper-justify = Justify paper text
thread-paper-tone = Paper colours
thread-tone-theme = Follow the theme
thread-tone-white = White paper
thread-tone-sepia = Sepia
thread-math-scale = Formula size
thread-math-scale-description = Against the text around it; KaTeX uses 121%
thread-text-size = Thread text size
thread-idle = Stop idle agents after
thread-idle-description = A stopped agent frees its memory, and resumes its conversation when you next send a message
thread-minutes = {$n ->
    [one] 1 minute
   *[other] {$n} minutes
}
thread-live-agents = Agents running at once, at most
thread-loaded = Threads kept in memory while hidden
thread-memory = Now {$loaded} loaded, {$agents} agents running, about {$size} MB of thread text and formulas
dark-mode = Dark mode
light-mode = Light mode
thread-working = Working…
thread-context = {$tokens} tokens in context
thread-loading = Reading the conversation…
thread-load-failed = The conversation could not be read: {$error}
thread-load-earlier = {$count ->
    [one] Show the earlier turn
   *[other] Show {$count} earlier turns
}
thread-empty-title = Ask Claude
thread-empty-body = Claude runs through its own command-line tool, so it uses your Claude subscription. LaTeX in replies is typeset.
thread-you = You
thread-thinking = Thinking…
thread-thought = Thought
thread-thought-tokens = Thought for about {$tokens} tokens
thread-permission = Claude wants to use {$tool}
thread-allow = Allow
thread-allow-always = Always allow
thread-deny = Deny
thread-placeholder = Message Claude — Enter sends, Shift+Enter adds a line
thread-attach = Attach images
thread-stop = Stop
thread-send = Send
image = Image
image-fit = Fit
image-actual = Actual size
image-zoom-in = Zoom in
image-zoom-out = Zoom out
image-copy-path = Copy file path
image-open-externally = Open in another app
image-full-screen = Full screen
image-leave-full-screen = Leave full screen
diagram = Diagram
diagram-source = Source
diagram-preview = Preview
diagram-copy-source = Copy Mermaid source
diagram-open-externally = Open as SVG in another app
profile-thread = Open as rich-text thread
