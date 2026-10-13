"""The chart spec (SPEC §2): dataclasses, JSON loading, variants, schema.

A spec is JSON. Its schema is these dataclasses: each field's type, default
and doc string, from which `load` parses and checks a JSON value (reporting
every error with its field path, "data.series[0].values[3]") and `schema`
writes a JSON Schema for producers and editors. Later chart types add fields
here; nothing else needs to know the list.

The spec gives facts and the chart's open choices; it has no field that sets
a style value (a font, size, colour, weight or spacing), since those are
locked by the style (SPEC §1.4): another look is another stylesheet. Width
and plot aspect are open choices, within limits. Options with `auto` are the
guided decisions; choosing one explicitly is allowed, and `reasons`, keyed by
the option's path ("chart.orientation"), says why.

Numbers become Decimal, exactly as written. A data value is a Value, which
keeps a known number (0 included), a missing value and a stated status apart
(R6-23, R0-12): nothing is ever coerced to zero.

A spec file holds one spec, or {"base": spec, "expect": …, "variants": [{"name",
"patch", "expect"}]}; each patch is a JSON Merge Patch (RFC 7386) on the base,
and `expect` = {"refused": [rule], "findings": [rule]} is what the variant must
provoke (§2, §1.4).
"""
import copy
import json
import re
import types
from dataclasses import MISSING, dataclass, field, fields, is_dataclass
from datetime import date
from pathlib import Path
from decimal import Decimal
from typing import Any, Literal, Union, get_args, get_origin, get_type_hints


class Num:
    """A JSON number, as a Decimal."""


def F(default=MISSING, doc="", factory=None):
    """A spec field: its default (none: required) and its doc string."""
    md = {"doc": doc}
    if factory is not None:
        return field(default_factory=factory, metadata=md)
    if default is MISSING:
        return field(metadata=md)
    return field(default=default, metadata=md)


class SpecError(Exception):
    def __init__(self, errors):
        super().__init__("; ".join(f"{p}: {m}" for p, m in errors))
        self.errors = errors


# ---- Custom value types ------------------------------------------------------

@dataclass
class Text:
    """A spec string: plain text (escaped for Typst), or {"typst": markup}."""
    plain: str = None
    typst: str = None

    @classmethod
    def from_json(cls, data, path, errors):
        if isinstance(data, str):
            return cls(plain=data)
        if isinstance(data, dict) and set(data) == {"typst"} and isinstance(data["typst"], str):
            return cls(typst=data["typst"])
        errors.append((path, 'expected a string or {"typst": markup}'))
        return None

    @classmethod
    def json_schema(cls):
        return {"oneOf": [{"type": "string"},
                          {"type": "object", "properties": {"typst": {"type": "string"}},
                           "required": ["typst"], "additionalProperties": False}]}

    def markup(self):
        from .typst import escape
        return self.typst if self.typst is not None else escape(self.plain)

    def __str__(self):
        return self.plain if self.plain is not None else self.typst


STATUSES = ("missing", "unknown", "pending", "tbd", "error")
CERTAINTIES = ("actual", "estimate", "projection", "target")
# A censored value (a state of its own, neither known nor missing): a result below a detection limit,
# "<1", or above the top of a test's range, ">2,420"; the limit is known, the value only bounded by it.
CENSORED = ("below", "above")
_CENSORED_TEXT = re.compile(r"\s*([<>])\s*(-?\d+(?:\.\d+)?)\s*")


@dataclass
class Value:
    """One data value: a known number, or not known and why (SPEC §2.4)."""
    value: Decimal = None
    status: str = "known"           # known, or one of STATUSES
    note: str = None
    certainty: str = "actual"
    complete: bool = True
    stated: bool = True             # False for a bare null: missing, reason unstated
    limit: Decimal = None           # a censored value's limit: below 1 ("<1"), above 2420 (">2,420")

    @property
    def known(self):
        return self.status == "known"

    @property
    def censored(self):
        """Below a detection limit or above a test's range: not missing, but not a number to draw."""
        return self.status in CENSORED

    @classmethod
    def from_json(cls, data, path, errors):
        if isinstance(data, bool):
            errors.append((path, "a value is a number, null or an object, not true/false"))
            return None
        if isinstance(data, (int, float, Decimal)):
            return cls(Decimal(str(data)))
        if data is None:
            return cls(status="missing", stated=False)
        if isinstance(data, str):                   # "<1", ">2420": a censored value, as a lab writes it
            m = _CENSORED_TEXT.fullmatch(data)
            if m is None:
                errors.append((path, 'a value given as text is censored, "<1" or ">2420"; otherwise a number'))
                return None
            return cls(status="below" if m.group(1) == "<" else "above", limit=Decimal(m.group(2)))
        if isinstance(data, dict):
            side = [k for k in CENSORED if k in data]
            if side:                                # {"below": 1}: below a detection limit (or above a range)
                lim = data[side[0]]
                extra = set(data) - {side[0], "note"}
                if len(side) > 1 or extra or isinstance(lim, bool) or not isinstance(lim, (int, float, Decimal)) \
                        or not isinstance(data.get("note", ""), str):
                    errors.append((path, "expected {below: limit, note?} or {above: limit, note?}"))
                    return None
                return cls(status=side[0], limit=Decimal(str(lim)), note=data.get("note"))
            if "status" in data:
                extra = set(data) - {"status", "note"}
                if data["status"] not in STATUSES or extra:
                    errors.append((path, f"status must be one of {', '.join(STATUSES)}, with an optional note"))
                    return None
                return cls(status=data["status"], note=data.get("note"))
            if "value" in data:
                extra = set(data) - {"value", "certainty", "complete", "note"}    # note: a footnote (§6.19)
                v = data["value"]
                if extra or isinstance(v, bool) or not isinstance(v, (int, float, Decimal)) \
                        or not isinstance(data.get("note", ""), str):
                    errors.append((path, "expected {value: number, certainty?, complete?, note?}"))
                    return None
                c = data.get("certainty", "actual")
                if c not in CERTAINTIES:
                    errors.append((path + ".certainty", f"must be one of {', '.join(CERTAINTIES)}"))
                    return None
                return cls(Decimal(str(v)), certainty=c, complete=bool(data.get("complete", True)),
                           note=data.get("note"))
        errors.append((path, "expected a number, null, {status, note?}, {value, certainty?, complete?, note?}, "
                             "or a censored value, {below: limit} or {above: limit} (\"<1\", \">2420\")"))
        return None

    @classmethod
    def json_schema(cls):
        return {"oneOf": [
            {"type": "number"}, {"type": "null"},
            {"type": "object", "properties": {"status": {"enum": list(STATUSES)},
                                              "note": {"type": "string", "description": "why it is not known, a "
                                                       "footnote on the words written for it (SPEC §6.19)"}},
             "required": ["status"], "additionalProperties": False},
            {"type": "object", "properties": {"value": {"type": "number"},
                                              "certainty": {"enum": list(CERTAINTIES)},
                                              "complete": {"type": "boolean"},
                                              "note": {"type": "string", "description": "a footnote on this value "
                                                       "(SPEC §6.19): its mark on the value's text, the note under "
                                                       "the chart"}},
             "required": ["value"], "additionalProperties": False},
            {"type": "object", "properties": {"below": {"type": "number", "description": "a censored value: below "
                                                        "this detection limit (\"<1\"), not missing; drawn by "
                                                        "heatmaps and tables (SPEC §2.4)"},
                                              "note": {"type": "string"}},
             "required": ["below"], "additionalProperties": False},
            {"type": "object", "properties": {"above": {"type": "number", "description": "a censored value: above "
                                                        "this limit of the test's range (\">2420\")"},
                                              "note": {"type": "string"}},
             "required": ["above"], "additionalProperties": False},
            {"type": "string", "pattern": "^\\s*[<>]\\s*-?\\d+(\\.\\d+)?\\s*$",
             "description": "a censored value as a laboratory writes it: \"<1\", \">2420\""}]}


# ---- The spec ----------------------------------------------------------------

@dataclass(kw_only=True)
class Ref:
    """An element reference (SPEC §2.8)."""
    series: str = F(None, "a series id")
    category: str = F(None, "a category name (x value)")
    x: Any = F(None, "an x value, or [from, to] for a segment of a line")
    panel: str = F(None, "a panel of small multiples (phase 2)")


@dataclass(kw_only=True)
class Job:
    kind: Literal["answer", "make_aware", "persuade", "lookup", "explain", "other"] = F(doc="R0-07")
    text: str = F(doc="the question answered, the problem, the action …")


@dataclass(kw_only=True)
class Insight:
    important: str = F(None, "the specific thing the audience should notice (R0-10)")
    why_care: str = F(None)
    explain: str = F(None)
    action: str = F(None)
    targets: list[Ref] = F(factory=list, doc="the elements the insight names (highlighted by default)")
    explain_anchor: Ref = F(None)


@dataclass(kw_only=True)
class Title:
    text: Text = F(doc="the title (the generator never writes prose; R2-12, R5-27)")
    kind: Literal["insight", "question", "descriptive"] = F("descriptive", "13-15 §5.2")
    descriptive: Text = F(None, "the descriptive line under an insight or question title: what, by what, "
                                "period with year (D-5 (a)); set in the label style")
    link: Ref = F(None, "a title connector (phase 2)")


@dataclass(kw_only=True)
class Unit:
    prefix: str = F(None, 'before the number: "$"')
    suffix: str = F(None, 'after the number: "%", "°C", " lbs." (a leading space is part of it)')
    noun: str = F(None, 'runs on after the number: "successful transactions per month"')
    what: str = F(None, 'a description, always in parentheses after the number: "weight of product" gives '
                        '"0.25 lbs. (weight of product)" (D-20)')
    given_in: Literal["thousands", "millions", "billions", "trillions"] = F(
        None, "only when the numbers in the data are already written in thousands, millions … (a source headed "
              "\"£000\" or \"$ million\"): \"thousands\" means that 1840 in the data stands for 1,840,000. Never a "
              "request to scale the labels: the generator hides digits by itself (G6.11), and scale.magnitude "
              "overrides that. Values given in thousands that still reach five integer digits are reported "
              "(R5-11), as a likely misreading")
    magnitude: int = F(0, "the older spelling of given_in, as a power of ten: 3 for thousands, 6 for millions, 9 "
                          "for billions (the numbers in the data are already in that unit); prefer given_in")
    fallback: Text = F(None, "the unit line's text when the phrase cannot sit inline (6.4)")
    scale_title: Text = F(None, 'the title under a horizontal scale, in place of the one derived from what or '
                                'measure.name and the unit ("Donations ($K)")')

    def __post_init__(self):
        # given_in is the unit the data are written in, in words; the code reads it as magnitude,
        # the power of ten (check_structure refuses the two when they disagree)
        if self.given_in in GIVEN_IN and not self.magnitude:
            self.magnitude = GIVEN_IN[self.given_in]


GIVEN_IN = {"thousands": 3, "millions": 6, "billions": 9, "trillions": 12}


@dataclass(kw_only=True)
class Measure:
    name: str = F(None, "what is measured, for reports and legends")
    kind: Literal["count", "amount", "percent", "score", "quantity", "change", "index", "rank"] = F(
        "count", "percent values in percent units (45 for 45 %)")
    unit: Unit = F(factory=Unit)
    zero: Literal["meaningful", "arbitrary"] = F(None, "02 D2 Q2: is zero 'none of it' (a ratio scale)")
    min: Num = F(None, "theoretical minimum (R2-26, R2-28)")
    max: Num = F(None, "theoretical maximum (R2-28)")
    persistence: Literal["summary", "state", "event"] = F(None, "time only (R6-8–R6-12)")
    cumulative: bool = F(False, "the unit phrase must say so (R2-11)")
    additive: bool = F(None, "time (phase 3): the values are totals over their periods (deaths per decade, sales "
                             "per week), so adjacent periods' values add up to the longer period's; unequal bins "
                             "are re-aggregated by adding only when this is true (R2-43, G2.41)")
    good: Literal["higher", "lower"] = F(None, "good direction (R3-17, R15-11)")
    dividing_point: Any = F(None, "diverging midpoint, a number or {comparison: id} (R3-13)")
    precision: Num = F(None, "the finest difference the message needs (R5-9)")
    id: str = F(None, "in data.measures: the id its series name in series.measure (a merged chart, R2-16)")
    heading: Text = F(None, "a merged chart's panel heading: a short name of the measure, or the question the "
                            "panel answers (G2.32, G7.6, RP5-6); default the measure's name")


@dataclass(kw_only=True)
class Event:
    """A date marked on a time axis (SPEC §6.13.12): something that happened outside the data, which the
    message refers to (a storage tank finished, a competitor opened), drawn as a thin line across the plot
    at its date with its label."""
    at: str = F(doc="its date (\"2025-06-15\") or instant (\"2025-06-15T09:30\"); a date alone is the start of "
                    "its day, or its middle where the events of the data are dated to the day (thin bars)")
    label: Text = F(doc="the words beside it, \"Storage tank finished\" (the generator never writes prose)")


@dataclass(kw_only=True)
class XDim:
    kind: Literal["nominal", "ordinal", "time"] = F(doc="required: the order of categories cannot be inferred "
                                                        "(R2-01, R2-02)")
    values: list[str] = F(factory=list, doc="categories, or period strings on the grid")
    grid: Literal["year", "quarter", "month", "week", "day", "hour", "bin", "none"] = F(
        None, "required for time: regular intervals (R6-3), or none; bin (phase 3): ranges of years, \"1960–1969\" "
              "(or 1960-1969), contiguous, re-aggregated to equal lengths or refused (R2-43)")
    week_start: Literal["auto", "monday", "sunday"] = F(
        "auto", "a week grid's first day (phase 3): auto is Sunday for a Sunday-first locale (audience.locale "
                "en-US, en-CA, ja-JP …, the book's weeks), else Monday (ISO 8601); a week is written as its first "
                "day, \"2022-06-19\", or as an ISO week, \"2022-W25\", only when weeks start on Monday")
    title: Text = F(None, "categorical axis title; none when the names say what they are (R2-03)")
    roles: dict[str, Literal["all_others", "total", "average", "missing"]] = F(
        factory=dict, doc="R3-16; 02 D1: the special categories, each in its reserved grey; missing for a category "
                          "that stands for answers or items not known (\"No answer\", \"Not known\"): the missing "
                          "grey, placed last after all_others (G0.18, G2.1, G4.15)")
    levels: Any = F("auto", "time axis rows (6.3)")
    names: dict[str, Text] = F(factory=dict, doc="the words for a value that is a code, where the chart names it "
                                                  "(phase 2): an arrow chart's key, {\"2022-Q1\": \"Before blitz\"}")
    groups: dict[str, str] = F(factory=dict, doc="the group each category belongs to (phase 3): a treemap's top "
                                                 "level, coloured by group and labelled (R7-16); a pie's slices of "
                                                 "a group together, its first part in the group's colour and the "
                                                 "next in a tint of it (07 #10, separated overlaps)")
    order: Literal["auto", "fixed"] = F(
        "auto", "phase 3 (owner, 1 Oct 2026): fixed when the categories' given order must be kept (a table "
                "shared by several charts ordered differently): bars, stacks and the table under them stand in "
                "the order given rather than sorted by value (a guided decision); auto lets the chart sort "
                "nominal categories (R2-01)")
    events: list[Event] = F(factory=list, doc="time: dates marked on the axis, each with its label (the tank "
                                              "finished, a competitor's opening; SPEC §6.13.12)")


@dataclass(kw_only=True)
class Series:
    id: str = F(doc="referenced by highlight, comments, targets")
    name: Text = F(None, "end label or legend text")
    values: list[Value] = F(None, "aligned with x.values")
    points: list[list[Any]] = F(None, "[[time, value]] when x.grid is none")
    role: Literal["regular", "average", "total", "all_others", "reference", "target", "missing"] = F(
        "regular", "special roles take the reserved greys (R3-16, G4.15); missing: a series that stands for answers "
                   "or items not known (\"No answer\" as a part of a stack), in the missing grey, last")
    certainty: Literal["actual", "estimate", "projection", "target"] = F("actual")
    slot: int = F(None, "fixed palette slot (R3-24)")
    keys: dict[str, str] = F(factory=dict, doc="small multiples: the series' value of each of data.dims "
                                               "({\"method\": \"Cash\", \"region\": \"West\"}; SPEC §6.10)")
    measure: str = F(None, "a merged chart: the id of its measure in data.measures (R2-16)")


@dataclass(kw_only=True)
class Parts:
    meaningful_total: bool = F(None, "R7-02")
    all_parts: bool = F(None, "R7-03")
    overlaps: list[list[str]] = F(factory=list, doc="[[part, parent]] (R7-05)")
    overlap: bool = F(None, "phase 3: whether any part contains or overlaps another (R7-05, G0.14); true also when "
                            "overlaps lists a pair; neither given: refused before a pie, stacked chart, treemap or "
                            "waterfall, like meaningful_total and all_parts (D-1; owner, 1 Oct 2026)")
    total: Num = F(None)
    kind: Literal["nominal", "ordinal"] = F(
        "nominal", "phase 3: the parts' own order, when they are series (several totals, over time): ordinal parts "
                   "(risk levels) are stacked in their order, the first at the base (R2-02); nominal parts are "
                   "stacked by the message, then by size (G7.31, D-26)")
    groups: dict[str, str] = F(factory=dict, doc="phase 3: the group each part belongs to, when the parts are series "
                                                 "(several totals, over time; {series id: group}): a group's parts "
                                                 "stack together in shades of the group's hue, the darkest in the "
                                                 "middle, named by a grouped legend (G4.9, G5.12, D-49; 03-04 #7)")
    order: Literal["auto", "fixed"] = F(
        "auto", "phase 3 (owner, 1 Oct 2026): fixed when the data's given order must be kept (a table shared by "
                "several charts ordered differently): the parts, as series or as the categories of one total, and "
                "the totals they make keep the order given in every stack and band, the first at the base, "
                "whatever their size or the message; auto lets the chart order them (largest first, the parts the "
                "message names at the base, D-26)")


@dataclass(kw_only=True)
class Comparison:
    """A comparison value (R15-7–R15-9, G0.19, G8.12; phase 2, SPEC §6.13): one value (a
    reference line), or one per category or period, aligned with x.values (target marks on
    bars, a reference series on a line chart)."""
    id: str = F()
    kind: Literal["target", "budget", "tolerance", "forecast", "average", "previous_period", "peer"] = F()
    values: list[Value] = F(factory=list, doc="one value, or one per x value; empty with compute")
    label: Text = F(None, "its label (\"Target\", \"Industry average\"); default the kind's name")
    valid: bool = F(True, "false for a doubtful comparator (another industry, years-old figures): it is not "
                          "drawn (R15-9)")
    reason: str = F(None, "where it comes from, or why it is doubtful (reported)")
    compute: Literal["mean"] = F(None, "an average of the plotted data, computed (the values left empty)")
    series: str = F(None, "the series it compares (default: the first)")
    render: Literal["auto", "mark", "line", "series", "none"] = F(
        "auto", "guided (G8.13, G7.57, D-21): auto makes one value per category a reference series, which the bar "
                "layout draws as flat marks across the bars (R11-4), and a line chart's a reference series; one "
                "value is a line across the plot (the message layer); none keeps it for a transform or the "
                "good/bad division only")
    unit: Unit = F(None, "the unit its values are given in, when it is not the measure's (targets in kWh "
                         "against use in MWh): a unit of another prefix or suffix is refused unless factor "
                         "converts it (R2-13); one of another magnitude only is converted (£K against £M); "
                         "none: the measure's own")
    factor: Num = F(None, "the number each value is multiplied by to give the measure's unit (0.001 from kWh to "
                          "MWh), with unit; reported")


@dataclass(kw_only=True)
class Dim:
    """A breakdown variable besides x (small multiples, SPEC §6.10): its values become the
    panels' rows or columns, or the series inside each panel."""
    id: str = F(doc="named in series.keys and panels.arrangement; not \"measure\" (reserved)")
    name: Text = F(None, "what the variable is (\"Payment method\"), for reports")
    kind: Literal["nominal", "ordinal"] = F("nominal", "ordinal values keep their order (R2-02)")
    values: list[str] = F(factory=list, doc="its values, in their order: the order of rows, columns or series")


@dataclass(kw_only=True)
class Data:
    x: XDim = F()
    measure: Measure = F(factory=Measure)
    series: list[Series] = F()
    as_of: str = F(None, "a date: periods ending after it are incomplete (R2-42)")
    parts: Parts = F(None, "required by part-to-whole types")
    comparisons: list[Comparison] = F(factory=list, doc="phase 2")
    dims: list[Dim] = F(factory=list, doc="small multiples: the breakdown variables besides x, each series "
                                         "keyed by its values (series.keys; SPEC §6.10)")
    measures: list[Measure] = F(factory=list, doc="a merged chart: several measures, each with its id, its "
                                                 "series naming it (series.measure); one scale each (R2-13, "
                                                 "R2-16)")
    actions: dict[str, Literal["good_strong", "good", "bad", "bad_strong", "none"]] = F(
        factory=dict, doc="action dots (phase 3; R12-4, G7.58): the action each category's value warrants, "
                          "supplied with the data since the thresholds are the maker's (D-1): good_strong and good "
                          "a strong and a light green dot, bad and bad_strong a light and a strong red one, none "
                          "(or absent) no dot")


@dataclass(kw_only=True)
class Facts:
    message: str = F(None, "time: pattern (default), periods, rank, individual_values; categories: compare …; "
                           "phase 3 (the full chooser, SPEC §3.2): parts of a total (07 §4.0) "
                           "compare_parts_precisely, fraction_of_total, subtotal_amount, subtotal_fraction, "
                           "cumulative_subtotals, compare_totals, compare_parts_across_totals, "
                           "fractions_within_each_total; a table alone lookup_only, export_only, not_comparable "
                           "(R10-11); action_needed (action dots, R12-4); stage_conversion (the funnel's "
                           "replacement, R12-5); profiles, items compared across attributes (a radar's data: a "
                           "heatmap table, R12-6)")
    small_differences_matter: bool = F(False, "narrow-range inset (R2-23, R9-6)")
    small_values_matter: bool = F(False, "outlier inset (R2-20)")
    distance_to_zero_irrelevant: bool = F(False, "truncation allowed for non-length marks (R2-24)")
    exact_values_needed: bool = F(False, "table with the graph (phase 2)")
    location_matters: bool = F(False, "where the values are (regions' sizes, neighbours, clusters) matters more to "
                                      "the message than reading and comparing them: a map (R8-2, R8-3; maps are "
                                      "phase 4)")
    ramp_focus: Literal["extremes", "pattern"] = F(
        None, "a heatmap of skewed values (phase 3; G4.29, G4.30, G4.33): whether its few extreme values are the "
              "message (a linear cut) or the pattern among the many (logarithmic steps over two orders of "
              "magnitude, else natural breaks); asked only when the values are skewed; default pattern, with an "
              "info")
    total_matters: bool = F(None, "a total over time (phase 3, 07 §4.3): the total's change must be visible (a "
                                  "total panel, absolute stacks); default false, with an info")
    focus: Literal["pattern", "periods"] = F(None, "a total over time (phase 3): the overall pattern of change "
                                                   "(areas) or particular periods (bars; R7-28); default periods "
                                                   "when message is periods, else pattern")
    incomplete_value_needed: bool = F(False, "the value of an incomplete first or last period must be shown: "
                                             "drawn apart with an “As of …” note; otherwise it is left out "
                                             "(R2-42, G2.40, 02 D10; phase 2)")
    compare: list[str] = F(factory=list, doc="small multiples: the dims whose values the insight compares "
                                             "directly, drawn as series in one panel (RP3-4, G1.19)")
    compare_shapes: bool = F(False, "small multiples: the message compares the shapes of series of very "
                                    "different sizes, not their sizes: a scale per panel (R6-37, G7.48)")


@dataclass(kw_only=True)
class Audience:
    data_savvy: bool = F(False, "R0-03, R6-41")
    knows_scale: bool = F(False, "02 D2 Q7 (not used under D-10 (a))")
    cvd: Literal["assume", "no"] = F("assume", "R3-19")
    bw_risk: bool = F(False, "R6-30")
    pie_averse: bool = F(False, "R7-40")
    locale: str = F("en-GB", "separators (R5-15)")
    date_format: str = F(None, "R5-19")
    abbreviations: dict[str, str] = F(None, 'the audience\'s own, {"thousand": "K", "million": "M"} (R5-13)')


@dataclass(kw_only=True)
class ChartOpts:
    type: Literal["auto", "bar", "line", "dot", "step", "thin_bar", "arrow", "bumps",
                  "pie", "donut", "treemap", "wrapped_bar",
                  # phase 3, totals stacked (layout/bars.py, waterfall.py, areas.py): stacked bars, absolute
                  # or 100 %, a waterfall, stacked areas, lines with a total panel
                  "stacked_bar", "stacked_bar_100", "waterfall", "stacked_area", "stacked_area_100", "line_total",
                  # phase 3, the chooser: a slope chart (its rare case, R6-16), and requests on the
                  # avoid-list, refused with their replacements (G1.61; choose.AVOIDED)
                  "slope", "dual_axis", "radar", "funnel", "word_cloud", "packed_bubbles", "bullet",
                  "choropleth", "3d", "smoothed_line",
                  # phase 3, tables and heatmaps (layout/table.py, heatmap.py, funnel.py): a table alone,
                  # a heatmap, action dots, the funnel's replacement
                  "table", "heatmap", "action_dots", "funnel_bars"] = F(
        "auto", "later types in §7; step, thin_bar, arrow and bumps: phase 2 (time); pie (several series: one pie "
                "per total), treemap, wrapped_bar: phase 3, a single total's breakdown (layout/pie.py, treemap.py, "
                "wrapped.py); donut only through an override of R7-39")
    orientation: Literal["auto", "vertical", "horizontal"] = F("auto")
    sort: Literal["auto", "desc", "asc", "given"] = F("auto", "02 D1")
    sort_by: str = F(None, "a series id (default: the series the insight names, else the first; D-46)")
    labelling: Literal["auto", "direct", "legend"] = F("auto")
    value_labels: Literal["auto", "on", "off"] = F("auto", "R2-32 (phase 2)")
    gridlines: bool = F(True, "R1-06: off only with an override")
    change_panels: Literal["auto", "both", "absolute", "relative"] = F(
        "auto", "an arrow chart's panels (R6-17, G1.47; phase 2): both, change in units and change in %; "
                "relative alone when facts.message is relative_change; absolute alone only through an override")
    encoding: Literal["auto", "sequential", "diverging", "categorical"] = F(
        "auto", "guided (phase 3; G4.5, G4.10, G4.11, D-37; notes 03-04 §5.6 `encoding`): the kind of ramp by "
                "which colour encodes a quantity (a heatmap, a shaded table), never its colours, which are the "
                "style's: auto is sequential, or diverging when data.measure.dividing_point gives the value the "
                "audience sees as dividing; diverging without one is refused (R3-13), categorical for a quantity "
                "too (R3-9, G4.14 [L]), except through an override")
    classing: Literal["auto", "linear", "natural", "quantile", "custom", "log"] = F(
        "auto", "guided (phase 3; G4.29–G4.33, SPEC §6.14): where a heatmap's colour steps are cut: auto is "
                "linear (equal steps on rounded values, G4.29) for fairly even values, and for skewed values (one "
                "of seven equal steps would hold more than half of them) linear when facts.ramp_focus is extremes; "
                "when it is pattern (the default, with an info), log (1–2–5 steps, zero its own, G4.33) for "
                "amounts on a ratio scale spanning at least two orders of magnitude (the 95th percentile of the "
                "positive values at least 100 times the 5th; owner, 1 Oct 2026), else natural (Jenks's breaks, "
                "rounded, G4.30); quantile (equal counts, G4.31) and custom (chart.breaks, G4.32) only as "
                "departures with a reason")
    breaks: list[Num] = F(None, "custom breaks (G4.32, with a reason): the values between a heatmap's steps; "
                                "for a diverging ramp, their distances from the dividing value")
    cells: Literal["auto", "strong", "muted"] = F(
        "auto", "guided (phase 3; owner, 1 Oct 2026): a coloured cell's strength where it carries its value: "
                "strong (the step itself, the value black or white, whichever reads better: a heatmap, whose "
                "colour is the message) or muted (the step paled, the value in the text grey: a table with "
                "colour hinting at sizes); auto is strong for a heatmap and muted for a shaded table "
                "(table.graphics heat)")


@dataclass(kw_only=True)
class ScaleOpts:
    zero: Literal["auto", "include", "truncate"] = F("auto")
    min: Num = F(None, "pins the lower limit (refused for bars unless 0)")
    max: Num = F(None, "pins the upper limit (refused past a theoretical maximum)")
    step: Num = F(None, "pins the step")
    steps: list[Num] = F(factory=lambda: [Decimal(1), Decimal(2), Decimal(5)], doc="step mantissas (R2-09)")
    allow_2_5: bool = F(False)
    target_intervals: int = F(5)
    precision_value: Num = F(None, "a value that must be a gridline (R2-33)")
    breathing: Num = F(Decimal("0.15"), "0.10–0.20 (R2-27, D-16)")
    magnitude: Any = F("auto", "auto | none | 3 | 6 | 9")
    magnitude_style: Literal["auto", "word_on_top", "suffix"] = F(
        "auto", "word_on_top (the default, D-3 (a)) or suffix; the top value in full is never produced (D-3 (c))")
    unit_position: Literal["auto", "inline", "above", "below"] = F("auto", "6.4")


@dataclass(kw_only=True)
class InsetOpts:
    kind: Literal["none", "auto", "outlier", "narrow_range"] = F(
        "auto", "guided (G2.7, G2.26, D-11): auto adds one when facts.small_values_matter or "
                "facts.small_differences_matter asks and the data call for it; none, or a kind the procedure "
                "would not give, is a guided finding")
    members: list[str] = F(None, "an outlier inset's categories (default: all but the outliers)")
    position: Literal["auto", "inside", "above", "below", "right"] = F(
        "auto", "guided (G2.29): an outlier inset over categories inside the plot; a narrow range above a "
                "vertical chart (below on request), beside horizontal bars")
    share_axis: Literal["auto", True, False] = F(
        "auto", "guided (G2.27, D-56): an inset aligned above or below its main chart drops the labels it would "
                "repeat when space is short (the figure, labelled twice, over --inset-share-height × its width); "
                "true or false against that is a guided finding")
    compact_main: bool = F(False, "the main chart's plot half as high under an inset above it (s. 0081)")


@dataclass(kw_only=True)
class Arrangement:
    """Which variable goes where in small multiples (SPEC §6.10): a dim id (or "measure", in a
    merged chart) for the rows and for the columns of panels, and those drawn as series inside
    each panel."""
    rows: str = F(None, "the dim whose values are the rows of panels, or none")
    columns: str = F(None, "the dim whose values are the columns of panels, or none")
    within: list[str] = F(factory=list, doc="the dims drawn as series inside each panel (RP3-4)")


@dataclass(kw_only=True)
class PanelOpts:
    """Small multiples and merged charts (SPEC §6.10; phase 2). Every option is guided."""
    arrangement: Union[Literal["auto"], Arrangement] = F(
        "auto", "guided (13-15 DP-1, G1.18, G1.19; D-38): auto puts the dims facts.compare names inside the "
                "panels and facets on the others; a merged chart's measures are its panels")
    scale: Literal["auto", "shared", "per_row", "independent"] = F(
        "auto", "guided (G7.48, R6-37, D-38): shared unless facts.compare_shapes; a merged chart's measures "
                "always have their own")
    series_labels: Literal["auto", "after_last", "first_panel", "legend"] = F(
        "auto", "guided (G5.15, RP3-9, D-38): each series labelled once, right of the last panel")


@dataclass(kw_only=True)
class TableOpts:
    """A table with the graph (SPEC §6.7.13; phase 2, series): how its rows name the series'
    colours, and whether the legend stays. Guided defaults by the owner's decision of 1 Oct 2026:
    each row keyed by its name then its square, and no legend."""
    row_key: Literal["auto", "none", "swatch", "swatch_beside", "swatch_after", "text", "pill", "halo"] = F(
        "auto", "guided (owner, 1 Oct 2026): auto keys the rows under several series by swatch_after, else none; "
                "none: plain row names (the book, s. 0307); swatch: a key square in its own column at the "
                "table's left; swatch_beside: the square just left of each name; swatch_after: each "
                "right-aligned name, then its square, the squares in one column on the plot's left edge; text: "
                "each name in its series' strong shade; pill: each name on a pale tint of its series; halo: a "
                "pale tint round the letters")
    legend: Literal["auto", "keep", "drop"] = F(
        "auto", "guided (owner, 1 Oct 2026): auto drops the legend above the plot when the table's rows are "
                "keyed (they are then the key); keep includes it")
    # phase 3: a table alone (chart.type table; SPEC §6.16; R10-12–R10-25, R10-9)
    names_title: Text = F(None, "a table alone: the heading over the series' names (\"Event\", \"Salesperson\"); "
                                "none: no heading there")
    stub: list["StubColumn"] = F(factory=list, doc="a table alone: text or date columns before the names, each "
                                                   "with one entry per series in their order (R10-19, R10-24)")
    graphics: Literal["none", "heat", "bars", "dots", "trend"] = F(
        "none", "a table alone, guided (R10-9, G1.16, G7.47): graphics built into a table whose audience refuses "
                "graphs: heat, the cells shaded by value, the body, the Total column and the totals row each on its "
                "own ramp; bars, a bar under each number from the cell's left edge; dots, a dot beside each "
                "number; bars and dots on one scale per column (graphics_columns); trend, a sparkline of each "
                "row's values in a Trend column before the Total")
    graphics_columns: list[str] = F(None, "the columns (x values) whose cells carry bars or dots; default: every "
                                          "column, a Total column too (s. 0309), never the totals row")
    stripes: bool = F(False, "zebra stripes (R10-16, G7.40 [L]): never; true only through an override, to "
                             "rebuild the book's rejected version")
    bold: list[Literal["header", "totals", "total_column"]] = F(
        factory=list, doc="bold rows or columns (R10-22, G7.43 [L]): never; only through an override, to rebuild "
                          "the book's rejected version")


@dataclass(kw_only=True)
class StubColumn:
    """A column of text or dates before a table's numbers (phase 3; SPEC §6.16): left-aligned
    (R10-19); dates written with every part of fixed width, 05-Jan-2021 (R10-24, R5-22)."""
    title: Text = F(None, "its heading, none for none")
    kind: Literal["text", "date"] = F("text", "date: ISO dates (2021-01-05), written 05-Jan-2021")
    values: list[str] = F(factory=list, doc="one per series, in their order; a totals row's entry may be \"\"")


@dataclass(kw_only=True)
class Highlight:
    targets: list[Ref] = F(factory=list)
    method: Literal["auto", "strong", "thickness", "bold_label", "border", "grey_others", "glow", "background",
                    "icon", "selective_label", "explode"] = F(
        "auto", "guided (G8.9, D-9): auto is the strong shade for bars and dots, thickness for lines; border, "
                "glow, background, icon and selective_label (value labels on the targets only) are drawn by the "
                "message layer (annotate.py, phase 2); a line's segment is {series, x: [from, to]}")
    dim_others: bool = F(False, "bars only: the others paled (07 §5.1)")
    emphasis: Literal["normal", "strong"] = F(
        "normal", "open: how strong the emphasis must be; a glow takes the complementary hue for a strong one "
                  "unless a contrast would confuse (phase 2)")
    glow: Literal["auto", "own", "complementary"] = F(
        "auto", "guided (a glow's hue, phase 2): own, a pale tint of the target's own colour; complementary, its "
                "hue turned 180° in OKLCH at the same lightness; auto: own for normal emphasis, complementary for "
                "strong unless the chart uses more than 3 regular colours, keys its colours in a legend, or has a "
                "colour within 30° of the complement")


@dataclass(kw_only=True)
class Comment:
    """The maker's own remark about the data (a callout, R5-5, R15-1, G6.3, G8.7; phase 2,
    SPEC §6.12): set apart from the labelling, placed in empty space, with a thin arrow to each
    anchor."""
    text: Text = F(doc="the words (the generator never writes prose)")
    anchors: list[Ref] = F(factory=list, doc="the elements it points at, one arrow each; none for a chart-wide "
                                             "remark")
    placement: Literal["auto", "inside", "right", "below"] = F(
        "auto", "auto: the empty space nearest its anchors, inside the plot or beside it, else a column at the "
                "right; inside, right and below force the region")
    answers: list[Literal["important", "why_care", "explain", "action"]] = F(
        factory=list, doc="which of the four questions it answers (R15-1): a comment answering important "
                          "states the insight, as an insight title does (G8.2, D-55)")
    arrow: bool = F(True, "an arrow to each anchor")


@dataclass(kw_only=True)
class NoteRef:
    """Where a note's mark is set (SPEC §6.19): one of the spec's own texts, a category's or a
    series' name, one value, every value of a status, a comment, or a table's stub column. The
    mark follows the text as drawn (the last line of a wrapped one); a name or an element written
    more than once is marked where it is first read."""
    element: Literal["title", "descriptive", "unit", "x_title", "names_title"] = F(
        None, "the title, the descriptive line, the unit phrase (wherever the scale writes it), the categorical "
              "axis title, a table's heading over the names")
    category: str = F(None, "a category's name where the chart writes it (a label, a table's column heading, a "
                            "slice's or tile's name); with series, that one value")
    series: str = F(None, "a series' name where the chart writes it (an end label, a legend entry, a table's row "
                          "name); with category, that one value: its value label, its table cell, the note in its "
                          "empty slot")
    status: Literal["missing", "unknown", "pending", "tbd", "error", "zero"] = F(
        None, "every value of that status, on the words written for it (labels.notes: an empty slot's note, a "
              "table cell's \"POA\"); with series or category, only that row's or column's")
    comment: int = F(None, "a comment, by its place in comments (0 the first)")
    stub: int = F(None, "a table's stub column, by its place in table.stub: its heading; with series, that row's "
                        "entry")


@dataclass(kw_only=True)
class Note:
    """A note under the chart (SPEC §6.19): a footnote, referred to by a mark on what it explains,
    or a plain note with no mark (a general remark, the data's source). Set in the note style, below
    everything else, wrapped to the chart's width; footnotes first, in the order of their marks."""
    text: Text = F(doc="the note's words, a sentence or two (\"POA: price on application.\"); the generator "
                       "never writes prose, nor the mark, which it numbers")
    on: list[NoteRef] = F(factory=list, doc="where its mark is set, one or more places sharing one mark; none: "
                                           "a plain note")


@dataclass(kw_only=True)
class LabelOpts:
    notes: dict[str, str] = F(factory=lambda: {
        "missing": "(missing)", "unknown": "(unknown)", "pending": "(pending)", "tbd": "(TBD)",
        "error": "(error)", "zero": "(zero)", "unnamed": "({n} parts too small to name)"})
    parts: Literal["auto", "value", "percent", "name"] = F(
        "auto", "open (phase 3): what labels a pie's slice (or a treemap's tile, where both fit) under its name, "
                "its value or its percentage of the total, formatted as a set (G7.27, D-51); name: the name alone; "
                "auto: the value on a pie, the name alone in a treemap's tile and on a pie of several totals")


@dataclass(kw_only=True)
class LayoutOpts:
    """Open choices for the particular chart, within limits (SPEC §1.4)."""
    width: Num = F(Decimal(368), "the figure width in points, 120–600 (the paper's text width is 368)")
    plot_aspect: Num = F(None, "plot height / plot width, 0.3–1.2; none: the style's")
    height: Num = F(None)


@dataclass(kw_only=True)
class Colours:
    """Which palette slot a series or a part takes, fixed by name across a set of charts (R3-24, G4.22;
    SPEC §2.10): the slot only, never a colour, which is the style's."""
    slots: dict[str, int] = F(factory=dict, doc="a name and its palette slot (1–16): a series by its name (in "
                                                "small multiples, by its values of the dims drawn inside the panels, "
                                                "\"Elstead Park\"), a pie's part by its category; a name the chart "
                                                "does not show is passed over, so that one map serves every chart of "
                                                "a document; a series' own slot comes first")
    registry: str = F(None, "the document's colour registry: a JSON file holding such a map ({\"Elstead Park\": 2, "
                            "…}), its path relative to the spec's file; the spec's own slots come first")


@dataclass(kw_only=True)
class Override:
    rule: str = F()
    reason: str = F()


@dataclass(kw_only=True)
class Exposure:
    """What a ratio divides by (R15-15): the size of each category or period."""
    values: list[Value] = F(doc="one per x value")
    noun: str = F(doc="what the values count, plural (\"work hours\", \"residents\")")


@dataclass(kw_only=True)
class Transform:
    """A derived measure, computed before the chart is chosen and laid out (R15-13–R15-17,
    R2-17, G8.15–G8.18; phase 2, transform.py): the unit phrase says what it is."""
    kind: Literal["deviation", "ratio", "index"] = F(
        doc="deviation: each value minus its comparison value (R15-14); ratio: each value per its exposure "
            "(R15-15); index: each series rebased at a common start (R15-16, R2-17; base 0: % change)")
    comparison: str = F(None, "deviation: the id of the comparison value in data.comparisons")
    relative: bool = F(False, "deviation: in % of the comparison value, not in the measure's unit")
    exposure: Exposure = F(None, "ratio: what each value is divided by")
    per: Num = F(Decimal(1), "ratio: per how many of the exposure (100000: per 100K work hours)")
    base: Num = F(Decimal(100), "index: the common value at the start (10000: the value of $10,000 invested; "
                                "100; 0: % change since the start)")
    at: str = F(None, "index: the x value of the common start (default the first)")
    name: str = F(None, "the derived measure's name (default derived)")
    unit: Unit = F(None, "the derived unit phrase in place of the one derived (\"above target\", \"(value of "
                         "$10,000 invested on Feb. 4)\")")


@dataclass(kw_only=True)
class Source:
    book: str = F(None, 'the book\'s page: "p. 49 (s. 0049)"')
    notes: str = F(None, '"00-01 F1"')
    shows: str = F(None, "what the example demonstrates")
    status: Literal["undrawn", "generated", "compared"] = F("generated")


@dataclass(kw_only=True)
class Spec:
    chartspec: Literal[1] = F(doc="schema version")
    id: str = F(doc="file stem, [a-z0-9-]+")
    purpose: Literal["communication", "analysis"] = F("communication", "R0-02")
    job: Job = F(None, "required for communication (R0-07)")
    insight: Insight = F(None, "recommended (R0-10, RP5-1)")
    title: Title = F(None, "required for communication")
    audience: Audience = F(factory=Audience)
    data: Data = F()
    facts: Facts = F(factory=Facts)
    chart: ChartOpts = F(factory=ChartOpts)
    scale: ScaleOpts = F(factory=ScaleOpts)
    inset: InsetOpts = F(factory=InsetOpts)
    panels: PanelOpts = F(factory=PanelOpts, doc="small multiples and merged charts (SPEC §6.10; phase 2)")
    table: TableOpts = F(factory=TableOpts, doc="a table with the graph: its rows' keys (SPEC §6.7.13; phase 2)")
    highlight: list[Highlight] = F(None)
    comments: list[Comment] = F(factory=list)
    notes: list[Note] = F(factory=list, doc="notes under the chart (SPEC §6.19): footnotes, each with the places "
                                           "its mark is set (on), and plain notes; not labels.notes, the words "
                                           "written for values not known. A value's own note is a footnote too")
    transform: Transform = F(None, "a derived measure: deviation, ratio or index (phase 2, transform.py)")
    labels: LabelOpts = F(factory=LabelOpts)
    layout: LayoutOpts = F(factory=LayoutOpts)
    colours: Colours = F(factory=Colours, doc="palette slots fixed by name, the same in every chart of a set "
                                              "(R3-24, G4.22): a map, or the document's registry file")
    overrides: list[Override] = F(factory=list, doc="R0-04: lets a refusal of a locked rule through, reported")
    reasons: dict[str, str] = F(factory=dict, doc="why a guided option departs from the procedure, keyed by "
                                                  "its path (\"chart.orientation\", \"highlight[0].dim_others\")")
    source: Source = F(factory=Source)


# ---- Loading -------------------------------------------------------------------

def _hints(cls):
    return get_type_hints(cls)


def _load(tp, data, path, errors):
    origin = get_origin(tp)
    if tp is Any:
        return data
    if origin in (Union, types.UnionType):
        args = [a for a in get_args(tp) if a is not type(None)]
        if data is None:
            return None
        if len(args) == 1:
            return _load(args[0], data, path, errors)
        for a in args:
            trial = []
            v = _load(a, data, path, trial)
            if not trial:
                return v
        errors.append((path, "matches none of the allowed forms"))
        return None
    if origin is Literal:
        if data not in get_args(tp):
            errors.append((path, f"{json.dumps(data)} is not one of {', '.join(json.dumps(a) for a in get_args(tp))}"))
        return data
    if origin is list:
        if not isinstance(data, list):
            errors.append((path, "expected a list"))
            return []
        (item,) = get_args(tp)
        return [_load(item, x, f"{path}[{i}]", errors) for i, x in enumerate(data)]
    if origin is dict:
        if not isinstance(data, dict):
            errors.append((path, "expected an object"))
            return {}
        _, item = get_args(tp)
        return {k: _load(item, v, f"{path}.{k}", errors) for k, v in data.items()}
    if hasattr(tp, "from_json"):                          # Text, Value: their own forms
        return tp.from_json(data, path, errors)
    if is_dataclass(tp):
        return _load_dataclass(tp, data, path, errors)
    if tp is Num:
        if isinstance(data, bool) or not isinstance(data, (int, float, Decimal)):
            errors.append((path, "expected a number"))
            return None
        return Decimal(str(data))
    if tp is bool:
        if not isinstance(data, bool):
            errors.append((path, "expected true or false"))
        return data
    if tp is int:
        if isinstance(data, bool) or not isinstance(data, int):
            errors.append((path, "expected an integer"))
        return data
    if tp is str:
        if not isinstance(data, str):
            errors.append((path, "expected a string"))
        return data
    return data


def _load_dataclass(cls, data, path, errors):
    if data is None:
        return None
    if not isinstance(data, dict):
        errors.append((path or "(spec)", "expected an object"))
        return None
    hints = _hints(cls)
    names = [f.name for f in fields(cls)]
    for k in data:
        if k not in names:
            errors.append((f"{path}.{k}" if path else k, f"unknown field; the fields are {', '.join(names)}"))
    kwargs = {}
    for f in fields(cls):
        p = f"{path}.{f.name}" if path else f.name
        if f.name in data and data[f.name] is None and f.default is None:
            kwargs[f.name] = None                        # null: the field's default, none
        elif f.name in data:
            kwargs[f.name] = _load(hints[f.name], data[f.name], p, errors)
        elif f.default is MISSING and f.default_factory is MISSING:
            errors.append((p, "required"))
    try:
        return cls(**kwargs)
    except TypeError as e:                               # a required field was missing
        errors.append((path or "(spec)", str(e)))
        return None


def load(data):
    """A Spec from its JSON value; raises SpecError listing every problem by path."""
    errors = []
    spec = _load_dataclass(Spec, data, "", errors)
    if spec is not None and spec.id is not None and not re.fullmatch(r"[a-z0-9-]+", spec.id or ""):
        errors.append(("id", "must match [a-z0-9-]+"))
    if errors:
        raise SpecError(errors)
    if spec is not None:
        spec._source = data           # the JSON value, for the checks the Rust engine makes (check_structure)
    return spec


def merge_patch(target, patch):
    """RFC 7386 JSON Merge Patch."""
    if not isinstance(patch, dict):
        return copy.deepcopy(patch)
    result = copy.deepcopy(target) if isinstance(target, dict) else {}
    for k, v in patch.items():
        if v is None:
            result.pop(k, None)
        else:
            result[k] = merge_patch(result.get(k), v)
    return result


def read_file(path):
    """(base spec JSON, base expect, {variant name: (spec JSON, expect)}) of a spec file."""
    # Preserve JSON decimal lexemes until the native protocol consumes them.
    # Python floats would round long values before Rust ever sees the spec.
    raw = json.loads(Path(path).read_text(encoding="utf-8"), parse_float=Decimal)
    if "base" in raw:
        base = raw["base"]
        variants = {}
        for v in raw.get("variants", []):
            variants[v["name"]] = (merge_patch(base, v.get("patch", {})), v.get("expect", {}))
        return base, raw.get("expect", {}), variants
    return raw, {}, {}


# ---- Schema ---------------------------------------------------------------------

def _schema(tp, defs):
    origin = get_origin(tp)
    if tp is Any:
        return {}
    if origin in (Union, types.UnionType):
        args = [a for a in get_args(tp) if a is not type(None)]
        inner = [_schema(a, defs) for a in args]
        return {"anyOf": inner + [{"type": "null"}]}
    if origin is Literal:
        return {"enum": list(get_args(tp))}
    if origin is list:
        return {"type": "array", "items": _schema(get_args(tp)[0], defs)}
    if origin is dict:
        return {"type": "object", "additionalProperties": _schema(get_args(tp)[1], defs)}
    if hasattr(tp, "json_schema"):
        return tp.json_schema()
    if is_dataclass(tp):
        name = tp.__name__
        if name not in defs:
            defs[name] = {}
            hints = _hints(tp)
            props, required = {}, []
            for f in fields(tp):
                s = _schema(hints[f.name], defs)
                doc = f.metadata.get("doc")
                if doc:
                    s = {**s, "description": doc}
                if f.default is not MISSING and f.default is not None:
                    s = {**s, "default": _jsonable(f.default)}
                props[f.name] = s
                if f.default is MISSING and f.default_factory is MISSING:
                    required.append(f.name)
            defs[name] = {"type": "object", "properties": props, "required": required,
                          "additionalProperties": False}
            if tp.__doc__:
                defs[name]["description"] = " ".join(tp.__doc__.split())
        return {"$ref": f"#/$defs/{name}"}
    return {Num: {"type": "number"}, bool: {"type": "boolean"}, int: {"type": "integer"},
            str: {"type": "string"}}.get(tp, {})


def _jsonable(v):
    if isinstance(v, Decimal):
        return float(v) if v != v.to_integral_value() else int(v)
    return v


def schema():
    """The JSON Schema of a spec (chartspec 1)."""
    defs = {}
    root = _schema(Spec, defs)
    return {"$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "chartspec-1.json", "title": "chartgen chart spec, version 1",
            **defs["Spec"], "$defs": {k: v for k, v in defs.items() if k != "Spec"}}


# ---- Structural checks (SPEC §3.1 step 2) ---------------------------------------------

_PERIOD = {
    "year": r"\d{4}", "quarter": r"\d{4}-Q[1-4]", "month": r"\d{4}-(0[1-9]|1[0-2])",
    "week": r"\d{4}-W(0[1-9]|[1-4]\d|5[0-3])", "day": r"\d{4}-\d{2}-\d{2}", "hour": r"\d{4}-\d{2}-\d{2}T\d{2}",
}
_PERIOD["week"] = f"{_PERIOD['week']}|{_PERIOD['day']}"   # phase 3: a week by its first day too (x.week_start)
_PERIOD["bin"] = r"\d{4}[–-]\d{4}"                        # phase 3: a range of years (R2-43; timeaxis.check_bins)


def _engine_findings(spec, prefixes):
    """The Rust engine's structural findings (CG-4) under these paths. The checks of time points, week
    and bin grids, marked dates and event references are the engine's (chartgen-data); the original made
    them in timeaxis.py and layout/events.py, which are not part of this package."""
    source = getattr(spec, "_source", None)
    if source is None:
        raise ValueError("check_structure needs a spec made by load(), which keeps its JSON value")
    from . import dispatch
    report = dispatch({"operation": "choose", "spec": source, "variant": None, "expect": {}})["value"]
    return [(f.get("path") or "", f["message"], f["rule"]) for f in report.get("findings", [])
            if f["severity"] == "error" and f["rule"] == "CG-4" and (f.get("path") or "").startswith(prefixes)]


def check_structure(spec):
    """Problems a well-formed spec cannot have, as (path, message, rule)."""
    out = []
    engine = []                                     # path prefixes the engine checks
    x, series = spec.data.x, spec.data.series
    n = len(x.values)
    if len(set(x.values)) != n:
        out.append(("data.x.values", "category names must be distinct", "CG-4"))
    ids = [s.id for s in series]
    if len(set(ids)) != len(ids):
        out.append(("data.series", "series ids must be distinct", "CG-4"))
    for name, k in (spec.colours.slots or {}).items():     # palette slots fixed by name (R3-24, SPEC §2.10)
        if not isinstance(k, int) or isinstance(k, bool) or not 1 <= k <= 16:
            out.append((f"colours.slots.{name}", "a palette slot, 1–16", "CG-4"))
    for i, s in enumerate(series):
        if s.slot is not None and not 1 <= s.slot <= 16:
            out.append((f"data.series[{i}].slot", "a palette slot, 1–16", "CG-4"))
    if not series:
        out.append(("data.series", "at least one series is needed", "CG-4"))
    for i, s in enumerate(series):
        if x.grid == "none":
            if s.points is None:
                out.append((f"data.series[{i}]", "irregular time (grid none) takes points, not values", "CG-4"))
            else:                                   # phase 2: [[time, value]] in order
                engine.append(f"data.series[{i}].points")
        elif s.values is None:
            out.append((f"data.series[{i}].values", "required", "CG-4"))
        elif len(s.values) != n:
            out.append((f"data.series[{i}].values", f"{len(s.values)} values for {n} categories", "CG-4"))
    if x.kind == "time":
        if x.grid is None:
            out.append(("data.x.grid", "required for time: regular intervals, or none (R6-3)", "CG-4"))
        elif x.grid != "none":
            pat = _PERIOD[x.grid]
            bad = [v for v in x.values if not re.fullmatch(pat, v)]
            if bad:
                out.append(("data.x.values", f"{bad[0]!r} is not a {x.grid} period", "CG-4"))
            elif x.values != sorted(x.values) or len(set(x.values)) != n:
                out.append(("data.x.values", "periods must be strictly increasing", "CG-4"))
            elif x.grid in ("week", "bin"):            # phase 3: weeks on their first day, contiguous bins
                engine.append("data.x.values")
    for c in x.roles:
        if c not in x.values:
            out.append((f"data.x.roles.{c}", "not a category", "CG-4"))
    for c in x.groups:                          # phase 3: a treemap's or a pie's groups
        if c not in x.values:
            out.append((f"data.x.groups.{c}", "not a category", "CG-4"))
    for c in (spec.data.parts.groups if spec.data.parts is not None else {}):   # phase 3: a stack's groups of parts
        if c not in ids:
            out.append((f"data.parts.groups.{c}", "not a series", "CG-4"))
    for c in spec.data.actions:                 # phase 3: action dots' statuses (R12-4)
        if c not in x.values:
            out.append((f"data.actions.{c}", "not a category", "CG-4"))
    for j, col in enumerate(spec.table.stub):   # phase 3: a table's text and date columns
        if len(col.values) != len(series):
            out.append((f"table.stub[{j}].values", f"{len(col.values)} entries for {len(series)} rows (series)",
                        "CG-4"))
        elif col.kind == "date":
            for i, v in enumerate(col.values):
                if v:
                    try:
                        date.fromisoformat(v)
                    except ValueError:
                        out.append((f"table.stub[{j}].values[{i}]", "not a date (YYYY-MM-DD)", "CG-4"))
    for c in spec.table.graphics_columns or []:
        if c not in x.values:
            out.append(("table.graphics_columns", f"no column {c!r}", "CG-4"))
    if spec.data.as_of:
        try:
            date.fromisoformat(spec.data.as_of)
        except ValueError:
            out.append(("data.as_of", "not a date (YYYY-MM-DD)", "CG-4"))
    units = [("data.measure.unit", spec.data.measure.unit)] + \
        [(f"data.measures[{i}].unit", m.unit) for i, m in enumerate(spec.data.measures)]
    for p, u in units:                          # the unit the data are written in (given_in, or magnitude)
        if u is None:
            continue
        if u.magnitude not in (0, 3, 6, 9, 12):
            out.append((f"{p}.magnitude", "3, 6, 9 or 12 (the data already in thousands, millions …); "
                                          "better given_in", "CG-4"))
        elif u.given_in is not None and u.magnitude != GIVEN_IN[u.given_in]:
            out.append((f"{p}.given_in", f"given_in {u.given_in} and magnitude {u.magnitude} disagree: give "
                                         "given_in alone", "CG-4"))
    if x.events:                                 # dates marked on a time axis (SPEC §6.13.12)
        engine.append("data.x.events")

    def ref_ok(r, path):
        if r.series is not None and r.series not in ids:
            out.append((path, f"no series {r.series!r}", "CG-4"))
        cat = r.category if r.category is not None else r.x if isinstance(r.x, str) else None
        if cat is not None and x.grid == "none" and x.kind == "time":
            # an irregular series' event, by its time as given or its day (SPEC §6.13.12)
            engine.append(path)
        elif cat is not None and cat not in x.values:
            out.append((path, f"no category {cat!r}", "CG-4"))
    if spec.insight:
        for i, r in enumerate(spec.insight.targets):
            ref_ok(r, f"insight.targets[{i}]")
    for j, h in enumerate(spec.highlight or []):
        for i, r in enumerate(h.targets):
            ref_ok(r, f"highlight[{j}].targets[{i}]")
    for j, c in enumerate(spec.comments or []):                  # phase 2: comments and the title
        for i, r in enumerate(c.anchors):                          # connector (annotate.py)
            ref_ok(r, f"comments[{j}].anchors[{i}]")
    if spec.title and spec.title.link:
        ref_ok(spec.title.link, "title.link")
    out += _check_notes(spec, ids)                  # notes under the chart and their marks (§6.19)
    if spec.chart.sort_by is not None and spec.chart.sort_by not in ids:
        out.append(("chart.sort_by", f"no series {spec.chart.sort_by!r}", "CG-4"))
    lay = spec.layout
    if not Decimal(120) <= lay.width <= Decimal(600):
        out.append(("layout.width", "an open choice within 120–600 pt", "CG-4"))
    if lay.plot_aspect is not None and not Decimal("0.3") <= lay.plot_aspect <= Decimal("1.2"):
        out.append(("layout.plot_aspect", "an open choice within 0.3–1.2", "CG-4"))
    if engine:
        out += _engine_findings(spec, tuple(engine))
    return out


def _check_notes(spec, ids):
    """Notes under the chart (SPEC §6.19): words in every note; each place a mark is set names one
    thing the spec has: an element, a comment, a stub column (with a series: its entry), a status
    (narrowed by a series or a category), or a series and a category (a name, or with both a value)."""
    out = []
    x = spec.data.x
    for j, n in enumerate(spec.notes or []):
        if n.text is None or not str(n.text).strip():
            out.append((f"notes[{j}].text", "a note needs its words", "CG-4"))
        for i, r in enumerate(n.on):
            p = f"notes[{j}].on[{i}]"
            kinds = [k for k in ("element", "comment", "stub", "status") if getattr(r, k) is not None]
            named = r.series is not None or r.category is not None
            if not kinds and not named:
                out.append((p, "where the mark goes: element, category, series, status, comment or stub", "CG-4"))
                continue
            if len(kinds) > 1 or (kinds and kinds[0] in ("element", "comment") and named) \
                    or (kinds == ["stub"] and r.category is not None):
                out.append((p, "one place per entry: an element, a comment, a stub column (with its series), a "
                               "status (with a series or a category), or a series and a category", "CG-4"))
                continue
            if r.series is not None and r.series not in ids:
                out.append((p + ".series", f"no series {r.series!r}", "CG-4"))
            if r.category is not None and r.category not in x.values:
                out.append((p + ".category", f"no category {r.category!r}", "CG-4"))
            if r.comment is not None and not 0 <= r.comment < len(spec.comments or []):
                out.append((p + ".comment", f"no comment {r.comment} (comments are counted from 0)", "CG-4"))
            if r.stub is not None and not 0 <= r.stub < len(spec.table.stub):
                out.append((p + ".stub", f"no stub column {r.stub} (table.stub, counted from 0)", "CG-4"))
            if r.element is not None:
                have = {"title": spec.title is not None and spec.title.text is not None,
                        "descriptive": spec.title is not None and spec.title.descriptive is not None,
                        "unit": True, "x_title": x.title is not None,
                        "names_title": spec.table.names_title is not None}[r.element]
                if not have:
                    out.append((p + ".element", f"the spec has no {r.element.replace('_', ' ')}", "CG-4"))
    return out
