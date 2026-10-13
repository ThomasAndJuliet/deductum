/** Peer TeX build helper. Uses the native Rust executable, never the Python facade. */
import {spawnSync} from 'node:child_process';
import {mkdtempSync, writeFileSync, readFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {Session} from '../figkit-web/figkit.mjs';
export function nativeDispatch(executable='figkit-dispatch') {
  return input => {
    const r=spawnSync(executable, [], {input, encoding:'utf8', maxBuffer:64*1024*1024});
    if(r.error || r.status !== 0) throw r.error ?? new Error(r.stderr);
    return r.stdout;
  };
}
const escape = s => s.replace(/[ \\{}$&#_%~^]/g,c=>({'\\':'\\textbackslash{}','~':'\\textasciitilde{}','^':'\\textasciicircum{}'}[c] ?? '\\'+c));
export class TeXSession extends Session {
  constructor(dispatch, context, {preamble='', engine='pdflatex'}={}) { super(dispatch,context); this.preamble=preamble; this.engine=engine; }
  measureBatch(requests) {
    const dir=mkdtempSync(join(tmpdir(),'figkit-tex-'));
    try {
      const lines=requests.map((r,i)=> {
        const t=r.typography;
        if(r.content.kind==='native' && !['latex','tex'].includes(r.content.host)) throw new Error('unsupported native host for TeX');
        if(!['normal','bold'].includes(t.weight) || !['normal','italic'].includes(t.font_style)) throw new Error('unsupported TeX typography');
        if(!/^[A-Za-z0-9_-]+$/.test(t.font_family)) throw new Error('TeX font family must be an NFSS identifier');
        if(r.content.kind==='plain' && r.content.text.includes('\t')) throw Object.assign(new Error('plain TeX text does not support tabs'),{kind:'unsupported_capability'});
        const body=r.content.kind==='native' ? this.resolve(r.content.key) : escape(r.content.text);
        const font=`\\fontfamily{${t.font_family}}\\fontsize{${t.font_size}bp}{${t.font_size*1.2}bp}\\selectfont${t.weight==='bold'?'\\bfseries':''}${t.font_style==='italic'?'\\itshape':''}`;
        const box=r.max_width==null ? `${font} ${body}` : `\\parbox{${r.max_width}bp}{${font} ${body}}`;
        return `\\sbox{\\figkitbox}{${box}}\\typeout{FIGKIT-${i}:\\number\\wd\\figkitbox,\\number\\ht\\figkitbox,\\number\\dp\\figkitbox}`;
      });
      writeFileSync(join(dir,'measure.tex'),`\\documentclass{article}\n${this.preamble}\n\\newsavebox{\\figkitbox}\n\\begin{document}\n${lines.join('\n')}\n\\end{document}`);
      const r=spawnSync(this.engine,['-interaction=nonstopmode','-halt-on-error','measure.tex'],{cwd:dir,encoding:'utf8',maxBuffer:16*1024*1024});
      if(r.error || r.status!==0) throw r.error ?? new Error(r.stdout+r.stderr);
      const log=readFileSync(join(dir,'measure.log'),'utf8');
      return requests.map((request,i)=> {
        const m=log.match(new RegExp(`FIGKIT-${i}:(\\d+),(\\d+),(\\d+)`));
        if(!m) throw new Error('TeX did not return measurement');
        const [width,ascent,descent]=m.slice(1).map(n=>Number(n)/65536*72/72.27);
        return {request,metrics:{size:{width,height:ascent+descent},baseline:request.max_width==null?ascent:null}};
      });
    } finally { rmSync(dir,{recursive:true,force:true}); }
  }
  async measure(requests) { return super.measure(requests, pending=>this.measureBatch(pending)); }
}

/** Compile using saved native boxes, verifying the final boxes against measured scene metrics. */
TeXSession.prototype.compose = function(figure) {
  const pgf=this.render(figure,'pgf');
  const native=[];
  const visit=nodes=>{for(const node of nodes) {if(node.kind==='group') visit(node.children); if(node.kind==='text' && node.placed.text.request.content.kind==='native') native.push(node.placed.text);}};
  visit(figure.primitives);
  const identities=new Map();
  for(const text of native) {
    const key=text.request.content.key, identity=JSON.stringify([text.request.typography,text.request.max_width]);
    if(identities.has(key) && identities.get(key)!==identity) throw new Error('use distinct native keys for differently styled TeX saved boxes');
    identities.set(key,identity);
  }
  const dir=mkdtempSync(join(tmpdir(),'figkit-compose-'));
  try {
    writeFileSync(join(dir,'figkit.sty'),readFileSync(join(dirname(fileURLToPath(import.meta.url)),'figkit.sty')));
    const declarations=native.map((text,i)=>{
      const r=text.request,t=r.typography,key=r.content.key;
      if(!/^[A-Za-z0-9_:-]+$/.test(key) || !/^[A-Za-z0-9_-]+$/.test(t.font_family)) throw new Error('unsupported TeX key/font family');
      const font=`\\fontfamily{${t.font_family}}\\fontsize{${t.font_size}bp}{${t.font_size*1.2}bp}\\selectfont${t.weight==='bold'?'\\bfseries':''}${t.font_style==='italic'?'\\itshape':''}`;
      const content=`${font} ${this.resolve(key)}`;
      const body=r.max_width==null?content:`\\parbox{${r.max_width}bp}{${content}}`;
      return `\\figkitDeclareNative{${key}}{${body}}\\typeout{FIGKIT-FINAL-${i}:\\number\\figkitNativeWidth{${key}},\\number\\dimexpr\\figkitNativeHeight{${key}}\\relax}`;
    });
    writeFileSync(join(dir,'figure.tex'),`\\documentclass{article}\\usepackage{figkit}
${this.preamble}
\\pagestyle{empty}\\begin{document}
${declarations.join('\n')}
${pgf}
\\end{document}`);
    const result=spawnSync(this.engine,['-interaction=nonstopmode','-halt-on-error','figure.tex'],{cwd:dir,encoding:'utf8',maxBuffer:16*1024*1024});
    if(result.error || result.status!==0) throw result.error ?? new Error(result.stdout+result.stderr);
    const log=readFileSync(join(dir,'figure.log'),'utf8');
    native.forEach((text,i)=>{
      const m=log.match(new RegExp(`FIGKIT-FINAL-${i}:(\\d+),(\\d+)`));
      if(!m) throw new Error('final native box not measured');
      const actual=m.slice(1).map(n=>Number(n)/65536*72/72.27),expected=text.metrics.size;
      if(Math.abs(actual[0]-expected.width)>0.001 || Math.abs(actual[1]-expected.height)>0.001) throw new Error('stale final native box measurement');
    });
    return readFileSync(join(dir,'figure.pdf'));
  } finally {rmSync(dir,{recursive:true,force:true});}
};
