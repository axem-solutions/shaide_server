"""Exercise the isolated Compose demo; never send model inference requests."""
import datetime
import http.cookiejar
import json
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request
import uuid

DEMO = Path(__file__).resolve().parent
PANEL = "http://127.0.0.1:13000/control-panel"
SERVER = "http://127.0.0.1:18080"
COMPOSE = ["docker", "compose", "-f", str(DEMO / "compose.yaml")]
SESSION = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
ANON = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def request(url, method="GET", body=None, token=None, session=SESSION):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    req = urllib.request.Request(url, method=method, headers=headers,
                                 data=None if body is None else json.dumps(body).encode())
    try:
        response = session.open(req, timeout=25)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read().decode()
        return response.status, json.loads(raw) if "application/json" in response.headers.get("Content-Type", "") else raw


def ok(url, **kwargs):
    status, body = request(url, **kwargs)
    assert 200 <= status < 300, (url, status, body)
    return body


def scenario(name):
    (DEMO / "fixtures/scenario.json").write_text(json.dumps({"scenario": name}) + "\n")
    subprocess.run(COMPOSE + ["restart", "shaide_server"], check=True, stdout=subprocess.DEVNULL)
    for _ in range(60):
        try:
            if request(SERVER + "/v1/platform-models", session=ANON)[0] == 401:
                return
        except (OSError, urllib.error.URLError):
            pass
        time.sleep(0.5)
    raise AssertionError("Demo server did not become ready")


def main():
    scenario("healthy")
    assert request(SERVER + "/v1/platform-models", session=ANON)[0] == 401
    assert request(PANEL + "/api/platform-models", session=ANON)[0] == 401
    credentials = {"username": "admin", "password": "admin"}
    admin = ok(SERVER + "/v1/login", method="POST", body=credentials)["access_token"]
    # The generated ordinary user belongs only to the isolated demo database.
    username = "discovery-smoke-" + uuid.uuid4().hex[:8]
    ok(SERVER + "/v1/user", method="POST", token=admin, body={
        "username": username, "password": "demo-only-password",
        "expiry": (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=1)).isoformat(),
    })
    user = ok(SERVER + "/v1/login", method="POST", body={"username": username, "password": "demo-only-password"})["access_token"]
    assert request(SERVER + "/v1/platform-models", token=user)[0] == 401
    ok(PANEL + "/api/login", method="POST", body=credentials)
    page = ok(PANEL + "/models")
    assert "Refresh available models" in page
    catalog = ok(SERVER + "/v1/platform-models", token=admin)
    assert catalog["warnings"] == [] and len(catalog["models"]) == 2, catalog
    models = ok(PANEL + "/api/platform-models")["models"]
    assert {m["name"] for m in models} == {"demo-chat", "demo-embed"}
    for model in models:
        assert model["platform"] == "axem"
        endpoint = model.get("url", model.get("chat_completions_endpoint"))
        assert ".discovery-demo.svc.cluster.local:8200/v1/" in endpoint
    chat = next(m for m in models if m["kind"] == "generative")
    embedding = next(m for m in models if m["kind"] == "embedding")
    assert chat["variant"] == "demo-chat" and chat["context_size"] == 32768
    assert chat["max_generated_tokens"] == 4096 and chat["supports_images"] is True
    assert chat["max_images_per_request"] == 4 and embedding["vector_size"] == 768
    before = ok(PANEL + "/api/models")["models"]
    assert not any(m["name"] in {"demo-chat", "demo-embed"} for m in before), "Remove demo models before running smoke.py"
    try:
        for model in models:
            ok(PANEL + "/api/models", method="POST", body={"kind": model["kind"], "model": model})
        installed = ok(PANEL + "/api/models")["models"]
        assert len(installed) == len(before) + 2, installed
    finally:
        for model in ok(PANEL + "/api/models")["models"]:
            if model["name"] in {"demo-chat", "demo-embed"}:
                ok(PANEL + "/api/models", method="DELETE", body={"kind": model["kind"], "id": model["id"]})
    assert ok(PANEL + "/api/models")["models"] == before
    print("PASS: real login, admin-only discovery, models page, add/remove both model kinds", flush=True)
    for name, expected, error_text in [
        ("missing-metadata", {"demo-embed"}, "metadata"),
        ("missing-dimensions", {"demo-chat"}, "dimensions"),
        ("duplicates", {"demo-embed"}, "multiple"),
        ("empty", set(), None),
    ]:
        scenario(name)
        catalog = ok(PANEL + "/api/platform-models")
        assert {m["name"] for m in catalog["models"]} == expected, catalog
        if error_text:
            assert error_text in catalog.get("error", "").lower(), catalog
            page = ok(PANEL + "/models")
            assert "MuiAlert-outlinedError" in page and error_text in page.lower()
        else:
            assert not catalog.get("error"), catalog
        print("PASS: " + name, flush=True)
    logs = subprocess.check_output(COMPOSE + ["logs", "--no-color", "platform-mock"], text=True)
    assert "POST " not in logs, "Unexpected inference or other POST to platform mock"
    assert "/server_info?config_format=json" in logs and "/v1/models" in logs
    print("PASS: metadata GETs only; no inference probes", flush=True)


if __name__ == "__main__":
    try:
        main()
    finally:
        scenario("healthy")
