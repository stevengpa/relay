# Relay Service

This project contains a relay service with server and client binaries.

## Prerequisites

- Docker or Podman installed
- Port 8080 (server) and 9090 (client) should be available on host machine

## Running the Server

### Using Docker:
```bash
docker-compose -f docker-compose.server.yml up -d
```

### Using Podman:
```bash
podman-compose -f podman-compose.server.yml up -d
```

### Environment Variables:
- `HTTP_PORT` (default: 8080) - HTTP port for the server to listen on
- `GRPC_PORT` (default: 9090) - GRPC port for the server to listen on
- `LOG_LEVEL` (default: info) - Logging level

## Running the Client

### Using Docker:
```bash
docker-compose -f docker-compose.client.yml up -d
```

### Using Podman:
```bash
podman-compose -f podman-compose.client.yml up -d
```

### Environment Variables:
- `CLIENT_PORT` (default: 9090) - Port for the client to listen on
- `SERVER_ADDR` (default: relay-server:9090) - Address of the server (GRPC port)
- `TARGET_URL` (default: http://relay-server:8080) - HTTP endpoint of the server
- `LOG_LEVEL` (default: info) - Logging level

## Ports

- Server listens on port 8080 (accessible from host)
- Client listens on port 9090 (accessible from host)

## Notes

- The compose files assume the binaries are located in `binaries/v0.0.1/x86_64-unknown-linux-gnu/`
- Both server and client run in Ubuntu 20.04 containers
- The client will automatically connect to the server when started