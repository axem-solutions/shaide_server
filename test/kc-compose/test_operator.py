import importlib.util
from contextlib import contextmanager, redirect_stderr, redirect_stdout
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('operator_helper', ROOT / 'scripts/kc/operate.py')
op = importlib.util.module_from_spec(spec); spec.loader.exec_module(op)

@contextmanager
def local_api(respond):
    calls = []
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def handle_request(self):
            raw = self.rfile.read(int(self.headers.get('Content-Length', 0)))
            calls.append((self.command, self.path, dict(self.headers), raw))
            status, headers, body = respond(self.command, self.path, raw)
            payload = json.dumps(body).encode()
            self.send_response(status)
            for name, value in headers.items(): self.send_header(name, value)
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        do_GET = do_POST = handle_request
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': 0.01})
    thread.start()
    try:
        yield 'http://127.0.0.1:' + str(server.server_port), calls
    finally:
        server.shutdown(); server.server_close(); thread.join()


class OperatorTests(unittest.TestCase):
    def check_args(self, server='http://127.0.0.1:55555', size=3):
        return SimpleNamespace(server=server, credentials=Path('/unused'), action='check',
                               username='fixture', model='fixture', vector_size=size)

    def test_all_redirect_statuses_rejected_without_followup_or_secret_leak(self):
        marker = 'synthetic-bearer-marker'
        secret_body = {'password': 'synthetic-body-marker'}
        with local_api(lambda *a: (200, {}, {'ok': True})) as (destination, received):
            for status in (301, 302, 303, 307, 308):
                for method in ('GET', 'POST'):
                    for cross_origin in (False, True):
                        with self.subTest(status=status, method=method, cross_origin=cross_origin):
                            received.clear()
                            def respond(command, path, raw):
                                return (status, {'Location': (destination if cross_origin else '') + '/destination?synthetic-url-marker'}, {}) if path == '/redirect' else (200, {}, {'ok': True})
                            with local_api(respond) as (origin, sent):
                                with self.assertRaises(ValueError) as failure:
                                    op.request(origin, method, '/redirect', secret_body if method == 'POST' else None, marker)
                                self.assertEqual(str(failure.exception), f'API request failed with HTTP {status}')
                                self.assertEqual(len(sent), 1)
                                self.assertEqual(received, [])
                                for value in (origin, destination, marker, secret_body['password'], 'synthetic-url-marker'):
                                    self.assertNotIn(value, str(failure.exception))

    def test_cli_redirect_failure_is_sanitized(self):
        with local_api(lambda *a: (302, {'Location': 'http://127.0.0.1:1/synthetic-url-marker'}, {})) as (origin, calls):
            stdout, stderr = io.StringIO(), io.StringIO()
            argv = ['operate.py', '--credentials', '/unused', '--server', origin,
                    'check', '--model', 'fixture', '--vector-size', '3']
            with patch.object(op, 'protected_read', return_value='synthetic-password-marker'), patch.object(op.sys, 'argv', argv), redirect_stdout(stdout), redirect_stderr(stderr):
                self.assertEqual(op.main(), 1)
            self.assertEqual(len(calls), 1)
            self.assertEqual(stdout.getvalue(), '')
            self.assertIn('Operator action failed.', stderr.getvalue())
            for value in (origin, 'synthetic-url-marker', 'synthetic-password-marker'):
                self.assertNotIn(value, stderr.getvalue())

    def test_invalid_dimensions_fail_before_credentials_or_network(self):
        for size in (0, -1, 65537):
            with self.subTest(size=size), patch.object(op, 'protected_read') as read, patch.object(op, 'request') as request:
                with self.assertRaisesRegex(ValueError, '1..65536'): op.run(self.check_args(size=size))
                read.assert_not_called(); request.assert_not_called()

    def embedding_fixture(self, declared=3, actual=3):
        def respond(method, path, raw):
            if path == '/v1/login': return 200, {}, {'role': 'user', 'access_token': 'synthetic-token'}
            if path == '/v1/embedding_models': return 200, {}, {'models': [{'name': 'fixture', 'vector_size': declared}]}
            if path == '/v1/embeddings':
                body = json.loads(raw)
                # Mirrors the server's check_dimensions: omission is accepted.
                if 'dimensions' in body and body['dimensions'] != declared:
                    return 400, {}, {'error': 'synthetic provider URL/token must not leak'}
                return 200, {}, {'data': [{'embedding': [0.0] * actual}]}
            self.fail('unexpected fixture request')
        return local_api(respond)

    def test_declared_model_dimensions_mismatch_is_rejected_by_server(self):
        with self.embedding_fixture(declared=4) as (origin, calls), patch.object(op, 'protected_read', return_value='synthetic'):
            with self.assertRaisesRegex(ValueError, 'HTTP 400'): op.run(self.check_args(origin))
            self.assertEqual(json.loads(calls[-1][3])['dimensions'], 3)

    def test_provider_output_dimensions_remain_validated(self):
        with self.embedding_fixture(actual=4) as (origin, calls), patch.object(op, 'protected_read', return_value='synthetic'):
            with self.assertRaisesRegex(ValueError, 'unexpected embedding dimensions'): op.run(self.check_args(origin))
            self.assertEqual(json.loads(calls[-1][3])['dimensions'], 3)

    def test_supported_dimension_boundaries_are_sent(self):
        for size in (1, 65536):
            with self.subTest(size=size), self.embedding_fixture(declared=size, actual=size) as (origin, calls), patch.object(op, 'protected_read', return_value='synthetic'), redirect_stdout(io.StringIO()):
                op.run(self.check_args(origin, size))
                self.assertEqual(json.loads(calls[-1][3])['dimensions'], size)

    def test_protected_inputs_reject_public_files_and_symlinks(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TMPDIR')) as directory:
            p = Path(directory) / 'secret'; p.write_text('sensitive'); p.chmod(0o644)
            with self.assertRaises(ValueError): op.protected_read(p)
            p.chmod(0o600); self.assertEqual(op.protected_read(p), 'sensitive')
            link = p.parent / 'link'; link.symlink_to(p)
            with self.assertRaises(OSError): op.protected_read(link)
    def test_admin_cannot_be_embedding_service(self):
        args = SimpleNamespace(server='http://127.0.0.1:55555',credentials=Path('/unused'),action='check',username='fixture',model='fixture',vector_size=3)
        with patch.object(op, 'protected_read', return_value='synthetic'), patch.object(op, 'request', return_value={'role': 'admin'}):
            with self.assertRaises(ValueError): op.run(args)
    def test_nonlocal_origins_rejected_before_secret_read(self):
        for server in ['http://example.com', 'http://user:password@localhost', 'http://localhost?secret=bad', 'http://localhost/path']:
            with self.subTest(server=server):
                with self.assertRaises(ValueError): op.run(SimpleNamespace(server=server))
