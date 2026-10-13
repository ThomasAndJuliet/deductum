"""Plain text into Typst markup (figkit ``typst``), by the one Rust implementation in
``chartgen-render``."""
from ._call import result


def escape(text):
    """Plain text as Typst markup that typesets to exactly that text."""
    return result("typst_escape", texts=[str(text)])[0]


def markup(text):
    """A spec text (plain string, or an object with .markup()) as Typst markup."""
    if text is None:
        return None
    if hasattr(text, "markup"):
        return text.markup()
    return escape(text)
