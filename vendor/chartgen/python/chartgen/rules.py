"""Python report objects; rule decisions and findings originate in Rust.

The JSON shapes follow the original's (charts/chartgen/rules.py): a finding
lists its non-empty fields plus ``guide`` (always) and ``title`` (for a rule the
original's table lists); a report lists exactly the original's fields, in its
order. ``copies`` (extra outputs by suffix) is an attribute, not a JSON field,
as in the original.
"""
from dataclasses import dataclass, field

REPORT_FIELDS = ("chart", "style", "variant", "type", "status", "decisions", "findings", "overrides",
                 "expect", "expect_met", "source", "summary", "path")


@dataclass
class Finding:
    rule: str
    severity: str
    message: str
    path: str | None = None
    supply: str | None = None
    elements: list = field(default_factory=list)
    reason: str | None = None
    level: str = "locked"
    recommended: str | None = None
    guide: list = field(default_factory=list)
    title: str | None = None

    def to_json(self):
        d = {k: v for k, v in vars(self).items() if k not in ("guide", "title") and v not in (None, [], "")}
        d["guide"] = list(self.guide or [])
        if self.title:
            d["title"] = self.title
        return d


class Report:
    def __init__(self, chart="?", style="plain", **fields):
        self.chart, self.style = chart, style
        self.variant = None
        self.type = None
        self.status = "drawn"
        self.decisions, self.findings, self.overrides = [], [], []
        self.expect, self.source, self.summary, self.copies = {}, {}, {}, {}
        self.expect_met = True
        self.path = []
        self.__dict__.update(fields)
        if self.copies is None:
            self.copies = {}
        self.findings = [Finding(**f) if isinstance(f, dict) else f for f in self.findings]

    @classmethod
    def from_json(cls, value):
        missing = [k for k in (*REPORT_FIELDS, "copies") if k not in value]
        if missing:
            raise ValueError(f"Rust report lacks the field(s) {', '.join(missing)}")
        return cls(**value)

    def to_json(self):
        out = {k: getattr(self, k) for k in REPORT_FIELDS}
        out["findings"] = [f.to_json() for f in self.findings]
        return out

    def errors(self):
        return [f for f in self.findings if f.severity == "error"]

    def rules_found(self, severities=("error", "warn", "info", "overridden")):
        return {f.rule for f in self.findings if f.severity in severities}
