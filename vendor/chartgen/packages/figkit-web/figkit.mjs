/** Independent host adapter. Supply dispatch from figkit-web WASM, and host metrics. */
export class Session {
  constructor(dispatch, context) { this.dispatch = dispatch; this.context = structuredClone(context); this.registry = new Map(); this.state = null; this.closed = false; this.started = false; }
  call(operation, fields = {}) {
    const r = JSON.parse(this.dispatch(JSON.stringify({version: 2, operation, ...fields})));
    if (r.outcome === 'error') { const e = new Error(r.error.message); Object.assign(e, r.error); throw e; }
    return r.value ?? r.output;
  }
  register(key, content) {
    if (this.closed || this.started) throw new Error('register content before measurement');
    if (this.registry.has(key)) throw new Error('native key already registered');
    this.registry.set(key, content);
  }
  resolve(key) {
    if (this.closed || !this.registry.has(key)) throw new Error('native registry key unavailable');
    return this.registry.get(key);
  }
  async measure(requests, measureBatch) {
    if (this.closed) throw new Error('session closed');
    this.started = true;
    for (const r of requests) if (r.content.kind === 'native') this.resolve(r.content.key);
    let r = this.call('measure', {context:this.context, session:this.state, requests, replies:[]});
    while (!r.complete) {
      const replies = await measureBatch(structuredClone(r.requests), this);
      if (!replies.length) throw new Error('measurement round made no progress');
      r = this.call('measure', {session:r.session, requests, replies});
    }
    this.state = r.session;
    return r.session.replies;
  }
  render(figure, backend='svg') {
    if (this.closed) throw new Error('session closed');
    const visit=nodes=>{for(const node of nodes) {
      if(node.kind==='group') visit(node.children);
      if(node.kind==='text') {
        const r=node.placed.text.request;
        if(Object.keys(this.context).some(k=>r.context[k]!==this.context[k]) || Object.keys(r.context).some(k=>r.context[k]!==this.context[k])) throw Object.assign(new Error('scene context differs from host session'),{kind:'stale_measurement'});
        if(r.content.kind==='native') this.resolve(r.content.key);
        if(!this.state) throw Object.assign(new Error('scene has no measurements in this session'),{kind:'missing_measurement'});
      }
    }};
    visit(figure.primitives);
    return this.call('render_scene', {figure, backend, session:this.state});
  }
  close() { this.registry.clear(); this.closed = true; }
}
/** Compose ordered geometry and host-native DOM content. Registry remains live until close. */
export function compose(plan, session, document=globalThis.document) {
  if (plan.version !== 2 || session.closed) throw new Error('invalid plan or closed registry');
  const canvas=document.createElement('div');
  Object.assign(canvas.style,{position:'relative',width:`${plan.width}pt`,height:`${plan.height}pt`,overflow:'hidden'});
  for(const layer of plan.layers) {
    if(layer.kind==='svg') {
      const geometry=document.createElement('div');
      Object.assign(geometry.style,{position:'absolute',left:'0',top:'0'});
      geometry.innerHTML=layer.svg;
      canvas.append(geometry);
    } else if(layer.kind==='text') {
      const r=layer.request;
      if(JSON.stringify(r.context)!==JSON.stringify(session.context)) {
        // Compare values independently of JSON dictionary insertion order.
        if(Object.keys(session.context).some(k=>r.context[k]!==session.context[k])) throw new Error('stale composition context');
      }
      const label=document.createElement('div');
      Object.assign(label.style,{position:'absolute',left:`${layer.x}pt`,top:`${layer.y}pt`,width:`${layer.metrics.size.width}pt`,height:`${layer.metrics.size.height}pt`,fontFamily:r.typography.font_family,fontSize:`${r.typography.font_size}pt`,fontWeight:r.typography.weight,fontStyle:r.typography.font_style,whiteSpace:r.max_width==null?'pre':'pre-wrap'});
      if(layer.paint.kind==='resolved') label.style.color=layer.paint.fill;
      if(r.content.kind==='native') label.append(session.resolve(r.content.key).cloneNode(true));
      else label.textContent=r.content.text;
      if(layer.clip) {
        const c=layer.clip, clip=document.createElement('div');
        Object.assign(clip.style,{position:'absolute',left:`${c.x}pt`,top:`${c.y}pt`,width:`${c.width}pt`,height:`${c.height}pt`,overflow:'hidden'});
        label.style.left=`${layer.x-c.x}pt`;label.style.top=`${layer.y-c.y}pt`;clip.append(label);canvas.append(clip);
      } else canvas.append(label);
    } else throw new Error('unsupported composition layer');
  }
  return canvas;
}
