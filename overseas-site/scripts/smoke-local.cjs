const crypto = require('crypto');

const baseUrl = (process.env.APP_BASE || 'http://127.0.0.1:5180').replace(/\/$/, '');
const apiBase = `${baseUrl}/api`;
const isLocalTarget =
  baseUrl.includes('127.0.0.1') ||
  baseUrl.includes('localhost') ||
  baseUrl.includes('192.168.');

const results = [];
const cleanupTasks = [];

function record(name, ok, detail = '') {
  results.push({ name, ok, detail });
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
  return { status: response.status, body, text };
}

async function expect(name, fn) {
  try {
    const detail = await fn();
    record(name, true, detail || '');
  } catch (error) {
    record(name, false, error.message);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function jsonHeaders(extra = {}) {
  return { 'Content-Type': 'application/json', ...extra };
}

function base64Url(input) {
  return Buffer.from(input)
    .toString('base64')
    .replace(/=/g, '')
    .replace(/\+/g, '-')
    .replace(/\//g, '_');
}

function createLocalAdminToken() {
  const secret = process.env.SMOKE_JWT_SECRET || 'dev-secret-change-in-production';
  const now = Math.floor(Date.now() / 1000);
  const header = { alg: 'HS256', typ: 'JWT' };
  const payload = {
    sub: Number(process.env.SMOKE_ADMIN_ID || 1),
    uid: process.env.SMOKE_ADMIN_UID || 'ADMLOCAL',
    email: process.env.SMOKE_ADMIN_EMAIL || 'admin@nexus.local',
    iat: now,
    exp: now + 3600,
  };
  const head = base64Url(JSON.stringify(header));
  const body = base64Url(JSON.stringify(payload));
  const signature = crypto
    .createHmac('sha256', secret)
    .update(`${head}.${body}`)
    .digest('base64')
    .replace(/=/g, '')
    .replace(/\+/g, '-')
    .replace(/\//g, '_');
  return `${head}.${body}.${signature}`;
}

function decodeJwtSub(token) {
  const payload = token.split('.')[1];
  if (!payload) return null;
  const decoded = JSON.parse(Buffer.from(payload.replace(/-/g, '+').replace(/_/g, '/'), 'base64').toString('utf8'));
  return decoded.sub || null;
}

function adminHeaders(adminToken) {
  return { Authorization: `Bearer ${adminToken}` };
}

async function cleanup() {
  const failures = [];
  for (const task of cleanupTasks.reverse()) {
    try {
      await task();
    } catch (error) {
      failures.push(error.message);
    }
  }
  if (failures.length) {
    throw new Error(failures.join('; '));
  }
  return `${cleanupTasks.length} 项`;
}

async function main() {
  let token = '';
  let apiKey = '';
  let keyUid = '';
  let userId = null;
  const adminToken = process.env.SMOKE_ADMIN_TOKEN || (isLocalTarget ? createLocalAdminToken() : '');

  await expect('前端首页可访问', async () => {
    const res = await request(`${baseUrl}/`);
    assert(res.status === 200, `HTTP ${res.status}`);
  });

  const pageRoutes = [
    '/login',
    '/register',
    '/dashboard',
    '/keys',
    '/models',
    '/purchase',
    '/balance',
    '/logs',
    '/docs',
    '/settings',
    '/admin',
    '/admin/users',
    '/admin/orders',
    '/admin/logs',
    '/admin/models',
    '/admin/providers',
    '/admin/reports',
    '/admin/risk',
    '/admin/audit-logs',
  ];

  await expect('前端主要路由可访问', async () => {
    for (const route of pageRoutes) {
      const res = await request(`${baseUrl}${route}`);
      assert(res.status === 200, `${route} HTTP ${res.status}`);
    }
    return `${pageRoutes.length} 个路由`;
  });

  await expect('API 健康检查', async () => {
    const res = await request(`${apiBase}/health`);
    assert(res.status === 200 && res.text === 'OK', `HTTP ${res.status}`);
  });

  await expect('OpenAPI 规格', async () => {
    const res = await request(`${apiBase}/openapi.json`);
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.openapi === '3.1.0', `openapi ${res.body?.openapi}`);
    assert(Boolean(res.body?.paths?.['/v1/chat/completions']), '缺少 chat completions 路径');
  });

  await expect('模型列表', async () => {
    const res = await request(`${apiBase}/models`);
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
    assert((res.body?.data?.items || []).length >= 17, '模型少于 17 个');
  });

  await expect('套餐列表', async () => {
    const res = await request(`${apiBase}/packages`);
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
    assert((res.body?.data?.items || []).length > 0, '套餐为空');
  });

  const stamp = Date.now();
  const email = `smoke_${stamp}@nexus.local`;
  const password = 'Test123456';

  await expect('注册账号', async () => {
    const res = await request(`${apiBase}/auth/register`, {
      method: 'POST',
      headers: jsonHeaders(),
      body: JSON.stringify({ email, nickname: `smoke_${stamp}`, password }),
    });
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
    assert(Boolean(res.body?.data?.token), '未返回 token');
  });

  await expect('错误密码提示', async () => {
    const res = await request(`${apiBase}/auth/login`, {
      method: 'POST',
      headers: jsonHeaders(),
      body: JSON.stringify({ email, password: 'Wrong123456' }),
    });
    assert(res.body?.code === 401, `code ${res.body?.code}`);
  });

  await expect('登录账号', async () => {
    const res = await request(`${apiBase}/auth/login`, {
      method: 'POST',
      headers: jsonHeaders(),
      body: JSON.stringify({ email, password }),
    });
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
    token = res.body?.data?.token || '';
    userId = decodeJwtSub(token);
    assert(Boolean(token), '未返回 token');
    assert(Boolean(userId), '未解析到用户 ID');
  });

  if (adminToken && userId) {
    cleanupTasks.push(async () => {
      const res = await request(`${apiBase}/admin/users/${userId}/disable`, {
        method: 'PUT',
        headers: adminHeaders(adminToken),
      });
      assert(res.status === 200 && res.body?.code === 0, `停用临时账号失败 HTTP ${res.status}`);
    });
  }

  await expect('登录态查询', async () => {
    const res = await request(`${apiBase}/auth/me`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
  });

  await expect('余额查询', async () => {
    const res = await request(`${apiBase}/balance`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
    assert(typeof res.body?.data?.balance !== 'undefined', '余额字段为空');
  });

  await expect('创建 API Key', async () => {
    const res = await request(`${apiBase}/keys`, {
      method: 'POST',
      headers: jsonHeaders({ Authorization: `Bearer ${token}` }),
      body: JSON.stringify({
        name: `Smoke Key ${stamp}`,
        rate_limit: 500,
        daily_spend_limit: 0.5,
        models: ['gpt-4.1-mini'],
      }),
    });
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
    apiKey = res.body?.data?.key || '';
    keyUid = res.body?.data?.uid || '';
    assert(Boolean(apiKey), '未返回完整 Key');
    assert(Boolean(keyUid), '未返回 Key UID');
    cleanupTasks.push(async () => {
      const res = await request(`${apiBase}/keys/${keyUid}`, {
        method: 'DELETE',
        headers: { Authorization: `Bearer ${token}` },
      });
      assert([200, 404].includes(res.status), `删除临时 Key 失败 HTTP ${res.status}`);
    });
  });

  await expect('API Key 列表只显示前缀', async () => {
    const res = await request(`${apiBase}/keys`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
    const items = res.body?.data?.items || [];
    assert(items.length > 0, 'Key 列表为空');
    assert(!JSON.stringify(items).includes(apiKey), '列表泄露完整 Key');
  });

  await expect('模型白名单拦截', async () => {
    const res = await request(`${apiBase}/v1/chat/completions`, {
      method: 'POST',
      headers: jsonHeaders({ 'X-API-Key': apiKey }),
      body: JSON.stringify({
        model: 'gpt-4o-mini',
        messages: [{ role: 'user', content: 'ping' }],
      }),
    });
    assert(res.status === 403, `HTTP ${res.status}`);
    assert(res.body?.error?.code === 'MODEL_NOT_ALLOWED', `code ${res.body?.error?.code}`);
  });

  await expect('有效 Key 进入网关并触发余额校验', async () => {
    const res = await request(`${apiBase}/v1/chat/completions`, {
      method: 'POST',
      headers: jsonHeaders({ 'X-API-Key': apiKey }),
      body: JSON.stringify({
        model: 'gpt-4.1-mini',
        messages: [{ role: 'user', content: 'ping' }],
        max_tokens: 8,
      }),
    });
    assert(res.status === 402, `HTTP ${res.status}`);
    assert(res.body?.error?.code === 'INSUFFICIENT_BALANCE', `code ${res.body?.error?.code}`);
  });

  await expect('API Key 禁用启用删除', async () => {
    const headers = { Authorization: `Bearer ${token}` };
    const disabled = await request(`${apiBase}/keys/${keyUid}/disable`, { method: 'PUT', headers });
    assert(disabled.status === 200 && disabled.body?.code === 0, `disable HTTP ${disabled.status}`);

    const afterDisable = await request(`${apiBase}/v1/chat/completions`, {
      method: 'POST',
      headers: jsonHeaders({ 'X-API-Key': apiKey }),
      body: JSON.stringify({
        model: 'gpt-4.1-mini',
        messages: [{ role: 'user', content: 'ping' }],
      }),
    });
    assert(afterDisable.status === 401, `禁用后 HTTP ${afterDisable.status}`);

    const enabled = await request(`${apiBase}/keys/${keyUid}/enable`, { method: 'PUT', headers });
    assert(enabled.status === 200 && enabled.body?.code === 0, `enable HTTP ${enabled.status}`);

    const afterEnable = await request(`${apiBase}/v1/chat/completions`, {
      method: 'POST',
      headers: jsonHeaders({ 'X-API-Key': apiKey }),
      body: JSON.stringify({
        model: 'gpt-4.1-mini',
        messages: [{ role: 'user', content: 'ping' }],
      }),
    });
    assert(afterEnable.status === 402, `启用后 HTTP ${afterEnable.status}`);

    const deleted = await request(`${apiBase}/keys/${keyUid}`, { method: 'DELETE', headers });
    assert(deleted.status === 200 && deleted.body?.code === 0, `delete HTTP ${deleted.status}`);
    cleanupTasks.pop();

    const afterDelete = await request(`${apiBase}/v1/chat/completions`, {
      method: 'POST',
      headers: jsonHeaders({ 'X-API-Key': apiKey }),
      body: JSON.stringify({
        model: 'gpt-4.1-mini',
        messages: [{ role: 'user', content: 'ping' }],
      }),
    });
    assert(afterDelete.status === 401, `删除后 HTTP ${afterDelete.status}`);
  });

  await expect('无效 Key 被拒绝', async () => {
    const res = await request(`${apiBase}/v1/chat/completions`, {
      method: 'POST',
      headers: jsonHeaders({ 'X-API-Key': 'sk-invalid' }),
      body: JSON.stringify({
        model: 'gpt-4.1-mini',
        messages: [{ role: 'user', content: 'ping' }],
      }),
    });
    assert(res.status === 401, `HTTP ${res.status}`);
    assert(res.body?.code === 401, `code ${res.body?.code}`);
  });

  await expect('普通用户不能访问管理端', async () => {
    const res = await request(`${apiBase}/admin/users`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    assert(res.status === 403, `HTTP ${res.status}`);
  });

  await expect('未登录不能访问管理端', async () => {
    const res = await request(`${apiBase}/admin/orders`);
    assert(res.status === 401, `HTTP ${res.status}`);
  });

  await expect('调用日志接口可访问', async () => {
    const res = await request(`${apiBase}/logs/calls`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    assert(res.status === 200, `HTTP ${res.status}`);
    assert(res.body?.code === 0, `code ${res.body?.code}`);
  });

  if (adminToken) {
    await expect('管理端只读接口', async () => {
      const headers = adminHeaders(adminToken);
      for (const path of [
        '/admin/users',
        '/admin/orders',
        '/admin/models',
        '/admin/providers',
        '/admin/reports/profit',
        '/admin/audit-logs',
        '/admin/risk/users',
      ]) {
        const res = await request(`${apiBase}${path}`, { headers });
        assert(res.status === 200 && res.body?.code === 0, `${path} HTTP ${res.status} code ${res.body?.code}`);
      }
    });

    await expect('管理端模型定价配置', async () => {
      const headers = adminHeaders(adminToken);
      const models = await request(`${apiBase}/admin/models`, { headers });
      const item = models.body?.data?.items?.[0];
      assert(Boolean(item), '模型为空');
      const res = await request(`${apiBase}/admin/models/${item.id}/pricing`, {
        method: 'PUT',
        headers: jsonHeaders(headers),
        body: JSON.stringify({
          upstream_input_rate: Number(item.upstream_input_rate),
          upstream_output_rate: Number(item.upstream_output_rate),
          margin_rate: Number(item.margin_rate),
          status: Number(item.status),
        }),
      });
      assert(res.status === 200 && res.body?.code === 0, `HTTP ${res.status} code ${res.body?.code}`);
    });

    await expect('管理端供应商配置', async () => {
      const headers = adminHeaders(adminToken);
      const providers = await request(`${apiBase}/admin/providers`, { headers });
      const item = providers.body?.data?.items?.[0];
      assert(Boolean(item), '供应商为空');
      const res = await request(`${apiBase}/admin/providers/${item.id}`, {
        method: 'PUT',
        headers: jsonHeaders(headers),
        body: JSON.stringify({
          name: item.name,
          base_url: item.base_url,
          api_key_env: item.api_key_env,
          status: Number(item.status),
          sort: Number(item.sort),
        }),
      });
      assert(res.status === 200 && res.body?.code === 0, `HTTP ${res.status} code ${res.body?.code}`);
    });

    await expect('管理端异常更新返回业务错误', async () => {
      const headers = adminHeaders(adminToken);
      const pricing = await request(`${apiBase}/admin/models/999999999/pricing`, {
        method: 'PUT',
        headers: jsonHeaders(headers),
        body: JSON.stringify({
          upstream_input_rate: 0,
          upstream_output_rate: 0,
          margin_rate: 0,
          status: 0,
        }),
      });
      assert(pricing.body?.code === 404, `pricing code ${pricing.body?.code}`);
      const provider = await request(`${apiBase}/admin/providers/999999999`, {
        method: 'PUT',
        headers: jsonHeaders(headers),
        body: JSON.stringify({
          name: 'Missing',
          base_url: 'https://example.com',
          api_key_env: 'MISSING_KEY',
          status: 0,
          sort: 0,
        }),
      });
      assert(provider.body?.code === 404, `provider code ${provider.body?.code}`);
    });
  }

  await expect('清理临时测试数据', cleanup);

  const passed = results.filter((item) => item.ok).length;
  const failed = results.length - passed;

  console.log(`海外站本地冒烟测试：通过 ${passed}，失败 ${failed}`);
  for (const item of results.filter((result) => !result.ok)) {
    console.log(`- ${item.name}: ${item.detail}`);
  }

  if (failed > 0) process.exit(1);
}

main().catch((error) => {
  console.error(`海外站本地冒烟测试异常：${error.message}`);
  process.exit(1);
});
