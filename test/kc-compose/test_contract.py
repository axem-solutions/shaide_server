"""No Cargo, image builds or running-stack mutations; disposable setup/config checks."""
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('setup', ROOT / 'scripts/kc/setup.py')
setup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(setup)

class SetupTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=os.environ.get('TMPDIR'))
        self.root = Path(self.tmp.name)
        self.dest = self.root / 'credentials'
    def tearDown(self):
        self.tmp.cleanup()
    def generate(self):
        setup.generate(self.dest, 'local/kc:operator', 'local/mcp:operator', 'local/rustfs:operator')
    def test_new_destination_modes_and_unique_tokens(self):
        with patch.dict(os.environ, {}):
            old = os.umask(0)
            try: self.generate()
            finally: os.umask(old)
        self.assertEqual(stat.S_IMODE(self.dest.stat().st_mode), 0o700)
        for p in self.dest.iterdir():
            self.assertEqual(stat.S_IMODE(p.stat().st_mode), 0o600)
        values = [(self.dest / n).read_text() for n in setup.TOKENS]
        self.assertEqual(len(values), len(set(values)))
        self.assertTrue(all(len(v.strip()) >= 32 for v in values))
    def test_exact_cli_setup_keeps_secrets_off_output(self):
        result = subprocess.run(['python3', str(ROOT / 'scripts/kc/setup.py'), str(self.dest),
                                 '--kc-image', 'local/kc:operator', '--kc-mcp-image', 'local/mcp:operator',
                                 '--rustfs-image', 'local/rustfs:operator'], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in setup.TOKENS:
            self.assertNotIn((self.dest / name).read_text().strip(), result.stdout + result.stderr)
        again = subprocess.run(['python3', str(ROOT / 'scripts/kc/setup.py'), str(self.dest),
                                '--kc-image', 'local/kc:operator', '--kc-mcp-image', 'local/mcp:operator',
                                '--rustfs-image', 'local/rustfs:operator'], capture_output=True, text=True)
        self.assertNotEqual(again.returncode, 0)

    def test_existing_directory_untouched(self):
        self.dest.mkdir(mode=0o755)
        p = self.dest / 'admin_password'; p.write_text('keep'); p.chmod(0o644)
        with self.assertRaises(FileExistsError): self.generate()
        self.assertEqual(p.read_text(), 'keep')
        self.assertEqual(stat.S_IMODE(p.stat().st_mode), 0o644)
    def test_existing_file_untouched(self):
        self.dest.write_text('keep'); self.dest.chmod(0o644)
        with self.assertRaises(FileExistsError): self.generate()
        self.assertEqual(self.dest.read_text(), 'keep')
        self.assertEqual(stat.S_IMODE(self.dest.stat().st_mode), 0o644)
    def test_symlinks_and_dangling_symlinks(self):
        for target in [self.root / 'absent', self.root]:
            with self.subTest(target=target):
                self.dest.symlink_to(target)
                with self.assertRaises((FileExistsError, ValueError)): self.generate()
                self.assertTrue(self.dest.is_symlink()); self.dest.unlink()
    def test_symlink_parent_rejected(self):
        link = self.root / 'link'; link.symlink_to(self.root)
        with self.assertRaises(ValueError):
            setup.generate(link / 'credentials', 'kc', 'mcp', 'rustfs')
        self.assertFalse(self.dest.exists())
    def test_key_generation_failure_cleans_owned_files(self):
        with patch.object(setup.subprocess, 'run', side_effect=RuntimeError('injected failure')):
            with self.assertRaises(RuntimeError): self.generate()
        self.assertFalse(self.dest.exists())
    def test_partial_write_failure_cleanup(self):
        real = setup.write_exclusive
        def fail(path, content):
            if path.name == 'jwt_secret': raise OSError('injected failure')
            return real(path, content)
        with patch.object(setup, 'write_exclusive', side_effect=fail):
            with self.assertRaises(OSError): self.generate()
        self.assertFalse(self.dest.exists())
    def test_exclusive_writer_preserves_existing_file_and_symlink(self):
        p = self.root / 'file'; p.write_text('keep'); p.chmod(0o644)
        for path in [p, self.root / 'link']:
            if path != p: path.symlink_to(p)
            with self.assertRaises(FileExistsError): setup.write_exclusive(path, b'replace')
        self.assertEqual(p.read_text(), 'keep')
        self.assertEqual(stat.S_IMODE(p.stat().st_mode), 0o644)
    def test_invalid_image_does_not_create_destination(self):
        with self.assertRaises(ValueError):
            setup.generate(self.dest, 'kc\nINJECT=1', 'mcp', 'rustfs')
        self.assertFalse(self.dest.exists())

class ComposeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=os.environ.get('TMPDIR'))
        self.dest = Path(self.tmp.name) / 'credentials'
        setup.generate(self.dest, 'local/kc:operator', 'local/mcp:operator', 'local/rustfs:operator')
    def tearDown(self): self.tmp.cleanup()
    def compose(self, overlay=True, env=True):
        args = ['docker', 'compose']
        if env: args += ['--env-file', str(self.dest / 'config.env')]
        args += ['-f', str(ROOT / 'compose.yaml')]
        if overlay: args += ['-f', str(ROOT / 'compose.kc.yaml')]
        args += ['--profile', 'server', 'config', '--format', 'json']
        clean = {k: v for k, v in os.environ.items() if not k.startswith(('KC_', 'SHAIDE_', 'MCP_', 'RUSTFS_'))}
        return subprocess.run(args, capture_output=True, text=True, env=clean)
    def model(self):
        r = self.compose(); self.assertEqual(r.returncode, 0, r.stderr); return json.loads(r.stdout)
    def test_no_kc_needs_no_kc_variables(self):
        r = self.compose(False, False); self.assertEqual(r.returncode, 0, r.stderr)
        m = json.loads(r.stdout); self.assertNotIn('knowledge-center', m['services'])
        self.assertEqual(m['services']['vectordb']['ports'][0]['published'], '6333')
    def test_missing_optional_settings_fail_only_overlay(self):
        r = self.compose(True, False); self.assertNotEqual(r.returncode, 0)
        self.assertIn('required', r.stderr)
    def test_only_server_has_host_ports(self):
        m = self.model()
        for name, svc in m['services'].items():
            if name != 'shaide_server': self.assertFalse(svc.get('ports'), name)
        self.assertEqual(m['services']['shaide_server']['ports'][0]['host_ip'], '127.0.0.1')
    def test_storage_and_vectors_are_isolated_from_server_and_mcp(self):
        m = self.model(); s = m['services']
        for backend in ['kc-rustfs', 'kc-qdrant']:
            for facade in ['shaide_server', 'kc-mcp']:
                self.assertTrue(set(s[backend]['networks']).isdisjoint(s[facade]['networks']))
        self.assertNotIn('default', s['knowledge-center']['networks'])
        for name, net in m['networks'].items():
            if name != 'default': self.assertTrue(net['internal'], name)
    def test_server_has_no_storage_credentials_or_mounts(self):
        m = self.model(); s = m['services']['shaide_server']
        self.assertFalse(any(k.startswith(('S3_', 'RUSTFS_', 'AWS_')) and k != 'S3_WEBHOOK_TOKEN' for k in s['environment']))
        self.assertFalse(any('storage' in x['source'] for x in s['secrets']))
    def test_compose_output_contains_no_secret_values(self):
        output = self.compose().stdout
        for n in setup.TOKENS:
            self.assertNotIn((self.dest / n).read_text().strip(), output)
    def test_named_state_and_no_build_contract(self):
        m = self.model(); s = m['services']
        for service in ['shaide_server', 'kc-rustfs', 'kc-qdrant']:
            self.assertTrue(any(v['type'] == 'volume' for v in s[service]['volumes']))
        for name in ['knowledge-center', 'kc-mcp', 'kc-rustfs']:
            self.assertNotIn('build', s[name]); self.assertEqual(s[name]['pull_policy'], 'never')
        self.assertEqual(s['shaide_server']['environment']['VECTOR_DB_URL'], 'http://vectordb:6334')
    def test_signed_context_and_notification_trust(self):
        s = self.model()['services']; env = s['shaide_server']['environment']
        targets = json.loads(env['MCP_COLLECTION_CONTEXT_TARGETS'])
        self.assertEqual(targets[0]['url'], 'http://kc-mcp:8080')
        self.assertEqual(env['KNOWLEDGE_CENTER_URL'], 'http://knowledge-center:8080')
        self.assertIn('kc-notifications:8080/v1/object-storage/event', s['kc-rustfs']['environment']['RUSTFS_NOTIFY_WEBHOOK_ENDPOINT_SHAIDE'])
        self.assertEqual(s['knowledge-center']['environment']['RUSTFS_WEBHOOK_ARN'], 'arn:rustfs:sqs:eu-central-1:shaide:webhook')

if __name__ == '__main__': unittest.main()
