"""Chartgen command-line host. Decisions, layouts and schema validation use Rust."""
import argparse
import json
from pathlib import Path
import sys
from . import dispatch, render
from .rules import Finding
from .spec import read_file
from .style import Style

LEVEL_ORDER = ("locked", "style", "guided", "open")
SEVERITY_ORDER = {"error": 0, "warn": 1, "overridden": 2, "info": 3}

def write(path,text):
    path=Path(path);path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text,encoding="utf-8")

def _line(name,report):
    worst = [f for f in report.findings if f.severity in ("error", "warn", "overridden")]
    rules = ", ".join(sorted({f"{f.rule}{'!' if f.severity == 'error' else ''}" for f in worst}))
    ok = "ok  " if report.expect_met else "FAIL"
    extra = ""
    if not report.expect_met:
        extra = f"  missing {report.summary.get('expect_missing')} unexpected {report.summary.get('unexpected_errors')}"
    return f"{ok} {name:48s} {report.status:11s} {rules}{extra}"

def _finding_lines(f):
    guide = ", ".join(f.get("guide") or []) or "–"
    lines = [f"    {f['severity']:10s} {f['rule']:7s} {guide:18s} {f['message']}"]
    pad = " " * 41
    for key, label in (("path", "field"), ("recommended", "the procedure gives"),
                       ("supply", "supply"), ("reason", "reason")):
        if f.get(key):
            lines.append(f"{pad}{label}: {f[key]}")
    if f.get("elements"):
        elements = f["elements"]
        shown = ", ".join(elements[:6]) + (f" … and {len(elements) - 6} more" if len(elements) > 6 else "")
        lines.append(f"{pad}elements: {shown}")
    return lines

def _findings(findings):
    grouped = {}
    for finding in findings:
        grouped.setdefault(finding.get("level", "locked"), []).append(finding)
    for level in (*LEVEL_ORDER, *(key for key in grouped if key not in LEVEL_ORDER)):
        if level in grouped:
            print(f"  {level}")
            for finding in sorted(grouped[level],key=lambda f: SEVERITY_ORDER.get(f["severity"],9)):
                print("\n".join(_finding_lines(finding)))

def _counts(findings):
    counts = {}
    for finding in findings:
        counts[finding["severity"]] = counts.get(finding["severity"], 0) + 1
    names = {"error": "error", "warn": "warning", "overridden": "overridden", "info": "info"}
    return ", ".join(f"{counts[key]} {names[key]}{'s' if counts[key] != 1 and key in ('error', 'warn') else ''}"
                     for key in SEVERITY_ORDER if key in counts) or "no findings"

def selected(path,variant):
    base,expect,variants=read_file(path)
    if variant is None:return base,expect
    if variant not in variants:raise ValueError(f"{path}: no variant {variant!r} (has {', '.join(variants) or 'none'})")
    return variants[variant]

def render_one(args,path,variant,out,report_path):
    spec,expect=selected(path,variant)
    # The original names the source "specs/<file>" in the output's header comment.
    output,report=render(spec,args.style,variant,expect,source_name=f"specs/{Path(path).name}",base_dir=Path(path).parent,backend=args.backend)
    if output is None:
        Path(out).unlink(missing_ok=True)               # no stale drawing of a refused chart
    else:write(out,output)
    for suffix,copy in (report.copies or {}).items():
        write(Path(out).with_name(f"{Path(out).stem}.{suffix}{Path(out).suffix}"),copy)
    write(report_path,json.dumps(report.to_json(),ensure_ascii=False,indent=1)+"\n")
    print(_line(Path(path).stem+(f"--{variant}" if variant else ""),report))
    return 0 if report.expect_met else 1

def _choose_text(report, variant):
    verdict={"ready":"would be drawn (run render for the layout, its decisions and the lint)",
             "refused":"refused: a locked rule, or a fact the book says cannot be inferred",
             "invalid":"invalid: the spec is malformed",
             "unsupported":"not drawn by this stage of the generator"}.get(report["status"],report["status"])
    print(f"{report['chart']}{'--'+variant if variant else ''}: {verdict}")
    if report.get("path"):
        group=next((p.get("group") for p in reversed(report["path"]) if p.get("group")),None)
        print("\nThe path (G1.10, in D-12 (a)'s order; step, level, rule, guide rules)"+(f": group {group}" if group else ""))
        for step in report["path"]:
            guide=", ".join(step.get("guide") or []) or "–"
            print(f"    {step.get('step', '')} {step.get('name', ''):24s} {step.get('level', ''):8s} {step.get('rule', ''):7s} {guide:18s} {step.get('text', '')}")
    if report.get("decisions"):
        print("\nDecisions, in order (level, rule, guide rules)")
        for decision in report["decisions"]:
            guide=", ".join(decision.get("guide") or []) or "–"
            print(f"    {decision.get('level', ''):8s} {decision.get('rule', ''):7s} {guide:18s} {decision.get('text', '')}")
    rows=_choice(report)
    if rows:
        print("\nThe choice")
        for row in rows:
            print(f"    {row['part']:12s} {row['value']}  [{row['rule']}; {row['level']}]")
    if report.get("findings"):
        print("\nRefusals and findings, by level")
        _findings(report["findings"])
    if report.get("defaults"):
        print("\nFacts the spec does not give (the generator's default; what reads it)")
        for d in report["defaults"]:
            print(f"    {d['path']:36s} {d['default']:10s} {d['used_by']}")
    missing=_unreported(report)
    if missing:
        print(f"chartgen: the Rust engine does not yet report {', '.join(missing)} for choose",file=sys.stderr)

# ---- choose: the original's choice rows and defaulted facts (charts/chartgen/__main__.py) -----
# The engine reports the chooser's choice (type, orientation, sort, sort_by, the reason a type
# keeps the given order) and whether the values are parts of a total; these rows and facts are
# the original CLI's presentation of them and of the spec.

def _given(data, path):
    """Whether a dotted path is present (and not null) in a spec's JSON."""
    for key in path.split("."):
        if not isinstance(data, dict) or data.get(key) is None:
            return False
        data = data[key]
    return True


def _facts(spec, breakdown):
    """The facts the chooser and the scale read, where they apply: (path, default, what uses it)."""
    x, m = spec.data.x, spec.data.measure
    time = x.kind == "time"
    job = spec.job.kind if spec.job else None
    rows = []
    if time:
        rows.append(("data.x.grid", "required", "06 DP6-1: one value per regular interval, or irregular (R6-3)"))
        rows.append(("data.measure.persistence", "required" if x.grid == "none" else "summary",
                     "06 DP6-1: whether a value summarises, holds until the next, or is an event (R6-1, R6-8–R6-12)"))
    rows += [
        ("facts.message", "pattern" if time else "compare",
         "06 DP6-1: the pattern of change (a line) or the periods themselves (bars)" if time else
         "the chart type for categories (bars; parts of a total, 07 §4; a table alone, R10-11)"),
        ("data.measure.zero", "meaningful", "02 D2: whether the scale may leave out zero (R2-22, R2-24; for a score or "
         "quantity say which)"),
        ("data.measure.min", "none", "a theoretical minimum: the lower limit of dots and lines (R2-26), nothing "
         "below it (R2-28)"),
        ("data.measure.max", "100 for parts of a meaningful total" if m.kind == "percent" else "none",
         "a theoretical maximum: no scale past it (R2-28)"),
        ("facts.small_differences_matter", "false", "a narrow-range inset (R2-23, R9-6)"),
        ("facts.small_values_matter", "false", "an outlier inset (R2-20)"),
        ("facts.distance_to_zero_irrelevant", "false", "truncating the scale of lines and dots (R2-24, G2.8)"),
    ]
    if x.kind == "ordinal":
        rows.append(("data.x.title", "none", "the categorical axis title, needed when the names do not say what "
                     "they are (R2-03)"))
        rows.append(("audience.data_savvy", "false", "a line across ordered categories (R6-41)"))
    if job in ("answer", "persuade", "make_aware") or (spec.insight and spec.insight.important):
        rows.append(("insight.targets", "none", "what is highlighted by default (R15-5, G8.9)"))
    if len(spec.data.series) > 1:
        rows.append(("audience.cvd", "assume", "the colour-vision check of the colours in use (R3-19)"))
    rows += [("facts.exact_values_needed", "false", "step 1: a table aligned with the graph (R10-7, G1.14)"),
             ("facts.location_matters", "false", "step 3: a map, where location outweighs reading the values (R8-2; "
                                                "phase 4)")]
    if breakdown:
        rows += [("data.parts.meaningful_total", "required", "step 4: the first check before a pie, stacked chart, "
                                                             "treemap or waterfall (R7-02, D-1)"),
                 ("data.parts.all_parts", "required", "step 4: the second check (R7-03, D-1)"),
                 ("data.parts.overlap", "required", "step 4: the third check, false or the pairs in "
                                                    "data.parts.overlaps (R7-05, D-1, D-24; owner, 1 Oct 2026)"),
                 ("audience.pie_averse", "false", "step 4: a 100 % stacked bar instead of a pie (R7-40, D-39)")]
        if x.kind == "time":
            rows += [("facts.total_matters", "false", "step 4: a total panel, or absolute stacks (07 §4.3, R7-25)"),
                     ("facts.focus", "pattern", "step 4: stacked areas for the pattern, bars for periods (R7-28)")]
    if x.kind == "time" and x.grid == "week":
        rows.append(("data.x.week_start", "auto", "the weeks' first day: by audience.locale, Sunday for en-US, else "
                                                  "Monday (ISO)"))
    if x.kind == "time" and x.grid == "bin":
        rows.append(("data.measure.additive", "required", "unequal bins re-aggregated by adding (R2-43, D-1)"))
    return rows


def _choice_rows(spec, choice, report):
    """The chooser's result, each part with its rule and level."""
    x, opts = spec.data.x, spec.chart
    t = report.get("type") or choice["type"]
    rows = [("type", t, "G1.10 (the path above)", "guided")]
    if choice["orientation"] == "auto":
        rows.append(("orientation", "auto: the layout settles it from the typeset label widths (6.7; R9-10, "
                     "G2.4): run render", "R9-10", "guided"))
    else:
        rows.append(("orientation", choice["orientation"], "R2-37" if x.kind == "time" else "R9-10",
                     "locked" if x.kind == "time" else "guided"))
    if x.kind == "nominal" and choice["sort"] == "given" and choice.get("given_order"):
        rows.append(("sort", f"given: {choice['given_order']}", "R2-01 (G2.1)", "guided"))
    elif x.kind == "nominal":
        rows.append(("sort", f"{choice['sort']} by {choice['sort_by']}", "R2-01 (G2.1), R9-9", "guided"))
    else:
        rows.append(("sort", "given: the built-in order", "R2-02 (G2.2)", "locked"))
    if len(spec.data.series) == 1:
        rows.append(("labelling", "none: one series needs neither labels nor a legend", "R4-1 (G5.6)", "guided"))
    elif choice["type"] in ("bar", "dot") and opts.labelling == "auto":
        refs = sum(1 for s in spec.data.series if s.role in ("target", "reference"))
        rows.append(("labelling", "auto: " + (
            "the bars and their reference named once, beside the first bar (the layout)" if refs else
            "a legend row above the plot; turned, names beside the last cluster (the layout)"
            if choice["type"] == "bar" else "names beside the last category's dots, else a legend (the layout)"),
            "R4-1, R4-6, R11-4 (G5.1, G5.5, G5.7, G5.10; D-7, D-31)", "guided"))
    else:
        rows.append(("labelling", f"{opts.labelling}: direct end labels unless the collision test fails, then a "
                     "legend (the layout)" if opts.labelling == "auto" else opts.labelling, "R4-1, R4-2 (G5.1, G5.3)",
                     "guided"))
    return [{"part": p, "value": v, "rule": r, "level": lv} for p, v, r, lv in rows]


def _loaded(data):
    from .spec import SpecError, load
    try:
        return load(data)
    except SpecError:
        return None


def _complete(report, data):
    """The report with the original's choice rows and defaulted facts, from the engine's choice."""
    if report.get("choice") is not None and not isinstance(report["choice"], dict):
        return report                                   # already rows
    spec = _loaded(data) if report["status"] != "invalid" else None
    out = dict(report)
    choice = report.get("choice")
    # As the original: no rows when the chooser made no choice (a refusal before or in it).
    out["choice"] = _choice_rows(spec, choice, report) if spec is not None and choice else []
    if not choice and report["status"] == "ready":
        # A chart that would be drawn has a choice; the engine does not yet report a panel chart's.
        out["unreported"] = ["the choice (orientation, sort, labelling) of a panel chart's first panel"]
    if spec is None:
        out["defaults"] = []
    else:
        out["defaults"] = [{"path": p, "default": d, "used_by": w}
                           for p, d, w in _facts(spec, report.get("breakdown", False)) if not _given(data, p)]
    return out


def _choice(report):
    return report.get("choice") or []


def _unreported(report):
    """Parts of the original's choose output the engine does not supply (never silently absent)."""
    return list(report.get("unreported") or [])


def _choose_json(report, variant):
    out={"chart":report["chart"],"variant":variant,"status":report["status"],"type":report.get("type"),
         "path":report.get("path",[]),"decisions":report.get("decisions",[]),"choice":_choice(report),
         "findings":[Finding(**f).to_json() for f in report.get("findings",[])],"defaults":report.get("defaults") or []}
    missing=_unreported(report)
    if missing:
        out["unreported"]=missing
    return out

def _style_findings(style):
    """The original's style lint (Rust chartgen-lint) over the flattened style."""
    return dispatch({"operation":"lint_style","tokens":dict(style.tokens),
                     "rules":[[sel,[[p,v] for p,v in decls]] for sel,decls in style.rules]})["value"]

def _svg_extents(svg,style):
    """The typeset extent of each text of a source SVG, measured by the host as bake typesets it;
    None when the measurer cannot give ink extents (the lint then says it is partial)."""
    import hashlib
    from .measure import Measurer
    queries=dispatch({"operation":"lint_svg_texts","svg":svg,"css":style.css})["value"]
    requests=[]
    for q in queries:
        if not q["code"]:
            requests.append(None);continue
        st={"fill":"#000000","weight":q["weight"] or "normal","font_style":q["slant"] or "normal"}
        key=f"${q['code']}$" if q["mode"]=="math" else q["code"]
        ident=hashlib.sha256(json.dumps([q["mode"],q["code"],q["size"],st],ensure_ascii=False).encode()).hexdigest()[:24]
        requests.append({"id":ident,"content":{"kind":"native","host":"typst","key":key,"alternative":None},
                         "role":"prose","font_family":"New Computer Modern","font_size":q["size"],"style":st,
                         "max_width":None,"context":"lint"})
    unique=list({r["id"]:r for r in requests if r}.values())
    try:
        got=Measurer(style.css).measure_requests(unique) if unique else {}
    except (NotImplementedError,RuntimeError,OSError):
        return None
    out=[]
    for r in requests:
        if r is None:
            out.append(None);continue
        m=got.get(r["id"])
        if not m or m.get("baseline") is None:
            return None
        out.append([m["width"],m["baseline"],m["height"]-m["baseline"]])
    return out

def main(argv=None):
    parser=argparse.ArgumentParser(prog="chartgen",description=__doc__)
    commands=parser.add_subparsers(dest="command",required=True)
    schema=commands.add_parser("schema");schema.add_argument("--out")
    choose=commands.add_parser("choose");choose.add_argument("spec");choose.add_argument("--variant");choose.add_argument("--json",action="store_true")
    draw=commands.add_parser("render");draw.add_argument("spec");draw.add_argument("--variant");draw.add_argument("--out",required=True);draw.add_argument("--report",required=True)
    build=commands.add_parser("build");build.add_argument("specs");build.add_argument("--src",required=True);build.add_argument("--reports",required=True)
    for command in (draw,build):
        command.add_argument("--style",default="plain")
        command.add_argument("--backend",choices=("compatibility_svg","svg","pgf","typst"),default="compatibility_svg")
    style=commands.add_parser("style");style.add_argument("style");style.add_argument("--out",required=True);style.add_argument("--report")
    lint=commands.add_parser("lint");lint.add_argument("svg",nargs="+");lint.add_argument("--style",default="plain");lint.add_argument("--final-width",type=float);lint.add_argument("--cvd",choices=("assume","no"),default="assume");lint.add_argument("--json")
    args=parser.parse_args(argv)
    try:
        if args.command=="schema":
            text=json.dumps(dispatch({"operation":"schema"})["value"],ensure_ascii=False,indent=1)+"\n"
            if args.out:write(args.out,text)
            else:print(text,end="")
            return 0
        if args.command=="choose":
            spec,expect=selected(args.spec,args.variant)
            report=dispatch({"operation":"choose","spec":spec,"variant":args.variant,"expect":expect})["value"]
            report=_complete(report,spec)
            if args.json:print(json.dumps(_choose_json(report,args.variant),ensure_ascii=False,indent=1))
            else:_choose_text(report,args.variant)
            return 0 if report['status'] in ('ready','unsupported') else 1
        if args.command=="render":return render_one(args,args.spec,args.variant,args.out,args.report)
        if args.command=="build":
            failed=0
            for path in sorted(Path(args.specs).glob("*.json")):
                spec,_,variants=read_file(path)
                for variant in [None,*variants]:
                    name=spec.get("id",path.stem)+("--"+variant if variant else "")
                    suffix={"pgf":".pgf","typst":".json"}.get(args.backend,".svg")
                    out=Path(args.src)/(name+suffix)
                    try:
                        failed+=render_one(args,path,variant,out,Path(args.reports)/(name+".json"))
                    except (RuntimeError,ValueError,OSError,NotImplementedError) as error:
                        # An engine error fails this chart explicitly; the build goes on with the rest.
                        out.unlink(missing_ok=True)
                        (Path(args.reports)/(name+".json")).unlink(missing_ok=True)
                        print(f"ERR  {name:48s} {error}")
                        failed+=1
            if failed:print(f"{failed} chart(s) did not do what their spec expects",file=sys.stderr)
            return int(failed>0)
        if args.command=="style":
            style=Style(args.style);write(args.out,style.css)
            findings=_style_findings(style)
            if args.report:write(args.report,json.dumps({"style":style.name,"findings":findings},ensure_ascii=False,indent=1)+"\n")
            for f in findings:
                print(f"style {style.name}: {f['rule']} {f['severity']}: {f['message']}",file=sys.stderr)
            print(style.defs or "")
            return 0
        if args.command=="lint":
            style_=Style(args.style)
            results={}
            failed=False
            for path in args.svg:
                svg=Path(path).read_text(encoding="utf-8")
                findings=dispatch({"operation":"lint_svg","svg":svg,"style":args.style,"css":style_.css,
                                   "extents":_svg_extents(svg,style_),
                                   "final_width":args.final_width,"cvd":args.cvd})["value"]
                results[path]=findings
                print(f"{path} under {args.style}: {_counts(findings)}")
                _findings(findings)
                failed |= any(f["severity"]=="error" for f in findings)
            if args.json:
                write(args.json,json.dumps({"style":args.style,"files":results},ensure_ascii=False,indent=1)+"\n")
            # A partial lint (texts not measured) must not certify a source SVG as clean.
            partial=any(f["rule"]=="CG-SVG-PARTIAL" for fs in results.values() for f in fs)
            return 1 if failed else 2 if partial else 0
    except (RuntimeError,ValueError,OSError,NotImplementedError) as error:
        print(f"chartgen: {error}",file=sys.stderr);return 1
    return 0

if __name__=="__main__":sys.exit(main())
