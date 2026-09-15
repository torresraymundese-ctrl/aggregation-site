const { spawnSync } = require('child_process');
const path = require('path');

const baseUrl = (process.env.APP_BASE || '').replace(/\/$/, '');
const allowWrite = process.env.SMOKE_ALLOW_WRITE === '1';

function fail(message) {
  console.error(`服务器冒烟测试失败：${message}`);
  process.exit(1);
}

if (!baseUrl) {
  fail('缺少 APP_BASE，例如 APP_BASE=https://your-domain.com');
}

if (!allowWrite) {
  fail('缺少 SMOKE_ALLOW_WRITE=1。本测试会注册测试账号并创建 API Key。');
}

if (baseUrl.includes('127.0.0.1') || baseUrl.includes('localhost')) {
  fail('服务器冒烟测试必须使用线上域名；本地测试请运行 smoke:local。');
}

if (!baseUrl.startsWith('https://')) {
  fail('服务器冒烟测试必须使用 HTTPS 域名。');
}

const localSmoke = spawnSync(process.execPath, [path.join(__dirname, 'smoke-local.cjs')], {
  stdio: 'inherit',
  env: { ...process.env, APP_BASE: baseUrl },
});

if (localSmoke.status !== 0) {
  process.exit(localSmoke.status || 1);
}

async function request(url, options = {}) {
  const response = await fetch(url, options);
  const text = await response.text();
  let body = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = text;
  }
  return { status: response.status, body };
}

function jsonHeaders(extra = {}) {
  return { 'Content-Type': 'application/json', ...extra };
}

async function verifyAdmin() {
  const email = process.env.SMOKE_ADMIN_EMAIL;
  const password = process.env.SMOKE_ADMIN_PASSWORD;

  if (!email && !password) {
    console.log('服务器管理员冒烟测试：跳过，未设置 SMOKE_ADMIN_EMAIL 和 SMOKE_ADMIN_PASSWORD');
    return;
  }

  if (!email || !password) {
    fail('SMOKE_ADMIN_EMAIL 和 SMOKE_ADMIN_PASSWORD 必须同时设置。');
  }

  const apiBase = `${baseUrl}/api`;
  const register = await request(`${apiBase}/auth/register`, {
    method: 'POST',
    headers: jsonHeaders(),
    body: JSON.stringify({ email, nickname: 'Smoke Admin', password }),
  });

  if (![0, 409].includes(register.body?.code)) {
    fail(`管理员注册返回异常 code=${register.body?.code}`);
  }

  const login = await request(`${apiBase}/auth/login`, {
    method: 'POST',
    headers: jsonHeaders(),
    body: JSON.stringify({ email, password }),
  });

  if (login.status !== 200 || login.body?.code !== 0 || !login.body?.data?.token) {
    fail(`管理员登录失败 HTTP ${login.status} code=${login.body?.code}`);
  }

  const admin = await request(`${apiBase}/admin/users`, {
    headers: { Authorization: `Bearer ${login.body.data.token}` },
  });

  if (admin.status !== 200 || admin.body?.code !== 0) {
    fail(`管理员访问失败 HTTP ${admin.status} code=${admin.body?.code}`);
  }

  console.log('服务器管理员冒烟测试：通过');
}

verifyAdmin().catch((error) => fail(error.message));
