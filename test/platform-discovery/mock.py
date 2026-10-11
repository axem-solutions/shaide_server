"""Local discovery fixtures only. No inference endpoints are implemented."""

import json
import ssl
import sys
import threading
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

SCENARIOS = {"healthy", "missing-metadata", "missing-dimensions", "duplicates", "empty"}
SELECTOR = "app.kubernetes.io/part-of=app-serving,axem.dev/model-slug,axem.dev/model-category"


def service(name, category):
    return {
        "apiVersion": "v1", "kind": "Service",
        "metadata": {"name": name, "namespace": "discovery-demo", "labels": {
            "app.kubernetes.io/part-of": "app-serving",
            "axem.dev/model-slug": name, "axem.dev/model-category": category,
        }},
        "spec": {"type": "ExternalName" if category == "generative" else "ClusterIP",
                 "ports": [{"name": "http", "port": 8200, "protocol": "TCP"}],
                 **({"externalName": "unused.gateway.internal"} if category == "generative" else {})},
    }


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        # Never log headers or service-account credentials.
        print(f"{self.command} {self.path} host={self.headers.get('Host', '')} " + fmt % args, flush=True)

    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        url = urlsplit(self.path)
        if self.server.server_port == 8443:
            if url.path == "/healthz":
                return self.reply(200, {"ok": True})
            if self.headers.get("Authorization") != "Bearer mock-service-account":
                return self.reply(401, {"error": "Fake service-account token required"})
        try:
            scenario = json.loads(Path("/fixtures/scenario.json").read_text())["scenario"]
            if scenario not in SCENARIOS:
                raise ValueError("Unknown scenario")
        except (ValueError, KeyError, OSError) as error:
            return self.reply(500, {"error": str(error)})
        if self.server.server_port == 8443:
            if url.path != "/api/v1/services":
                return self.reply(404, {"error": "Unknown Kubernetes endpoint"})
            if parse_qs(url.query).get("labelSelector") != [SELECTOR]:
                return self.reply(400, {"error": "Unexpected discovery selector"})
            items = [] if scenario == "empty" else [service("mock-chat", "generative"), service("mock-embed", "embedder")]
            if scenario == "duplicates":
                items.append(service("mock-chat-copy", "generative"))
            return self.reply(200, {"apiVersion": "v1", "kind": "ServiceList", "metadata": {"resourceVersion": "1"}, "items": items})
        host = self.headers.get("Host", "").split(":")[0]
        if host not in {f"{name}.discovery-demo.svc.cluster.local" for name in ("mock-chat", "mock-chat-copy", "mock-embed")}:
            return self.reply(404, {"error": "Unknown model service"})
        embedding = host.startswith("mock-embed.")
        name = "demo-embed" if embedding else "demo-chat"
        if url.path == "/v1/models":
            return self.reply(200, {"object": "list", "data": [{"id": name, "parent": None}]})
        if url.path == "/v1/shaide/metadata":
            if scenario == "missing-metadata" and not embedding:
                return self.reply(404, {"error": "Runtime metadata unavailable"})
            config = {"model": f"/weights/{name}", "served_model_name": [name], "max_model_len": 32768}
            if embedding:
                if scenario != "missing-dimensions":
                    config["pooler_config"] = {"dimensions": 768}
            else:
                config.update(override_generation_config={"max_new_tokens": 4096},
                              multimodal_config={"limit_per_prompt": {"image": 4}})
            return self.reply(200, {"schema_version": 1, "vllm_config": {"model_config": config}})
        return self.reply(404, {"error": "Unknown metadata endpoint"})

    def do_POST(self):
        self.reply(501, {"error": "Discovery mock only; inference is not implemented"})


if __name__ == "__main__":
    if sys.argv[1:] == ["healthcheck"]:
        with urllib.request.urlopen("https://localhost:8443/healthz", context=ssl.create_default_context(cafile="/serviceaccount/ca.crt"), timeout=1) as response:
            assert response.status == 200
    else:
        http = ThreadingHTTPServer(("0.0.0.0", 8200), Handler)
        https = ThreadingHTTPServer(("0.0.0.0", 8443), Handler)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain("/tmp/mock.crt", "/tmp/mock.key")
        https.socket = context.wrap_socket(https.socket, server_side=True)
        threading.Thread(target=http.serve_forever, daemon=True).start()
        https.serve_forever()
