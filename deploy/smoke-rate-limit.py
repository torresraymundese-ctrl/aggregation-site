#!/usr/bin/env python3
import http.client
import json
import secrets
import ssl
import subprocess
import sys
import time


DOMAIN = "openbridgetech.ca"
MODEL = "gpt-4.1-mini"


def request(method, path, body=None, token=None, api_key=None, direct_api=False):
    headers = {"Host": DOMAIN}
    if body is not None:
        body = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if api_key:
        headers["X-API-Key"] = api_key

    if direct_api:
        conn = http.client.HTTPConnection("127.0.0.1", 8080, timeout=20)
    else:
        context = ssl._create_unverified_context()
        conn = http.client.HTTPSConnection("127.0.0.1", 443, timeout=20, context=context)

    try:
        conn.request(method, path, body=body, headers=headers)
        res = conn.getresponse()
        raw = res.read().decode("utf-8", errors="replace")
        try:
            parsed = json.loads(raw) if raw else None
        except json.JSONDecodeError:
            parsed = None
        return {
            "status": res.status,
            "headers": {k.lower(): v for k, v in res.getheaders()},
            "body": parsed,
            "raw": raw,
        }
    finally:
        conn.close()


def expect(name, check):
    try:
        check()
        print(f"PASS {name}")
        return True
    except Exception as exc:
        print(f"FAIL {name}: {exc}")
        return False


def assert_true(value, message):
    if not value:
        raise AssertionError(message)


def sql_quote(value):
    return "'" + value.replace("'", "''") + "'"


def psql(sql):
    proc = subprocess.run(
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
        input=sql,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.strip() or "psql failed")
    return proc.stdout.strip()


def has_numeric_line(output):
    return any(line.strip().isdigit() for line in output.splitlines())


def main():
    stamp = int(time.time() * 1000)
    secret = secrets.token_hex(6)
    email = f"ratelimit-{stamp}@openbridgetech.ca"
    password = f"Temp{secret}Aa1!"
    nickname = f"ratelimit-{stamp}"
    token = ""
    api_key = ""
    key_uid = ""
    spend_api_key = ""
    spend_key_uid = ""
    synthetic_request_id = f"smokedaily{stamp}"[-32:]
    passed = 0
    failed = 0

    def run(name, check):
        nonlocal passed, failed
        if expect(name, check):
            passed += 1
        else:
            failed += 1

    def health():
        res = request("GET", "/api/health")
        assert_true(res["status"] == 200, f"HTTP {res['status']}")

    def cleanup_old_synthetic_spend():
        output = psql(
            "WITH deleted AS ("
            "DELETE FROM api_calls WHERE error_msg = 'synthetic daily spend smoke' RETURNING id"
            ") SELECT COUNT(*) FROM deleted;"
        )
        assert_true(has_numeric_line(output), "old synthetic spend cleanup failed")

    def register():
        res = request(
            "POST",
            "/auth/register",
            {"email": email, "nickname": nickname, "password": password},
            direct_api=True,
        )
        assert_true(res["status"] == 200, f"HTTP {res['status']}")
        assert_true(res["body"].get("code") == 0, f"code {res['body'].get('code')}")

    def login():
        nonlocal token
        res = request("POST", "/api/auth/login", {"email": email, "password": password})
        assert_true(res["status"] == 200, f"HTTP {res['status']}")
        assert_true(res["body"].get("code") == 0, f"code {res['body'].get('code')}")
        token = res["body"]["data"]["token"]

    def create_key():
        nonlocal api_key, key_uid
        res = request(
            "POST",
            "/api/keys",
            {
                "name": f"rate-limit-smoke-{stamp}",
                "rate_limit": 2,
                "models_allowed": [MODEL],
                "daily_spend_limit": 0.001,
            },
            token=token,
        )
        assert_true(res["status"] == 200, f"HTTP {res['status']}")
        assert_true(res["body"].get("code") == 0, f"code {res['body'].get('code')}")
        data = res["body"]["data"]
        api_key = data["key"]
        key_uid = data["uid"]
        assert_true(data.get("daily_spend_limit") == 0.001, "daily_spend_limit missing")

    def create_spend_key():
        nonlocal spend_api_key, spend_key_uid
        res = request(
            "POST",
            "/api/keys",
            {
                "name": f"daily-spend-smoke-{stamp}",
                "rate_limit": 10,
                "models_allowed": [MODEL],
                "daily_spend_limit": 0.001,
            },
            token=token,
        )
        assert_true(res["status"] == 200, f"HTTP {res['status']}")
        assert_true(res["body"].get("code") == 0, f"code {res['body'].get('code')}")
        data = res["body"]["data"]
        spend_api_key = data["key"]
        spend_key_uid = data["uid"]

    def list_key():
        res = request("GET", "/api/keys", token=token)
        assert_true(res["status"] == 200, f"HTTP {res['status']}")
        items = res["body"]["data"]["items"]
        current = next((item for item in items if item.get("uid") == key_uid), None)
        assert_true(current is not None, "key not listed")
        assert_true(current.get("rate_limit") == 2, "rate_limit not listed")
        assert_true(current.get("daily_spend_limit") == 0.001, "daily_spend_limit not listed")

    def call_expect_402(remaining):
        res = request(
            "POST",
            "/api/v1/chat/completions",
            {
                "model": MODEL,
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 8,
            },
            api_key=api_key,
        )
        assert_true(res["status"] == 402, f"HTTP {res['status']}")
        assert_true(res["headers"].get("x-ratelimit-limit") == "2", "missing limit header")
        assert_true(
            res["headers"].get("x-ratelimit-remaining") == str(remaining),
            f"remaining {res['headers'].get('x-ratelimit-remaining')}",
        )

    def call_expect_429():
        res = request(
            "POST",
            "/api/v1/chat/completions",
            {
                "model": MODEL,
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 8,
            },
            api_key=api_key,
        )
        assert_true(res["status"] == 429, f"HTTP {res['status']}")
        assert_true(res["body"]["error"]["code"] == "RATE_LIMIT_EXCEEDED", "wrong error code")
        assert_true(res["headers"].get("retry-after") is not None, "missing Retry-After")

    def seed_daily_spend():
        sql = f"""
WITH key_row AS (
    SELECT id, user_id FROM api_keys WHERE uid = {sql_quote(spend_key_uid)}
),
provider_row AS (
    SELECT provider_id FROM provider_models WHERE model_id = {sql_quote(MODEL)} LIMIT 1
)
INSERT INTO api_calls (
    request_id, api_key_id, user_id, model_id, provider_id,
    input_tokens, output_tokens, cost_points, latency_ms, status_code, error_msg
)
SELECT
    {sql_quote(synthetic_request_id)}, key_row.id, key_row.user_id, {sql_quote(MODEL)}, provider_row.provider_id,
    1, 1, 0.001, 1, 200, 'synthetic daily spend smoke'
FROM key_row CROSS JOIN provider_row
RETURNING id;
"""
        output = psql(sql)
        assert_true(has_numeric_line(output), "synthetic spend not inserted")

    def call_expect_daily_limit():
        res = request(
            "POST",
            "/api/v1/chat/completions",
            {
                "model": MODEL,
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 8,
            },
            api_key=spend_api_key,
        )
        assert_true(res["status"] == 429, f"HTTP {res['status']}")
        assert_true(
            res["body"]["error"]["code"] == "KEY_DAILY_LIMIT_EXCEEDED",
            f"code {res['body']['error'].get('code')}",
        )

    def cleanup_synthetic_spend():
        output = psql(
            f"DELETE FROM api_calls WHERE request_id = {sql_quote(synthetic_request_id)} RETURNING id;"
        )
        assert_true(has_numeric_line(output), "synthetic spend not deleted")

    def delete_key():
        res = request("DELETE", f"/api/keys/{key_uid}", token=token)
        assert_true(res["status"] == 200, f"HTTP {res['status']}")
        assert_true(res["body"].get("code") == 0, f"code {res['body'].get('code')}")

    def delete_spend_key():
        res = request("DELETE", f"/api/keys/{spend_key_uid}", token=token)
        assert_true(res["status"] == 200, f"HTTP {res['status']}")
        assert_true(res["body"].get("code") == 0, f"code {res['body'].get('code')}")

    run("health", health)
    run("cleanup_old_synthetic_spend", cleanup_old_synthetic_spend)
    run("register_temp_user", register)
    run("login_temp_user", login)
    run("create_limited_key", create_key)
    run("create_daily_spend_key", create_spend_key)
    run("list_limit_fields", list_key)
    run("first_call_has_rate_headers", lambda: call_expect_402(1))
    run("second_call_has_rate_headers", lambda: call_expect_402(0))
    run("third_call_is_limited", call_expect_429)
    run("seed_daily_spend", seed_daily_spend)
    run("daily_spend_limit_blocks_call", call_expect_daily_limit)
    run("cleanup_synthetic_spend", cleanup_synthetic_spend)
    run("delete_temp_key", delete_key)
    run("delete_daily_spend_key", delete_spend_key)

    print(f"SUMMARY passed={passed} failed={failed}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
