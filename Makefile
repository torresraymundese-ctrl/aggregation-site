.PHONY: dev build up down logs restart clean test

# Development environment
dev:
	docker-compose up -d --build
	@echo "Server running on http://localhost:8080"

# Build production image
build:
	docker build -t overseas-api:latest .

# Start services
up:
	docker-compose up -d

# Stop services
down:
	docker-compose down

# View logs
logs:
	docker-compose logs -f

# Restart services
restart:
	docker-compose restart

# Clean up
clean:
	docker-compose down -v
	rm -rf overseas-api/target

# Shell into container
shell:
	docker-compose exec app sh

# Run tests
test:
	cd overseas-api && cargo test

# Check code
check:
	cd overseas-api && cargo check

# Build release
release:
	cd overseas-api && cargo build --release