"""The native ``figkit`` protocol operation, with the original's exceptions."""
import json

from . import _native

_EXCEPTIONS = {"ValueError": ValueError, "IndexError": IndexError, "KeyError": KeyError,
               "AttributeError": AttributeError, "OSError": OSError}


def call(_call_name, **args):
    """The data of a figkit call; a malformed request is a RuntimeError."""
    request = json.dumps({"version": 1, "operation": "figkit", "call": _call_name, "args": args},
                         ensure_ascii=False, allow_nan=False)
    response = json.loads(_native.dispatch(request))
    if response["outcome"] == "error":
        error = response["error"]
        raise RuntimeError(f"{error['kind']}: {error['message']}")
    return response["value"]


def unwrap(value):
    """The result of a call, raising the original's exception where it raised one."""
    if "raise" in value:
        r = value["raise"]
        raise _EXCEPTIONS.get(r["type"], RuntimeError)(r["message"])
    return value["ok"]


def result(_call_name, **args):
    return unwrap(call(_call_name, **args))


def number(v):
    """A number of a result (non-finite numbers travel as strings)."""
    return float(v) if isinstance(v, str) else v


def path_data(commands, n):
    """SVG path data of path commands, in emit.py's layout, numbers written by n."""
    p = lambda x, y: f"{n(x)},{n(y)}"
    out = []
    for c in commands:
        op = c["op"]
        if op == "move_to":
            out.append("M" + p(c["x"], c["y"]))
        elif op == "line_to":
            out.append("L" + p(c["x"], c["y"]))
        elif op == "cubic_to":
            out.append(f"C{p(c['x1'], c['y1'])} {p(c['x2'], c['y2'])} {p(c['x'], c['y'])}")
        elif op == "arc_to":
            rot = "0" if c["rotation"] == 0 else n(c["rotation"])
            out.append(f"A{n(c['rx'])},{n(c['ry'])} {rot} {int(c['large_arc'])},{int(c['sweep'])} "
                       + p(c["x"], c["y"]))
        elif op == "close":
            out.append("Z")
        else:
            raise ValueError(f"unknown path command {op!r}")
    return " ".join(out)
