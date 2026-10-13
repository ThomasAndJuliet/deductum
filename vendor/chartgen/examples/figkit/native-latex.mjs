import {writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {TeXSession, nativeDispatch} from '../../packages/figkit-latex/figkit.mjs';

const executable = process.argv[2] ?? fileURLToPath(new URL('../../target/debug/figkit-dispatch', import.meta.url));
const output = process.argv[3] ?? 'figkit-latex.pdf';
const context = {id: 'example', engine: 'pdflatex', engine_version: 'local',
  font_environment: 'cmr', settings: '', registry_revision: '1'};
const session = new TeXSession(nativeDispatch(executable), context);
session.register('equation', 'Native mathematics: $x^2+y^2=r^2$');
const request = {id: 'equation', content: {kind: 'native', host: 'latex', key: 'equation', alternative: null},
  typography: {font_family: 'cmr', font_size: 12, weight: 'normal', font_style: 'normal'},
  max_width: null, context};
const replies = await session.measure([request]);
const figure = {version: 2, size: {width: 240, height: 70}, background: 'white',
  metadata: {title: 'Native LaTeX figure', description: null}, resources: {clips: {}, symbols: {}},
  primitives: [{kind: 'text', placed: {
    text: {request, metrics: replies.equation.metrics, paint: {kind: 'resolved', fill: 'black'}},
    position: {x: 12, y: 25}, anchor: 'start',
    metadata: {element_id: null, authored_id: null, classes: [], label_group: null}}}]};
writeFileSync(output, session.compose(figure));
console.log(`Wrote ${output}`);
