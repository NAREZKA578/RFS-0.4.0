# Makefile for the RFS server supervisor (надзиратель).
#
# Requires Go 1.22+. No external dependencies.
#
# Usage:
#   make build      Build the rfs-orchestrator binary
#   make test       Run all tests
#   make vet        Run go vet
#   make clean      Remove build artifacts
#   make run        Build and run with --server <path> start default

BINARY := rfs-orchestrator
SRC    := ./...

.PHONY: build test vet clean run help

help:
	@echo "RFS Supervisor (надзиратель) — make targets:"
	@echo "  make build   Build the $(BINARY) binary"
	@echo "  make test    Run all tests"
	@echo "  make vet     Run go vet (static analysis)"
	@echo "  make clean   Remove build artifacts"
	@echo "  make run     Build and start a profile (requires --server)"

build:
	@go build -o $(BINARY) .

test:
	@go test -v ./...

vet:
	@go vet ./...

clean:
	@rm -f $(BINARY) $(BINARY).exe
	@rm -rf profiles/

run: build
	@if [ -z "$(SERVER)" ]; then \
		echo "Usage: make run SERVER=/path/to/rfs-server"; \
		exit 1; \
	fi
	./$(BINARY) --server "$(SERVER)" start default
