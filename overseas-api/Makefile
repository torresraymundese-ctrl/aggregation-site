.PHONY: help build run test docker-build docker-run docker-stop deploy clean

PROJECT_NAME := overseas-api
VERSION := $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")

help:
	@echo "Available commands:"
	@echo "  make build        - Build release binary"
	@echo "  make test       - Run tests"
	@echo "  make docker-build - Build Docker image"
	@echo "  make docker-run - Run Docker container"
	@echo "  make docker-stop - Stop Docker container"
	@echo "  make deploy    - Deploy to production"

build:
	cargo build --release

test:
	cargo test

docker-build:
	docker build -t $(PROJECT_NAME):$(VERSION) .
	docker tag $(PROJECT_NAME):$(VERSION) $(PROJECT_NAME):latest

docker-run:
	docker-compose up -d

docker-stop:
	docker-compose down

clean:
	cargo clean
	docker system prune -f