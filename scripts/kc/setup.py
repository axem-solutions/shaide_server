#!/usr/bin/env python3
"""Create a NEW operator-owned credential directory. Never update existing state."""
import argparse
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys

TOKENS = ('admin_password', 'jwt_secret', 'session_secret', 'internal_token',
          'webhook_token', 'gateway_token', 'scoped_token', 'embedding_password',
          'storage_access_key', 'storage_secret_key')


def write_exclusive(path, content):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, 'wb', closefd=False) as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
    finally:
        os.close(fd)


def generate(destination, kc_image, mcp_image, rustfs_image):
    for image in (kc_image, mcp_image, rustfs_image):
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._/:@-]*', image):
            raise ValueError('image reference must contain only image-reference characters')
    destination = Path(os.path.abspath(destination))
    for parent in destination.parents:
        if parent.is_symlink():
            raise ValueError('symlink parents are not allowed')
    # A single mkdir is the ownership boundary: EEXIST, including dangling
    # symlinks, returns before chmod, writes or cleanup can touch user state.
    destination.mkdir(mode=0o700)
    try:
        destination.chmod(0o700)
        for name in TOKENS:
            write_exclusive(destination / name, (secrets.token_urlsafe(36) + '\n').encode())
        # OpenSSL writes only to captured memory; we create files exclusively.
        key = subprocess.run(['openssl', 'genpkey', '-algorithm', 'RSA', '-pkeyopt',
                              'rsa_keygen_bits:2048'], check=True, capture_output=True).stdout
        public = subprocess.run(['openssl', 'pkey', '-pubout'], input=key,
                                check=True, capture_output=True).stdout
        write_exclusive(destination / 'private.pem', key)
        write_exclusive(destination / 'public.pem', public)
        # Compose single-quoted values preserve literal dollars in paths.
        path = str(destination)
        if any(c in path for c in "'\n\r"):
            raise ValueError('destination cannot contain quotes or newlines')
        env = (f"KC_SECRETS_DIR='{path}'\nKC_LOCAL_UID={os.getuid()}\nKC_LOCAL_GID={os.getgid()}\n"
               f'SHAIDE_KC_IMAGE={kc_image}\nSHAIDE_KC_MCP_IMAGE={mcp_image}\n'
               f'SHAIDE_RUSTFS_IMAGE={rustfs_image}\nKC_SHAIDE_USERNAME=kc-embedding-service\n')
        write_exclusive(destination / 'config.env', env.encode())
    except BaseException:
        # Only files in the newly created private directory are owned here.
        for child in destination.iterdir():
            child.unlink()
        destination.rmdir()
        raise
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--kc-image', required=True)
    parser.add_argument('--kc-mcp-image', required=True)
    parser.add_argument('--rustfs-image', required=True)
    args = parser.parse_args()
    try:
        generate(args.destination, args.kc_image, args.kc_mcp_image, args.rustfs_image)
    except (OSError, ValueError, subprocess.SubprocessError):
        print('Setup failed; use a new nonsymlink destination and check OpenSSL/image references.', file=sys.stderr)
        return 1
    print('Created protected configuration. No accounts or models were registered.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
