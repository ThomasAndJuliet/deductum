"""Host-owned native registry and bounded measurement driver for protocol version 2."""
import copy
import json
from . import _native


class FigureError(RuntimeError):
    def __init__(self, error):
        self.kind = error['kind']
        self.path = error.get('path')
        super().__init__(error['message'])


def dispatch(operation, **fields):
    response = json.loads(_native.dispatch(json.dumps(dict(version=2, operation=operation, **fields), allow_nan=False)))
    if response['outcome'] == 'error':
        raise FigureError(response['error'])
    return response.get('value', response.get('output'))


class Session:
    """Registry values stay in this host until close, including during composition.

    Context includes engine/version, fonts, settings and registry revision. Register
    all native content before starting measurement; changing it requires a new session.
    """
    def __init__(self, context):
        self.context = copy.deepcopy(context)
        self.registry = {}
        self.state = None
        self.closed = False
        self.started = False

    def register(self, key, content):
        if self.closed or self.started:
            raise RuntimeError('register content before measurement starts')
        if key in self.registry:
            raise ValueError('native key already registered')
        self.registry[key] = content

    def resolve(self, key):
        if self.closed:
            raise RuntimeError('native registry is closed')
        return self.registry[key]

    def measure(self, requests, measure_batch):
        if self.closed:
            raise RuntimeError('measurement session is closed')
        self.started = True
        requests = copy.deepcopy(requests)
        for request in requests:
            content = request['content']
            if content['kind'] == 'native':
                self.resolve(content['key'])
        result = dispatch('measure', context=self.context, session=self.state, requests=requests, replies=[])
        while not result['complete']:
            pending = result['requests']
            replies = measure_batch(copy.deepcopy(pending), self)
            if not replies:
                raise FigureError(dict(kind='missing_measurement', message='measurement round made no progress'))
            result = dispatch('measure', session=result['session'], requests=requests, replies=replies)
        self.state = result['session']
        return {key: copy.deepcopy(reply) for key, reply in self.state['replies'].items() if key in {r['id'] for r in requests}}

    def render(self, figure, backend='svg'):
        if self.closed:
            raise RuntimeError('measurement session is closed')
        def validate(nodes):
            for node in nodes:
                if node['kind'] == 'group':
                    validate(node['children'])
                elif node['kind'] == 'text':
                    request = node['placed']['text']['request']
                    if request['context'] != self.context:
                        raise FigureError(dict(kind='stale_measurement', message='scene context differs from host session'))
                    if request['content']['kind'] == 'native':
                        self.resolve(request['content']['key'])
                    if self.state is None:
                        raise FigureError(dict(kind='missing_measurement', message='scene has no measurements in this session'))
        validate(figure['primitives'])
        return dispatch('render_scene', figure=figure, backend=backend, session=self.state)

    def close(self):
        self.registry.clear()
        self.closed = True
