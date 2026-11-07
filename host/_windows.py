"""Win32 security and process ownership. Imported only on native Windows."""
from __future__ import annotations

import ctypes as C
from ctypes import wintypes as W
import os
import tempfile
import threading
from pathlib import Path

k = C.WinDLL("kernel32", use_last_error=True)
a = C.WinDLL("advapi32", use_last_error=True)


def api(dll, name, result, *args):
    f = getattr(dll, name)
    f.restype, f.argtypes = result, args
    return f


close = api(k, "CloseHandle", W.BOOL, W.HANDLE)
local_free = api(k, "LocalFree", W.HANDLE, W.HANDLE)
current_process = api(k, "GetCurrentProcess", W.HANDLE)
open_token = api(a, "OpenProcessToken", W.BOOL, W.HANDLE, W.DWORD, C.POINTER(W.HANDLE))
token_info = api(a, "GetTokenInformation", W.BOOL, W.HANDLE, C.c_int, W.LPVOID, W.DWORD, C.POINTER(W.DWORD))
sid_string = api(a, "ConvertSidToStringSidW", W.BOOL, W.LPVOID, C.POINTER(W.LPWSTR))
convert_sd = api(a, "ConvertStringSecurityDescriptorToSecurityDescriptorW", W.BOOL,
                 W.LPCWSTR, W.DWORD, C.POINTER(W.LPVOID), C.POINTER(W.DWORD))
set_security = api(a, "SetFileSecurityW", W.BOOL, W.LPCWSTR, W.DWORD, W.LPVOID)
attributes = api(k, "GetFileAttributesW", W.DWORD, W.LPCWSTR)
get_security = api(a, "GetNamedSecurityInfoW", W.DWORD, W.LPCWSTR, C.c_int, W.DWORD,
                   C.POINTER(W.LPVOID), W.LPVOID, W.LPVOID, W.LPVOID, C.POINTER(W.LPVOID))


def checked(ok):
    if not ok:
        raise C.WinError(C.get_last_error())
    return ok


def current_sid():
    token = W.HANDLE()
    checked(open_token(current_process(), 8, C.byref(token)))
    try:
        size = W.DWORD()
        token_info(token, 1, None, 0, C.byref(size))
        buf = C.create_string_buffer(size.value)
        checked(token_info(token, 1, buf, size, C.byref(size)))
        sid = C.cast(buf, C.POINTER(W.LPVOID))[0]
        result = W.LPWSTR()
        checked(sid_string(sid, C.byref(result)))
        try:
            return result.value
        finally:
            local_free(C.cast(result, W.HANDLE))
    finally:
        close(token)


class SecurityAttributes(C.Structure):
    _fields_ = [("length", W.DWORD), ("descriptor", W.LPVOID), ("inherit", W.BOOL)]


def private_descriptor():
    descriptor = W.LPVOID()
    checked(convert_sd(f"D:P(A;OICI;FA;;;{current_sid()})(A;OICI;FA;;;SY)", 1,
                       C.byref(descriptor), None))
    return descriptor


def reject_reparse(path):
    for part in [path, *path.parents]:
        value = attributes(str(part))
        if value != 0xffffffff and value & 0x400:
            raise OSError("Deductum state must not pass through a reparse point")


def check_owner(path):
    owner, security = W.LPVOID(), W.LPVOID()
    error = get_security(str(path), 1, 1, C.byref(owner), None, None, None, C.byref(security))
    if error:
        raise C.WinError(error)
    owner_text = W.LPWSTR()
    try:
        checked(sid_string(owner, C.byref(owner_text)))
        if owner_text.value != current_sid():
            raise OSError("Deductum state is owned by another user")
    finally:
        if owner_text:
            local_free(C.cast(owner_text, W.HANDLE))
        local_free(security)


def secure_file(path):
    """Replace explicit legacy file ACLs; directory ACLs cannot revoke them."""
    path = Path(path)
    reject_reparse(path)
    check_owner(path)
    if not path.is_file():
        raise OSError("Deductum endpoint or lock is not a regular file")
    descriptor = private_descriptor()
    try:
        checked(set_security(str(path), 4 | 0x80000000, descriptor))
    finally:
        local_free(descriptor)


def replace_preserving_dacl(staged, destination):
    """Replace an upstream configuration file without broadening its ACL."""
    staged, destination = Path(staged), Path(destination)
    reject_reparse(destination)
    reject_reparse(staged)
    if destination.exists():
        check_owner(destination)
        descriptor = W.LPVOID()
        error = get_security(str(destination), 1, 4, None, None, None, None, C.byref(descriptor))
        if error:
            raise C.WinError(error)
        try:
            control, revision = W.WORD(), W.DWORD()
            get_control = api(a, "GetSecurityDescriptorControl", W.BOOL, W.LPVOID,
                              C.POINTER(W.WORD), C.POINTER(W.DWORD))
            checked(get_control(descriptor, C.byref(control), C.byref(revision)))
            protection = 0x80000000 if control.value & 0x1000 else 0x20000000
            checked(set_security(str(staged), 4 | protection, descriptor))
        finally:
            local_free(descriptor)
    os.replace(staged, destination)


def check_private_dacl(path, *, protected=False):
    """Accept only the simple current-user/System ACLs created for our cache."""
    path = Path(path)
    reject_reparse(path)
    check_owner(path)
    dacl, descriptor = W.LPVOID(), W.LPVOID()
    error = get_security(str(path), 1, 4, None, None, C.byref(dacl), None, C.byref(descriptor))
    if error:
        raise C.WinError(error)
    try:
        if protected:
            control, revision = W.WORD(), W.DWORD()
            get_control = api(a, "GetSecurityDescriptorControl", W.BOOL, W.LPVOID,
                              C.POINTER(W.WORD), C.POINTER(W.DWORD))
            checked(get_control(descriptor, C.byref(control), C.byref(revision)))
            if not control.value & 0x1000:
                raise OSError("The Isabelle cache root does not have a protected DACL")
        if not dacl:
            raise OSError("The Isabelle cache has a permissive NULL DACL")
        class AclSize(C.Structure):
            _fields_ = [(name, W.DWORD) for name in ("count", "used", "free")]
        size = AclSize()
        get_info = api(a, "GetAclInformation", W.BOOL, W.LPVOID, W.LPVOID, W.DWORD, C.c_int)
        get_ace = api(a, "GetAce", W.BOOL, W.LPVOID, W.DWORD, C.POINTER(W.LPVOID))
        checked(get_info(dacl, C.byref(size), C.sizeof(size), 2))
        allowed = {current_sid(), "S-1-5-18"}
        for index in range(size.count):
            ace = W.LPVOID()
            checked(get_ace(dacl, index, C.byref(ace)))
            # ACCESS_ALLOWED_ACE has an 8-byte header/mask before its SID.
            if C.cast(ace, C.POINTER(C.c_ubyte))[0] != 0:
                raise OSError("The Isabelle cache has an unsupported DACL entry")
            sid = W.LPWSTR()
            checked(sid_string(W.LPVOID(ace.value + 8), C.byref(sid)))
            try:
                if sid.value not in allowed:
                    raise OSError("The Isabelle cache grants access to another user")
            finally:
                local_free(C.cast(sid, W.HANDLE))
    finally:
        local_free(descriptor)


def secure_directory(path):
    """Create the state root privately, or replace its DACL before using it."""
    path = Path(os.path.abspath(path))
    # An override names app-owned state, never permission to secure an entire
    # profile, shared temporary directory, AppData root or drive.
    broad = [Path.home(), Path(tempfile.gettempdir()), Path(path.anchor)]
    broad.extend(Path(os.environ[name]) for name in ("USERPROFILE", "APPDATA", "LOCALAPPDATA")
                 if os.environ.get(name))
    if any(path == root or path in root.parents for root in broad):
        raise OSError("Deductum state must be a dedicated application directory")
    reject_reparse(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = private_descriptor()
    create = api(k, "CreateDirectoryW", W.BOOL, W.LPCWSTR, C.POINTER(SecurityAttributes))
    try:
        attrs = SecurityAttributes(C.sizeof(SecurityAttributes), descriptor, False)
        if not create(str(path), C.byref(attrs)) and C.get_last_error() != 183:
            raise C.WinError(C.get_last_error())
        reject_reparse(path)
        check_owner(path)
        checked(set_security(str(path), 4 | 0x80000000, descriptor))  # protected DACL
        if not path.is_dir():
            raise OSError("Deductum state root is not a directory")
    finally:
        local_free(descriptor)


class IO_COUNTERS(C.Structure):
    _fields_ = [(n, C.c_ulonglong) for n in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
                                            "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class BASIC_LIMIT(C.Structure):
    _fields_ = [("PerProcessUserTimeLimit", C.c_longlong), ("PerJobUserTimeLimit", C.c_longlong),
                ("LimitFlags", W.DWORD), ("MinimumWorkingSetSize", C.c_size_t),
                ("MaximumWorkingSetSize", C.c_size_t), ("ActiveProcessLimit", W.DWORD),
                ("Affinity", C.c_size_t), ("PriorityClass", W.DWORD), ("SchedulingClass", W.DWORD)]


class EXTENDED_LIMIT(C.Structure):
    _fields_ = [("BasicLimitInformation", BASIC_LIMIT), ("IoInfo", IO_COUNTERS),
                ("ProcessMemoryLimit", C.c_size_t), ("JobMemoryLimit", C.c_size_t),
                ("PeakProcessMemoryUsed", C.c_size_t), ("PeakJobMemoryUsed", C.c_size_t)]


class ACCOUNTING(C.Structure):
    _fields_ = [(n, C.c_longlong) for n in ("TotalUserTime", "TotalKernelTime", "ThisPeriodTotalUserTime",
                                           "ThisPeriodTotalKernelTime")] + [(n, W.DWORD) for n in
               ("TotalPageFaultCount", "TotalProcesses", "ActiveProcesses", "TotalTerminatedProcesses")]


create_job = api(k, "CreateJobObjectW", W.HANDLE, W.LPVOID, W.LPCWSTR)
open_job = api(k, "OpenJobObjectW", W.HANDLE, W.DWORD, W.BOOL, W.LPCWSTR)
set_job = api(k, "SetInformationJobObject", W.BOOL, W.HANDLE, C.c_int, W.LPVOID, W.DWORD)
query_job = api(k, "QueryInformationJobObject", W.BOOL, W.HANDLE, C.c_int, W.LPVOID, W.DWORD, W.LPVOID)
assign_job = api(k, "AssignProcessToJobObject", W.BOOL, W.HANDLE, W.HANDLE)
terminate_job = api(k, "TerminateJobObject", W.BOOL, W.HANDLE, W.UINT)
process_times = api(k, "GetProcessTimes", W.BOOL, W.HANDLE, C.POINTER(W.FILETIME),
                    C.POINTER(W.FILETIME), C.POINTER(W.FILETIME), C.POINTER(W.FILETIME))
wait_handle = api(k, "WaitForSingleObject", W.DWORD, W.HANDLE, W.DWORD)


def creation_time(handle=None):
    created, exited, kernel, user = (W.FILETIME() for _ in range(4))
    checked(process_times(current_process() if handle is None else handle, C.byref(created),
                          C.byref(exited), C.byref(kernel), C.byref(user)))
    return (created.dwHighDateTime << 32) | created.dwLowDateTime


def watch_owner(pid, expected_creation, job):
    """Hold a specific host lifetime, validating identity before backend creation."""
    open_process = api(k, "OpenProcess", W.HANDLE, W.DWORD, W.BOOL, W.DWORD)
    owner = checked(open_process(0x100000 | 0x1000, False, pid))
    try:
        if creation_time(owner) != expected_creation or wait_handle(owner, 0) != 258:
            raise OSError("The owning Deductum host exited before prover launch")
    except BaseException:
        close(owner)
        raise
    def watch():
        try:
            # Failure to observe ownership is a containment failure too.
            wait_handle(owner, 0xffffffff)
            terminate_job(job, 1)
        finally:
            close(owner)
    threading.Thread(target=watch, name="prover-owner", daemon=True).start()


def join_job(name):
    handle = checked(create_job(None, name))
    try:
        limit = EXTENDED_LIMIT()
        limit.BasicLimitInformation.LimitFlags = 0x2000  # KILL_ON_JOB_CLOSE, no breakaway
        checked(set_job(handle, 9, C.byref(limit), C.sizeof(limit)))
        checked(assign_job(handle, current_process()))
        return handle
    except BaseException:
        close(handle)
        raise


def kill_job(name):
    handle = open_job(8, False, name)
    if not handle:
        if C.get_last_error() == 2:
            return
        raise C.WinError(C.get_last_error())
    try:
        checked(terminate_job(handle, 1))
    finally:
        close(handle)


class MEMORY_COUNTERS(C.Structure):
    _fields_ = [("cb", W.DWORD), ("PageFaultCount", W.DWORD)] + [(n, C.c_size_t) for n in
               ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage",
                "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]


def job_usage(name):
    """CPU in 100ns units and tree RSS bytes; no persistent query handle."""
    handle = open_job(4, False, name)
    if not handle:
        return None
    try:
        acc = ACCOUNTING()
        checked(query_job(handle, 1, C.byref(acc), C.sizeof(acc), None))
        # The process-ID list is a pointer-sized variable array. Retry if it grew.
        count = max(acc.ActiveProcesses + 16, 32)
        for _ in range(4):
            buf = C.create_string_buffer(8 + count * C.sizeof(C.c_size_t))
            if query_job(handle, 3, buf, len(buf), None):
                break
            if C.get_last_error() != 234:
                return None
            count *= 2
        else:
            return None
        assigned = C.cast(buf, C.POINTER(W.DWORD))[1]
        pids = C.cast(C.addressof(buf) + 8, C.POINTER(C.c_size_t))
        open_process = api(k, "OpenProcess", W.HANDLE, W.DWORD, W.BOOL, W.DWORD)
        memory = api(k, "K32GetProcessMemoryInfo", W.BOOL, W.HANDLE, W.LPVOID, W.DWORD)
        rss = 0
        for i in range(assigned):
            proc = open_process(0x410, False, pids[i])
            if not proc:
                return None
            try:
                info = MEMORY_COUNTERS()
                info.cb = C.sizeof(info)
                if not memory(proc, C.byref(info), info.cb):
                    return None
                rss += info.WorkingSetSize
            finally:
                close(proc)
        return acc.TotalUserTime + acc.TotalKernelTime, rss
    finally:
        close(handle)


def process_parents(pid, depth=4):
    """Observe ancestry for MCP identity; never use snapshots for termination."""
    class ENTRY(C.Structure):
        _fields_ = [("dwSize", W.DWORD), ("cntUsage", W.DWORD), ("pid", W.DWORD),
                    ("heap", C.c_size_t), ("module", W.DWORD), ("threads", W.DWORD),
                    ("parent", W.DWORD), ("priority", W.LONG), ("flags", W.DWORD),
                    ("exe", W.WCHAR * 260)]
    snapshot = api(k, "CreateToolhelp32Snapshot", W.HANDLE, W.DWORD, W.DWORD)(2, 0)
    if snapshot == C.c_void_p(-1).value:
        raise C.WinError(C.get_last_error())
    first = api(k, "Process32FirstW", W.BOOL, W.HANDLE, C.POINTER(ENTRY))
    next_entry = api(k, "Process32NextW", W.BOOL, W.HANDLE, C.POINTER(ENTRY))
    entries = {}
    try:
        entry = ENTRY()
        entry.dwSize = C.sizeof(entry)
        more = first(snapshot, C.byref(entry))
        while more:
            entries[entry.pid] = (entry.parent, entry.exe)
            more = next_entry(snapshot, C.byref(entry))
    finally:
        close(snapshot)
    result = []
    for _ in range(depth):
        if pid not in entries:
            break
        parent, name = entries[pid]
        result.append((name, pid))
        pid = parent
    return result


def ascii_short_path(path):
    """Use an existing NTFS name for narrow-argv tools; never create aliases."""
    path = os.path.abspath(path)
    if path.isascii():
        return path
    get_short = api(k, "GetShortPathNameW", W.DWORD, W.LPCWSTR, W.LPWSTR, W.DWORD)
    buffer = C.create_unicode_buffer(32768)
    size = get_short(path, buffer, len(buffer))
    if not size or size >= len(buffer) or not buffer.value.isascii():
        raise OSError("Isabelle's Windows launcher needs an existing ASCII NTFS short name for this path; "
                      "move the installation or project to an ASCII path on a volume with short names enabled")
    short = buffer.value
    if not os.path.samefile(path, short) or os.path.normcase(os.path.realpath(path)) != os.path.normcase(os.path.realpath(short)):
        raise OSError("The existing NTFS short name does not identify the original Isabelle path")
    return short
