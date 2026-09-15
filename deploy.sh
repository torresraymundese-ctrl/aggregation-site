#!/bin/bash
# 部署脚本 - 一键部署海外站

set -e

echo "=== 海外站部署脚本 ==="

# 1. 检查环境
if [ ! -d "overseas-site/dist" ]; then
    echo "[错误] 前端未构建，请先运行 npm run build"
    exit 1
fi

if [ ! -d "overseas-api/target" ]; then
    echo "[错误] 后端未编译"
    exit 1
fi

# 2. 检查环境变量
if [ -z "$DATABASE_URL" ]; then
    echo "[错误] 请设置 DATABASE_URL"
    exit 1
fi

# 3. 构建前端 (可选)
read -p "需要重新构建前端吗? (y/n): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "[1/3] 构建前端..."
    cd overseas-site && npm run build && cd ..
fi

# 4. 构建后端 (可选)
read -p "需要重新构建后端吗? (y/n): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "[2/3] 构建后端..."
    cd overseas-api && cargo build --release && cd ..
fi

# 5. 部署
echo "[3/3] 启动服务..."
docker-compose -f docker-compose-deploy.yml up -d

echo ""
echo "=== 部署完成 ==="
echo "访问: http://localhost"
echo "API:  http://localhost/api"
echo "后端: http://localhost:8080"
echo ""
echo "停止: docker-compose -f docker-compose-deploy.yml down"