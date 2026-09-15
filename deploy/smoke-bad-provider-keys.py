#!/usr/bin/env python3
import http.client
import json
import secrets
import subprocess
import sys
import time


HOST = "127.0.0.1"
PORT = 18080
BASE = f"{HOST}:{PORT}"
CONTAINER = "openbridge-bad-provider-smoke"
TEST_BALANCE = "5.0000"

PROVIDERS = [
    {
        "name": "openai",
        "env": "PROVIDER_OPENAI_API_KEY",
        "model": "gpt-4.1-mini",
    },
    {
        "name": "anthropic",
        "env": "PROVIDER_ANTHROPIC_API_KEY",
        "model": "claude-sonnet-4-6",
    },
    {
        "name": "google",
        "env": "PROVIDER_GOOGLE_API_KEY",
        "model": "gemini-2.5-flash",
    },
]


def run(cmd, input_text=None, check=True):
    proc = subprocess.run(
        cmd,
        input=input_text,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(proc.stderr.strip() or proc.stdout.strip() or "command failed")
    return proc.stdout.strip()


def docker(*args, check=True):
    return run(["sudo", "docker", *args], check=check)


def psql(sql):
    return run(
        [
            "sudo",
            "docker",
            "exec",
            "-i",
            "openbridge-postgres-1",
            "sh",
            "-lc",
            'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -qAt',
        ],
        input_text=sql,
    )


def sql_quote(value):
    return "'" + value.replace("'", "''") + "'"


def request(method, path, body=None, token=None, api_key=None):
    headers = {}
    data = None
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if api_key:
        headers["X-API-Key"] = api_key

    conn = http.client.HTTPConnection(BASE, timeout=90)
    try:
        conn.request(method, path, body=data, headers=headers)
        res = conn.getresponse()
        raw = res.read().decode("utf-8", errors="replace")
        try:
            parsed = json.loads(raw) if raw else None
        except json.JSONDecodeError:
            parsed = None
        return {
            "status": res.status,
            "body": parsed,
            "raw": raw,
        }
    finally:
        conn.close()


def expect(name, fn):
    try:
        fn()
        print(f"PASS {name}")
        return True
    except Exception as exc:
        print(f"FAIL {name}: {exc}")
        return False


def assert_true(value, message):
    if not value:
        raise AssertionError(message)


def stop_sidecar():
    docker("rm", "-f", CONTAINER, check=False)


def start_sidecar(provider):
    stop_sidecar()
    docker(
        "run",
        "-d",
        "--rm",
        "--name",
        CONTAINER,
        "--network",
        "openbridge_app-net",
        "-p",
        f"127.0.0.1:{PORT}:8080",
        "--env-file",
        "/opt/openbridge/.env.production",
        "-e",
        f"{provider['env']}=invalid-provider-key-for-smoke",
        "openbridge-api",
    )
    deadline = time.time() + 45
    while time.time() < deadline:
        try:
            res = request("GET", "/health")
            if res["status"] == 200:
                return
        except Exception:
            time.sleep(1)
    raise RuntimeError("sidecar did not become healthy")


def grant_test_balance(email):
    sql = f"""
WITH user_row AS (
    SELECT id FROM overseas_users WHERE email = {sql_quote(email)}
)
INSERT INTO balances (user_id, balance, frozen_balance, total_recharged, total_consumed)
SELECT id, {TEST_BALANCE}, 0, {TEST_BALANCE}, 0 FROM user_row
ON CONFLICT (user_id) DO UPDATE SET
    balance = {TEST_BALANCE},
    total_recharged = GREATEST(balances.total_recharged, {TEST_BALANCE});
SELECT CAST(balance AS VARCHAR) FROM balances WHERE user_id = (SELECT id FROM overseas_users WHERE email = {sql_quote(email)});
"""
    output = psql(sql)
    assert_true(TEST_BALANCE in output, "test balance not granted")


def cleanup_user(email):
    sql = f"""
UPDATE balances
SET balance = 0, frozen_balance = 0
WHERE user_id = (SELECT id FROM overseas_users WHERE email = {sql_quote(email)});

UPDATE api_keys
SET status = 1, updated_at = NOW()
WHERE user_id = (SELECT id FROM overseas_users WHERE email = {sql_quote(email)});

UPDATE overseas_users
SET status = 1, updated_at = NOW()
WHERE email = {sql_quote(email)};
"""
    psql(sql)


def cleanup_old_smoke_users():
    sql = """
UPDATE balances
SET balance = 0, frozen_balance = 0
WHERE user_id IN (
    SELECT id FROM overseas_users WHERE email LIKE 'bad-provider-smoke-%@openbridgetech.ca'
);

UPDATE api_keys
SET status = 1, updated_at = NOW()
WHERE user_id IN (
    SELECT id FROM overseas_users WHERE email LIKE 'bad-provider-smoke-%@openbridgetech.ca'
);

UPDATE overseas_users
SET status = 1, updated_at = NOW()
WHERE email LIKE 'bad-provider-smoke-%@openbridgetech.ca';
"""
    psql(sql)


def balance_and_cost(email):
    sql = f"""
SELECT
    COALESCE((SELECT CAST(balance AS VARCHAR) FROM balances WHERE user_id = u.id), '0'),
    COALESCE((SELECT CAST(SUM(cost_points) AS VARCHAR) FROM api_calls WHERE user_id = u.id), '0')
FROM overseas_users u
WHERE u.email = {sql_quote(email)};
"""
    output = psql(sql)
    parts = output.split("|")
    if len(parts) < 2:
        raise RuntimeError("balance query failed")
    return parts[0], parts[1]


def run_provider_case(provider):
    stamp = int(time.time() * 1000)
    marker = f"bad-provider-smoke-{provider['name']}-{stamp}"
    email = f"{marker}@openbridgetech.ca"
    password = f"Temp{secrets.token_hex(6)}Aa1!"
    token = ""
    api_key = ""

    try:
        start_sidecar(provider)

        res = request(
            "POST",
            "/auth/register",
            {"email": email, "nickname": marker, "password": password},
        )
        assert_true(res["status"] == 200, f"register HTTP {res['status']}")
        assert_true(res["body"].get("code") == 0, f"register code {res['body'].get('code')}")

        grant_test_balance(email)

        res = request("POST", "/auth/login", {"email": email, "password": password})
        assert_true(res["status"] == 200, f"login HTTP {res['status']}")
        token = res["body"]["data"]["token"]

        res = request(
            "POST",
            "/keys",
            {
                "name": marker,
                "rate_limit": 20,
                "models_allowed": [provider["model"]],
            },
            token=token,
        )
        assert_true(res["status"] == 200, f"key HTTP {res['status']}")
        api_key = res["body"]["data"]["key"]

        res = request(
            "POST",
            "/v1/chat/completions",
            {
                "model": provider["model"],
                "messages": [{"role": "user", "content": marker}],
                "max_tokens": 8,
            },
            api_key=api_key,
        )
        error = (res["body"] or {}).get("error", {})
        assert_true(
            res["status"] == 502,
            f"chat HTTP {res['status']} code={error.get('code')} raw={res['raw'][:300]}",
        )
        assert_true(error.get("code") == "UPSTREAM_API_ERROR", f"error code {error.get('code')}")
        assert_true(
            int(error.get("upstream_status", 0)) in (400, 401, 403, 429),
            f"bad upstream status {error.get('upstream_status')}",
        )

        balance, cost = balance_and_cost(email)
        assert_true(balance == TEST_BALANCE, f"balance changed: {balance}")
        assert_true(cost in ("0", "0.0000"), f"cost changed: {cost}")
    finally:
        cleanup_user(email)


def main():
    passed = 0
    failed = 0
    try:
        cleanup_old_smoke_users()
        for provider in PROVIDERS:
            if expect(f"{provider['name']}_bad_key_no_charge", lambda p=provider: run_provider_case(p)):
                passed += 1
            else:
                failed += 1
        print(f"SUMMARY passed={passed} failed={failed}")
        return 0 if failed == 0 else 1
    finally:
        stop_sidecar()


if __name__ == "__main__":
    sys.exit(main())
