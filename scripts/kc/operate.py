#!/usr/bin/env python3
"""Explicit local account/model setup and authenticated readiness. Never prints tokens."""
import argparse
import json
import os
from pathlib import Path
import stat
import sys
import urllib.error
import urllib.parse
import urllib.request


def protected_read(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o600 or info.st_uid != os.getuid():
            raise ValueError('input must be an operator-owned regular file with mode 0600')
        with os.fdopen(fd, closefd=False) as stream:
            return stream.read().strip()
    finally:
        os.close(fd)


class RejectRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # Never forward account passwords, bearer tokens or provider input.
        return None


def request(base, method, path, body=None, token=None):
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    raw = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=raw, method=method, headers=headers)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), RejectRedirects())
    try:
        with opener.open(req, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        # Server/provider messages can contain rejected URLs. Print only status.
        error.close()
        raise ValueError(f'API request failed with HTTP {error.code}') from None


def run(args):
    url = urllib.parse.urlsplit(args.server)
    if url.scheme != 'http' or url.hostname not in ('127.0.0.1', 'localhost', '::1') or url.username or url.password or url.query or url.fragment or url.path not in ('', '/'):
        raise ValueError('server must be a loopback HTTP origin')
    if args.action == 'check' and not 1 <= args.vector_size <= 65536:
        raise ValueError('vector size must be within 1..65536')
    base = args.server.rstrip('/')
    dest = args.credentials
    if args.action in ('create-service-account', 'register-embedding'):
        login = request(base, 'POST', '/v1/login', {'username': 'admin', 'password': protected_read(dest / 'admin_password')})
        if login['role'] != 'admin': raise ValueError('administrator login required')
        if args.action == 'create-service-account':
            body = {'username': args.username, 'password': protected_read(dest / 'embedding_password'), 'expiry': args.expiry}
            request(base, 'POST', '/v1/user', body, login['access_token'])
            print('Created service account. Run check before using KC.')
        else:
            body = json.loads(protected_read(args.model_file))
            request(base, 'POST', '/v1/embedding_model', body, login['access_token'])
            print('Registered embedding model. Verify its dimensions and provider readiness.')
    else:
        login = request(base, 'POST', '/v1/login', {'username': args.username, 'password': protected_read(dest / 'embedding_password')})
        if login['role'] != 'user': raise ValueError('KC requires a role=user service account')
        catalog = request(base, 'GET', '/v1/embedding_models', token=login['access_token'])
        if not any(m['name'] == args.model for m in catalog['models']):
            raise ValueError('required embedding model is not registered')
        response = request(base, 'POST', '/v1/embeddings', {'model': args.model, 'dimensions': args.vector_size, 'input': ['Synthetic local readiness check.']}, login['access_token'])
        vectors = response.get('data', [])
        if len(vectors) != 1 or len(vectors[0].get('embedding', [])) != args.vector_size:
            raise ValueError('provider returned unexpected embedding dimensions')
        print('Service login and actual embedding readiness passed. Storage/MCP still require an end-to-end smoke check.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--credentials', required=True, type=Path)
    parser.add_argument('--server', required=True)
    parser.add_argument('--username', default='kc-embedding-service')
    sub = parser.add_subparsers(dest='action', required=True)
    account = sub.add_parser('create-service-account')
    account.add_argument('--expiry', required=True, help='explicit future RFC3339 account expiry')
    model = sub.add_parser('register-embedding')
    model.add_argument('--model-file', required=True, type=Path)
    check = sub.add_parser('check')
    check.add_argument('--model', required=True)
    check.add_argument('--vector-size', required=True, type=int)
    args = parser.parse_args()
    try:
        run(args)
    except (OSError, ValueError, KeyError, urllib.error.URLError):
        print('Operator action failed. Check protected inputs, server, account, model and provider readiness.', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__': sys.exit(main())
