"""Capture only ff0a238's head arithmetic; never run against current production.
The returned head for a straight horizontal S=(0,0), Q=(20,0), E=(40,0)
has x=40-a,y=c. Baseline source is compiled verbatim, not reimplemented.
"""
from pathlib import Path
import json
import subprocess
import tempfile
source=subprocess.check_output(['git','show','ff0a238:crates/chartgen-layout/src/families/kit.rs'],text=True)
start=source.index('pub fn head_outline(')
end=source.index('\npub fn bezier(',start)
with tempfile.TemporaryDirectory() as temp:
    path=Path(temp)
    (path/'main.rs').write_text(source[start:end]+'''\nfn main(){for (a,c) in head_outline(0.1025){println!("{a:.17} {c:.17}");}}''')
    subprocess.run(['rustc',str(path/'main.rs'),'-o',str(path/'capture')],check=True)
    data=subprocess.check_output([str(path/'capture')],text=True)
points=[[40-float(a),float(c)] for a,c in (line.split() for line in data.splitlines())]
Path(__file__).with_name('family_head.json').write_text(json.dumps({'baseline':'ff0a238','source':'crates/chartgen-layout/src/families/kit.rs::head_outline','width':0.1025,'head':points},indent=2)+'\n')
