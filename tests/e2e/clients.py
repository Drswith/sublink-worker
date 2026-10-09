#!/usr/bin/env python3
"""End-to-end check that real clients accept and run the configs we generate.

Local sing-box servers expose every supported protocol. The script asks a
running sublink-worker for client configs built from their share links (and
from subscription URLs), validates them with the official `sing-box check` /
`mihomo -t`, starts the clients and sends traffic through them. A case passes
only when the target receives the request *and* the proxy server logged the
connection on one of its inbounds (the node's own inbound for single-node
cases), so a config that silently routes DIRECT fails. Surge has no Linux
client and is not covered.

Usage: python3 tests/e2e/clients.py [--worker http://127.0.0.1:38471] [--cache DIR] [--only SUBSTR]

Clients (pinned versions below) and geodata are downloaded into the cache on
first use. Two adaptations make the generated configs runnable offline and
unprivileged: TUN inbounds are dropped, and remote rule-set URLs point to a
local mirror of the same files.
"""

import argparse
import base64
import gzip
import hashlib
import http.server
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import urllib.parse
import urllib.request
import uuid as uuidlib

SINGBOX_VERSIONS = ["1.11.15", "1.12.25", "1.13.21", "1.14.2"]
MIHOMO_VERSION = "1.19.32"
GEODATA = {
    "GeoSite.dat": "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat",
    "GeoIP.dat": "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat",
    "geoip.metadb": "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb",
}
TARGET_HOST = "e2e.example"  # resolved only by the proxy server, so traffic must go through it
SINGBOX_CLIENT_PORT = 2080  # mixed inbound of the generated sing-box config
MIHOMO_SOCKS_PORT = 7891  # socks-port of the generated Clash config


def log(msg):
    print(msg, flush=True)


def download(url, dest):
    with urllib.request.urlopen(url, timeout=300) as r, open(dest + ".part", "wb") as f:
        shutil.copyfileobj(r, f)
    os.replace(dest + ".part", dest)


def ensure_clients(cache):
    os.makedirs(cache, exist_ok=True)
    for v in SINGBOX_VERSIONS:
        exe = os.path.join(cache, f"sing-box-{v}")
        if not os.path.exists(exe):
            log(f"downloading sing-box {v}")
            tgz = exe + ".tar.gz"
            download(f"https://github.com/SagerNet/sing-box/releases/download/v{v}/sing-box-{v}-linux-amd64.tar.gz", tgz)
            with tarfile.open(tgz) as t:
                member = t.getmember(f"sing-box-{v}-linux-amd64/sing-box")
                with t.extractfile(member) as src, open(exe, "wb") as dst:
                    shutil.copyfileobj(src, dst)
            os.chmod(exe, 0o755)
            os.remove(tgz)
    mihomo = os.path.join(cache, "mihomo")
    if not os.path.exists(mihomo):
        log(f"downloading mihomo {MIHOMO_VERSION}")
        gz = mihomo + ".gz"
        download(
            f"https://github.com/MetaCubeX/mihomo/releases/download/v{MIHOMO_VERSION}/mihomo-linux-amd64-v{MIHOMO_VERSION}.gz", gz
        )
        with gzip.open(gz) as src, open(mihomo, "wb") as dst:
            shutil.copyfileobj(src, dst)
        os.chmod(mihomo, 0o755)
        os.remove(gz)
    for name, url in GEODATA.items():
        path = os.path.join(cache, name)
        if not os.path.exists(path):
            log(f"downloading {name}")
            download(url, path)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def wait_port(port, proc, timeout=15):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if proc.poll() is not None:
            return False
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return True
        except OSError:
            time.sleep(0.2)
    return False


def wait_port_free(port, timeout=10):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.3):
                time.sleep(0.2)
        except OSError:
            return


def serve(handler_cls):
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler_cls)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


class Target(http.server.BaseHTTPRequestHandler):
    """Destination of proxied requests; also hosts subscription files."""

    files = {}

    def do_GET(self):
        body = self.files.get(self.path)
        if body is None:
            body = f"ok {self.path}".encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


class Mirror(http.server.BaseHTTPRequestHandler):
    """Serves rule-set files fetched once from their real URLs."""

    cache = ""
    sources = {}

    def do_GET(self):
        key = self.path.strip("/")
        path = os.path.join(self.cache, key)
        if not os.path.exists(path) and key in self.sources:
            fetch_rule_set(self.sources[key], path)
        if not os.path.exists(path):
            self.send_error(404)
            return
        with open(path, "rb") as f:
            body = f.read()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def fetch_rule_set(url, path):
    # The generated URLs go through gh-proxy.com; fall back to GitHub itself
    # and to raw.githubusercontent.com when a network blocks either.
    direct = url[len("https://gh-proxy.com/") :] if url.startswith("https://gh-proxy.com/") else url
    raw = re.sub(r"^https://github\.com/([^/]+)/([^/]+)/raw/", r"https://raw.githubusercontent.com/\1/\2/", direct)
    candidates = list(dict.fromkeys([url, direct, raw]))
    for candidate in candidates:
        try:
            download(candidate, path)
            return
        except Exception as e:  # try the next mirror
            log(f"  rule-set fetch failed {candidate}: {e}")


def mirror_url(mirror_base, url):
    key = hashlib.sha1(url.encode()).hexdigest() + os.path.splitext(urllib.parse.urlparse(url).path)[1]
    Mirror.sources[key] = url
    return f"{mirror_base}/{key}"


def adapt_singbox(config, mirror_base):
    config["inbounds"] = [i for i in config.get("inbounds", []) if i.get("type") != "tun"]
    for rs in config.get("route", {}).get("rule_set", []):
        if rs.get("type") == "remote":
            rs["url"] = mirror_url(mirror_base, rs["url"])
    return config


def adapt_clash(text, mirror_base):
    def repl(m):
        url = m.group(2)
        if url.startswith("http://127.0.0.1"):
            return m.group(0)
        return m.group(1) + mirror_url(mirror_base, url)

    # js-yaml folds long scalars, so a URL may follow `url: >-` on the next line.
    return re.sub(r"^(\s+url: )(?:>-\n\s+)?(https?://\S+)$", repl, text, flags=re.M)


def b64(s):
    return base64.b64encode(s.encode()).decode()


def b64url(s):
    return base64.urlsafe_b64encode(s.encode()).decode().rstrip("=")


def build_servers(sb, work):
    """sing-box server config with one inbound per protocol, plus share links."""
    out = subprocess.run([sb, "generate", "tls-keypair", "e2e.test"], capture_output=True, text=True, check=True).stdout
    cert = re.search(r"-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----", out, re.S).group(0)
    key = re.search(r"-----BEGIN PRIVATE KEY-----.*?-----END PRIVATE KEY-----", out, re.S).group(0)
    cert_path, key_path = os.path.join(work, "cert.pem"), os.path.join(work, "key.pem")
    open(cert_path, "w").write(cert)
    open(key_path, "w").write(key)
    reality = subprocess.run([sb, "generate", "reality-keypair"], capture_output=True, text=True, check=True).stdout
    private_key = re.search(r"PrivateKey: (\S+)", reality).group(1)
    public_key = re.search(r"PublicKey: (\S+)", reality).group(1)
    tls = {"enabled": True, "server_name": "e2e.test", "certificate_path": cert_path, "key_path": key_path}
    uid = str(uuidlib.uuid4())
    ss2022_key = base64.b64encode(os.urandom(16)).decode()
    port = iter(range(21001, 21100))

    cases = []

    def add(name, inbound, link):
        inbound = {"tag": name, "listen": "127.0.0.1", "listen_port": next(port), **inbound}
        # vmess links are base64 JSON, so they are built from the port instead of patched.
        port_str = str(inbound["listen_port"])
        cases.append((name, inbound, link(port_str) if callable(link) else link.replace("PORT", port_str)))

    add("ss", {"type": "shadowsocks", "method": "aes-128-gcm", "password": "test"},
        f"ss://{b64url('aes-128-gcm:test')}@127.0.0.1:PORT#ss")
    add("ss2022", {"type": "shadowsocks", "method": "2022-blake3-aes-128-gcm", "password": ss2022_key},
        f"ss://{b64url('2022-blake3-aes-128-gcm:' + ss2022_key)}@127.0.0.1:PORT#ss2022")
    vmess = lambda extra: lambda p: "vmess://" + b64(json.dumps({"v": "2", "add": "127.0.0.1", "port": p, "id": uid, "aid": "0", **extra}))
    add("vmess-tcp", {"type": "vmess", "users": [{"uuid": uid, "alterId": 0}]},
        vmess({"ps": "vmess-tcp", "net": "tcp", "type": "none", "tls": ""}))
    add("vmess-ws-tls", {"type": "vmess", "users": [{"uuid": uid, "alterId": 0}], "tls": tls, "transport": {"type": "ws", "path": "/vm"}},
        vmess({"ps": "vmess-ws-tls", "net": "ws", "path": "/vm", "host": "e2e.test", "tls": "tls", "sni": "e2e.test", "skip-cert-verify": True}))
    add("vless-ws", {"type": "vless", "users": [{"uuid": uid}], "transport": {"type": "ws", "path": "/vl"}},
        f"vless://{uid}@127.0.0.1:PORT?encryption=none&security=none&type=ws&path=%2Fvl&host=e2e.test#vless-ws")
    add("trojan", {"type": "trojan", "users": [{"password": "pass"}], "tls": tls},
        "trojan://pass@127.0.0.1:PORT?security=tls&sni=e2e.test&allowInsecure=1#trojan")
    trojan_port = cases[-1][1]["listen_port"]
    add("vless-reality", {"type": "vless", "users": [{"uuid": uid, "flow": "xtls-rprx-vision"}],
        "tls": {"enabled": True, "server_name": "e2e.test", "reality": {
            "enabled": True, "handshake": {"server": "127.0.0.1", "server_port": trojan_port},
            "private_key": private_key, "short_id": ["abcd1234"]}}},
        f"vless://{uid}@127.0.0.1:PORT?encryption=none&flow=xtls-rprx-vision&security=reality&sni=e2e.test&fp=chrome&pbk={public_key}&sid=abcd1234&type=tcp#vless-reality")
    add("hysteria2", {"type": "hysteria2", "users": [{"password": "hy2pass"}], "tls": tls},
        "hysteria2://hy2pass@127.0.0.1:PORT?sni=e2e.test&insecure=1#hysteria2")
    add("tuic", {"type": "tuic", "users": [{"uuid": uid, "password": "tuicpass"}], "congestion_control": "bbr", "tls": {**tls, "alpn": ["h3"]}},
        f"tuic://{uid}:tuicpass@127.0.0.1:PORT?congestion_control=bbr&alpn=h3&sni=e2e.test&allow_insecure=1#tuic")
    add("anytls", {"type": "anytls", "users": [{"password": "anypass"}], "tls": tls},
        "anytls://anypass@127.0.0.1:PORT/?sni=e2e.test&insecure=1#anytls")

    config = {
        "log": {"level": "info", "output": os.path.join(work, "server.log")},
        "dns": {"servers": [{"type": "hosts", "tag": "hosts", "predefined": {TARGET_HOST: "127.0.0.1"}}]},
        "inbounds": [c[1] for c in cases],
        "outbounds": [{"type": "direct", "tag": "direct"}],
        "route": {"default_domain_resolver": "hosts", "final": "direct"},
    }
    return config, [(name, link) for name, _, link in cases]


class Run:
    def __init__(self, args):
        self.args = args
        self.results = []

    def generate(self, route, params):
        url = f"{self.args.worker}/{route}?{urllib.parse.urlencode(params)}"
        with urllib.request.urlopen(url, timeout=60) as r:
            return r.read().decode()

    def record(self, name, ok, detail=""):
        self.results.append((name, ok, detail))
        log(f"{'PASS' if ok else 'FAIL'}  {name}{'  ' + detail if detail else ''}")

    def through_proxy(self, socks_port, inbound_tag, token):
        """Request the target via the client and confirm the server proxied it.

        inbound_tag None accepts any node: url-test groups may switch to the
        fastest one once health checks finish."""
        before = os.path.getsize(self.server_log) if os.path.exists(self.server_log) else 0
        url = f"http://{TARGET_HOST}:{self.target_port}/{token}"
        # Clients accept connections before rule sets and remote proxy providers
        # finish loading; mihomo needs ~10s before provider nodes join the groups.
        deadline = time.monotonic() + 30
        while True:
            r = subprocess.run(["curl", "-s", "--max-time", "10", "-x", f"socks5h://127.0.0.1:{socks_port}", url],
                               capture_output=True, text=True)
            if f"ok /{token}" in r.stdout or time.monotonic() > deadline:
                break
            time.sleep(1)
        time.sleep(0.3)
        with open(self.server_log, errors="replace") as f:
            f.seek(before)
            fresh = f.read()
        # Multi-user inbounds add the user index: "inbound/vless[tag]: [0] inbound connection to ...".
        tags = re.findall(rf"\[([^\]]+)\]: (?:\[\d+\] )?inbound connection to {re.escape(TARGET_HOST)}:", fresh)
        if f"ok /{token}" not in r.stdout:
            return False, f"no response through client (curl exit {r.returncode})"
        if not tags or (inbound_tag and inbound_tag not in tags):
            return False, f"response did not pass through inbound {inbound_tag or '(any)'}"
        return True, f"via {tags[-1]}"

    def run_client(self, cmd, port, workdir):
        out = open(os.path.join(workdir, "client.log"), "w")
        proc = subprocess.Popen(cmd, stdout=out, stderr=subprocess.STDOUT, cwd=workdir)
        if not wait_port(port, proc):
            proc.kill()
            proc.wait()
            out.close()
            return None, open(os.path.join(workdir, "client.log")).read()[-600:]
        return proc, ""

    def stop(self, proc, port):
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        wait_port_free(port)

    def singbox_case(self, name, version, params, inbound_tag, traffic=True):
        sb = os.path.join(self.args.cache, f"sing-box-{version}")
        workdir = tempfile.mkdtemp(prefix="sb-", dir=self.work)
        config = adapt_singbox(json.loads(self.generate("singbox", {**params, "sb_ver": version})), self.mirror)
        path = os.path.join(workdir, "config.json")
        json.dump(config, open(path, "w"), ensure_ascii=False, indent=2)
        check = subprocess.run([sb, "check", "-c", path], capture_output=True, text=True)
        if check.returncode != 0:
            return self.record(name, False, "check: " + check.stderr.strip()[-400:])
        if not traffic:
            return self.record(name, True)
        proc, err = self.run_client([sb, "run", "-c", path, "-D", workdir], SINGBOX_CLIENT_PORT, workdir)
        if proc is None:
            return self.record(name, False, "client did not start: " + err)
        try:
            ok, detail = self.through_proxy(SINGBOX_CLIENT_PORT, inbound_tag, re.sub(r"\W", "_", name))
        finally:
            self.stop(proc, SINGBOX_CLIENT_PORT)
        self.record(name, ok, detail)

    def mihomo_case(self, name, params, inbound_tag, traffic=True):
        home = tempfile.mkdtemp(prefix="mh-", dir=self.work)
        for f in GEODATA:
            shutil.copy(os.path.join(self.args.cache, f), home)
        path = os.path.join(home, "config.yaml")
        open(path, "w").write(adapt_clash(self.generate("clash", params), self.mirror))
        mihomo = os.path.join(self.args.cache, "mihomo")
        check = subprocess.run([mihomo, "-t", "-d", home, "-f", path], capture_output=True, text=True)
        if check.returncode != 0:
            return self.record(name, False, "mihomo -t: " + (check.stdout + check.stderr).strip()[-400:])
        if not traffic:
            return self.record(name, True)
        proc, err = self.run_client([mihomo, "-d", home, "-f", path], MIHOMO_SOCKS_PORT, home)
        if proc is None:
            return self.record(name, False, "client did not start: " + err)
        try:
            ok, detail = self.through_proxy(MIHOMO_SOCKS_PORT, inbound_tag, re.sub(r"\W", "_", name))
        finally:
            self.stop(proc, MIHOMO_SOCKS_PORT)
        self.record(name, ok, detail)

    def wanted(self, name):
        return not self.args.only or self.args.only in name

    def main(self):
        ensure_clients(self.args.cache)
        self.work = tempfile.mkdtemp(prefix="sublink-e2e-")
        Mirror.cache = os.path.join(self.args.cache, "rule-sets")
        os.makedirs(Mirror.cache, exist_ok=True)
        self.mirror = f"http://127.0.0.1:{serve(Mirror).server_address[1]}"
        self.target_port = serve(Target).server_address[1]
        newest = os.path.join(self.args.cache, f"sing-box-{SINGBOX_VERSIONS[-1]}")
        server_config, links = build_servers(newest, self.work)
        self.server_log = server_config["log"]["output"]
        server_path = os.path.join(self.work, "server.json")
        json.dump(server_config, open(server_path, "w"), indent=2)
        server = subprocess.Popen([newest, "run", "-c", server_path], stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT)
        if not wait_port(21001, server):
            sys.exit("proxy server did not start")
        try:
            self.cases(links)
        finally:
            server.terminate()
            server.wait()
        failed = [r for r in self.results if not r[1]]
        log(f"\n{len(self.results) - len(failed)}/{len(self.results)} passed")
        if failed:
            log("failed: " + ", ".join(r[0] for r in failed))
        return 1 if failed else 0

    def cases(self, links):
        # Every protocol through every client, with default options.
        for proto, link in links:
            for version in SINGBOX_VERSIONS:
                if self.wanted(f"sing-box-{version}/{proto}"):
                    self.singbox_case(f"sing-box-{version}/{proto}", version, {"config": link}, proto)
            if self.wanted(f"mihomo/{proto}"):
                self.mihomo_case(f"mihomo/{proto}", {"config": link}, proto)

        # All protocols at once with every option switched on.
        all_links = "\n".join(link for _, link in links)
        custom = json.dumps([{"name": "E2E", "domain_suffix": "example.org", "ip_cidr": "10.0.0.0/8", "protocol": "bittorrent"}])
        full = {"config": all_links, "selectedRules": "comprehensive", "customRules": custom, "group_by_country": "true",
                "enable_clash_ui": "true", "include_auto_select": "false"}
        for version in SINGBOX_VERSIONS:
            if self.wanted(f"sing-box-{version}/all-options"):
                self.singbox_case(f"sing-box-{version}/all-options", version, full, None)
        if self.wanted("mihomo/all-options"):
            self.mihomo_case("mihomo/all-options", full, None)

        # Remote subscriptions in each format, fetched by the worker from a local URL.
        Target.files["/sub/base64"] = b64(all_links).encode()
        Target.files["/sub/clash"] = self.generate("clash", {"config": all_links}).encode()
        Target.files["/sub/singbox"] = self.generate("singbox", {"config": all_links}).encode()
        for kind in ["base64", "clash", "singbox"]:
            sub = {"config": f"http://127.0.0.1:{self.target_port}/sub/{kind}"}
            for version in SINGBOX_VERSIONS:
                if self.wanted(f"sing-box-{version}/sub-{kind}"):
                    self.singbox_case(f"sing-box-{version}/sub-{kind}", version, sub, None)
            if self.wanted(f"mihomo/sub-{kind}"):
                self.mihomo_case(f"mihomo/sub-{kind}", sub, None)

        # Xray output is the share links themselves, Base64-wrapped.
        if self.wanted("xray"):
            decoded = base64.b64decode(self.generate("xray", {"config": all_links})).decode()
            self.record("xray/roundtrip", decoded == all_links, "" if decoded == all_links else "decoded links differ from input")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--worker", default="http://127.0.0.1:38471")
    parser.add_argument("--cache", default=os.path.expanduser("~/.cache/sublink-e2e"))
    parser.add_argument("--only", help="run cases whose name contains this text")
    sys.exit(Run(parser.parse_args()).main())
