# Production build
FROM rust:1.75-alpine AS builder

WORKDIR /app

# Install build dependencies
RUN apk add --no-cache musl-dev openssl-dev pkgconfig

COPY overseas-api/Cargo.toml ./
COPY overseas-api/src ./src

RUN cargo build --release

# Runtime
FROM alpine:3.19

WORKDIR /app

# Install runtime dependencies
RUN apk add --no-cache ca-certificates openssl

COPY --from=builder /app/target/release/overseas-api ./overseas-api

EXPOSE 8080

CMD ["./overseas-api"]