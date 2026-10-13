"""Plain text into Typst markup, for the labels bake.py typesets.

Every string a spec supplies (titles, category names, unit phrases) is
plain text; Typst markup gives meaning to many ASCII characters, so each is
escaped here, once, before it enters the model. Checked with Typst 0.15.1 (now implemented once, in Rust):

- a backslash before  \\ # $ * _ @ < > [ ] ` ~ /  so that "$300,000" is not
  mathematics, "//" cannot start a comment and "~" is not a no-break space;
- a backslash before a leading "-", "+" or "=" (a list item or heading), and
  before the full stop of a leading "1." (a numbered list);
- a double hyphen is written "-\\-", so that it does not become a dash;
- quotes are left to Typst's smart quotes ("Int'l" becomes Int’l), and
  "&" is XML's to escape, not Typst's.

A spec string may instead be {"typst": "…"}: markup passed through
unchanged (for mathematics in a label). spec.Text carries either form.
"""
import json

from . import _native


def escape(text):
    """Plain text as Typst markup that typesets to exactly that text (the Rust engine's
    chartgen_render::typst::escape, through the protocol: one implementation for every host)."""
    response = json.loads(_native.dispatch(json.dumps({"version": 1, "operation": "escape", "text": str(text)},
                                                      ensure_ascii=False)))
    if response.get("outcome") != "data":
        raise RuntimeError(f"escape failed: {response.get('error')}")
    return response["value"]


def markup(text):
    """A spec text (plain string, or spec.Text) as Typst markup."""
    if text is None:
        return None
    if hasattr(text, "markup"):
        return text.markup()
    return escape(text)
