# Relay

Relay is an HTTP gateway with a pool of workers. You send a normal HTTP request to the gateway. Every connected worker receives that request, forwards it to its own API, and the gateway returns the first response that comes back.

```
your app  --->  gateway :8080  --->  every worker  --->  that worker's API
                     ^                      |
                     +------ first reply ---+
```

Later replies for the same request are ignored.

## What you need

- Rust (edition 2021) and Cargo
- `protoc` (Protocol Buffers compiler). The build generates the gRPC code from `proto/relay.proto`.

On macOS:

```bash
brew install protobuf
```

## Build

From the repository root:

```bash
cargo build --release
```

Binaries:

- `target/release/server` — the gateway
- `target/release/client` — one worker

## Run

Start the gateway first. It listens on two ports. Both bind `0.0.0.0` and can be changed with environment variables:

| Variable | Default | Role |
| --- | --- | --- |
| `HTTP_PORT` | `8080` | HTTP entry point. Send your requests here. |
| `GRPC_PORT` | `50051` | gRPC. Workers connect here. You do not call this port yourself. |

Defaults:

```bash
./target/release/server
```

Custom HTTP port (gRPC stays on `50051`):

```bash
HTTP_PORT=9090 ./target/release/server
```

Both ports:

```bash
HTTP_PORT=9090 GRPC_PORT=50052 ./target/release/server
```

A port value outside `1`–`65535` makes the server exit with an error. Callers such as `curl` must use `HTTP_PORT`.

Then start one worker per destination API. Each worker needs `TARGET_URL`.

Local gateway, default gRPC port:

```bash
TARGET_URL=http://api-a.example.com ./target/release/client
TARGET_URL=http://api-b.example.com ./target/release/client
```

`TARGET_URL` is the base URL only. The gateway appends the path and query string from the incoming request. A trailing slash on `TARGET_URL` is removed.

Gateway on another host, or a gRPC port other than `50051`. `SERVER_ADDR` must use the same port as `GRPC_PORT`:

```bash
SERVER_ADDR=http://192.168.1.10:50052 TARGET_URL=http://api-a.example.com ./target/release/client
```

`SERVER_ADDR` defaults to `http://127.0.0.1:50051`. The worker does not use `HTTP_PORT`. Changing only the HTTP port leaves the client command unchanged.

## Call the gateway

Any method, path, query string, header, and body are accepted. Example:

```bash
curl -X POST http://127.0.0.1:8080/hello/world -d '{"msg":"hi"}'
```

That becomes a request to each worker's target:

```text
POST http://api-a.example.com/hello/world
POST http://api-b.example.com/hello/world
```

The body you sent is forwarded. The status code, headers, and body of the **first** API response are what `curl` receives.

A request to `http://127.0.0.1:8080/items/7?x=1` is forwarded as `/items/7?x=1` on the target.

## Which response you get

Every connected worker handles every request. The gateway does not pick a worker in advance and does not merge answers. The caller gets whichever response arrives first. The rest are discarded.

If the destination APIs change data, each of them will perform that change. Run a single worker when the call must happen only once.

## Errors you can see

| HTTP status | Body | Meaning |
| --- | --- | --- |
| `503` | `no gRPC clients connected` | The gateway is up, but no worker is connected. |
| `502` | `no client returned a response` | A worker accepted the request and then dropped the connection before answering. |
| `502` | `the client could not reach the target API` | A worker answered, but it could not reach its `TARGET_URL`. This is that worker's own response, returned only if it arrives first. |

There is no request timeout. If every worker stays connected and none of them answers, the HTTP call waits until one does.

## Environment

Gateway:

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `HTTP_PORT` | no | `8080` | HTTP listen port, bound on `0.0.0.0`. |
| `GRPC_PORT` | no | `50051` | gRPC listen port, bound on `0.0.0.0`. |

Worker:

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `TARGET_URL` | yes | — | Base URL of the API this worker calls. |
| `SERVER_ADDR` | no | `http://127.0.0.1:50051` | Gateway gRPC address. Must use the same port as `GRPC_PORT`. |

## Headers

Workers forward request headers except `host`, `content-length`, `transfer-encoding`, and `connection`. Those are set again by the worker when it calls the target API. Other headers, including `Content-Type` and `Authorization`, are forwarded.

For how the broadcast and the first-response rule are implemented, see [TECH.md](TECH.md).
