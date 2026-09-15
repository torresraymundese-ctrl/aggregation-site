from __future__ import annotations

import subprocess
from pathlib import Path


ENV_PATH = Path("/opt/openbridge/.env.production")


def load_env(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.strip()] = value.strip()
    return values


def check(name: str, key_name: str, command: list[str], env: dict[str, str]) -> None:
    if not env.get(key_name):
        print(f"{name}: MISSING")
        return

    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=40)
    except Exception as exc:
        print(f"{name}: ERROR {type(exc).__name__}")
        return

    status = (result.stdout or "").strip()[-3:]
    print(f"{name}: HTTP {status}")


def main() -> None:
    env = load_env(ENV_PATH)

    check(
        "OpenAI",
        "PROVIDER_OPENAI_API_KEY",
        [
            "curl",
            "-sS",
            "-m",
            "30",
            "-o",
            "/tmp/openai_models_check.json",
            "-w",
            "%{http_code}",
            "-H",
            f"Authorization: Bearer {env.get('PROVIDER_OPENAI_API_KEY', '')}",
            "https://api.openai.com/v1/models",
        ],
        env,
    )

    check(
        "Anthropic",
        "PROVIDER_ANTHROPIC_API_KEY",
        [
            "curl",
            "-sS",
            "-m",
            "30",
            "-o",
            "/tmp/anthropic_models_check.json",
            "-w",
            "%{http_code}",
            "-H",
            f"x-api-key: {env.get('PROVIDER_ANTHROPIC_API_KEY', '')}",
            "-H",
            "anthropic-version: 2023-06-01",
            "https://api.anthropic.com/v1/models?limit=1",
        ],
        env,
    )

    check(
        "Google",
        "PROVIDER_GOOGLE_API_KEY",
        [
            "curl",
            "-sS",
            "-m",
            "30",
            "-o",
            "/tmp/google_models_check.json",
            "-w",
            "%{http_code}",
            "-H",
            f"x-goog-api-key: {env.get('PROVIDER_GOOGLE_API_KEY', '')}",
            "https://generativelanguage.googleapis.com/v1beta/models",
        ],
        env,
    )


if __name__ == "__main__":
    main()
