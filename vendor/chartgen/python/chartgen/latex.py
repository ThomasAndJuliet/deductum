"""LaTeX host measurement in an explicitly supplied document font context."""
import os
from pathlib import Path
import re
import subprocess
import tempfile

def escape(text):
    special = {"\\":r"\textbackslash{}","{":r"\{","}":r"\}","%":r"\%","&":r"\&","#":r"\#","$":r"\$","_":r"\_","^":r"\textasciicircum{}","~":r"\textasciitilde{}","−":r"\ensuremath{-}","–":"--","—":"---"}
    return "".join(special.get(c,c) for c in text)

class TeXMeasurer:
    """Use the same preamble and native declarations when including output.

    `native` maps stable label keys to explicit TeX content. Font selection is
    inherited from the preamble; the request supplies physical font sizes.
    """
    def __init__(self, preamble="", native=None, engine="pdflatex"):
        self.preamble, self.native, self.engine = preamble, native or {}, engine
        self.context_signature = (preamble, self.native, engine)

    def declarations(self):
        return "\n".join(r"\chartgenDeclareNative{" + self._key(key) + "}{" + value + "}" for key,value in self.native.items())

    @staticmethod
    def _key(key):
        if not re.fullmatch(r"[A-Za-z0-9_:-]+",key):
            raise ValueError("LaTeX native keys must contain only letters, digits, _, :, or -")
        return key

    def measure_requests(self, requests):
        source=[r"\documentclass{article}",self.preamble,r"\begin{document}",r"\newbox\chartgenMeasureBox"]
        for request in requests:
            content=request["content"]
            if content["kind"]=="plain":
                body=escape(content["text"])
            elif content["host"] in ("latex","tex"):
                key=self._key(content["key"])
                if key not in self.native:
                    raise ValueError(f"LaTeX native content {key!r} needs a declaration")
                body=self.native[key]
            elif content.get("alternative") is not None:
                body=escape(content["alternative"])
            else:
                raise ValueError("This label needs a LaTeX text alternative")
            size=request["font_size"]
            style=request.get("style") or {}
            weight=style.get("weight", "normal")
            slant=style.get("font_style", "normal")
            if weight not in ("normal", "bold") or slant not in ("normal", "italic"):
                raise ValueError("Unsupported LaTeX text weight or font style")
            body=f"\\fontsize{{{size}bp}}{{{size*1.2}bp}}\\selectfont " + (r"\bfseries " if weight=="bold" else "") + (r"\itshape " if slant=="italic" else "") + body
            source.append(r"\setbox\chartgenMeasureBox=\hbox{"+body+"}")
            if request.get("max_width") is not None:
                width=request["max_width"]
                # Plain and native content alike: wider than the limit, a top-aligned paragraph of
                # that width (as the PGF renderer sets it); its first line's height is the baseline.
                source.append(f"\\ifdim\\wd\\chartgenMeasureBox>{width}bp \\setbox\\chartgenMeasureBox=\\hbox{{\\parbox[t]{{{width}bp}}{{{body}}}}}\\fi")
            source.append(r"\typeout{CHARTGEN-METRIC:"+request["id"]+r":\number\wd\chartgenMeasureBox:\number\ht\chartgenMeasureBox:\number\dp\chartgenMeasureBox}")
        source.append(r"\end{document}")
        with tempfile.TemporaryDirectory(prefix="chartgen-tex-") as directory:
            path=Path(directory)/"measure.tex";path.write_text("\n".join(source))
            process=subprocess.run([self.engine,"-interaction=nonstopmode","-halt-on-error",str(path)],cwd=directory,text=True,capture_output=True,timeout=60)
            if process.returncode:
                raise RuntimeError("LaTeX measurement failed:\n"+process.stdout[-3000:])
            entries={}
            # TeX sp units -> physical 1/72-inch points, not TeX's 1/72.27 inch.
            conversion=72/72.27/65536
            limits={r["id"]:r.get("max_width") for r in requests}
            for key,width,ascent,descent in re.findall(r"CHARTGEN-METRIC:([^:\s]+):(\d+):(\d+):(\d+)",process.stdout):
                width=int(width)*conversion
                if limits.get(key) is not None:
                    width=min(width,limits[key])      # a paragraph's box is the limit, to TeX's sp rounding
                entries[key]={"width":width,"height":(int(ascent)+int(descent))*conversion,"baseline":int(ascent)*conversion}
            return entries
