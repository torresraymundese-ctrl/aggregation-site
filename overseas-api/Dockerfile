# ========================================
# 生产环境 Dockerfile
# ========================================
FROM rust:1-bookworm AS builder

WORKDIR /app

# 复制依赖文件
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo/

# 下载依赖（利用缓存），只 fetch，不生成假二进制。
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo fetch && rm -rf src

# 复制源码
COPY . .

# 构建
RUN cargo build --release

# ========================================
# 生产镜像
# ========================================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/overseas-api /app/

EXPOSE 8080

ENV DATABASE_URL=""
ENV PORT=8080

CMD ["/app/overseas-api"]
