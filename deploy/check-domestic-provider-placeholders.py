#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from urllib.error import URLError
from urllib.request import Request, urlopen


DEFAULT_BASE = "http://127.0.0.1:8080"
DEFAULT_ENV_PATH = Path(__file__).resolve().parents[1] / "overseas-api" / ".env"

DOMESTIC_PROVIDERS = [
    ("Volcengine", "PROVIDER_VOLCENGINE_API_KEY", "doubao-placeholder"),
    ("DeepSeek", "PROVIDER_DEEPSEEK_API_KEY", "deepseek"),
    ("Zhipu GLM", "PROVIDER_ZHIPU_API_KEY", "glm-placeholder"),
    ("Alibaba Qwen", "PROVIDER_QWEN_API_KEY", "qwen-placeholder"),
    ("Moonshot Kimi", "PROVIDER_MOONSHOT_API_KEY", "kimi-placeholder"),
    ("MiniMax", "PROVIDER_MINIMAX_API_KEY", "minimax-placeholder"),
    ("StepFun", "PROVIDER_STEPFUN_API_KEY", "step-placeholder"),
]


def load_env(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    if not path.exists():
        return values
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.strip()] = value.strip()
    return values


def request_json(base: str, path: str) -> dict:
    req = Request(base.rstrip("/") + path)
    with urlopen(req, timeout=10) as resp:
        return json.loads(resp.read().decode("utf-8"))


def main() -> int:
    base = os.environ.get("APP_BASE", DEFAULT_BASE)
    env_path = Path(os.environ.get("ENV_PATH", str(DEFAULT_ENV_PATH)))
    env = load_env(env_path)

    try:
        payload = request_json(base, "/models")
    except (URLError, TimeoutError, json.JSONDecodeError) as exc:
        print(f"FAIL /models unavailable: {exc}")
        return 1

    models = payload.get("data", {}).get("items", [])
    if not models:
        print("FAIL /models returned no models")
        return 1

    ok = True
    for provider_name, env_name, model_prefix in DOMESTIC_PROVIDERS:
        has_model = any(model_prefix in item.get("model_id", "") for item in models)
        has_key = bool(env.get(env_name) or os.environ.get(env_name))
        model_status = "OK" if has_model else "MISSING_MODEL"
        key_status = "KEY_READY" if has_key else "KEY_MISSING"
        print(f"{provider_name}: {model_status}, {key_status}")
        ok = ok and has_model

    if not ok:
        print("FAIL domestic provider placeholders are incomplete")
        return 1

    print("PASS domestic provider placeholders are present")
    return 0


if __name__ == "__main__":
    sys.exit(main())
